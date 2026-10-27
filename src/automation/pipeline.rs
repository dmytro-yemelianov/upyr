//! Conversion phases yield deadlines instead of sleeping in the application
//! event loop. All native input, clipboard and layout calls stay on its thread.

use super::SelectionOutcome;
use crate::{config::Config, layout::Conversion};
use anyhow::Result;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) enum Request {
    Selection,
    PreviousWord,
    Undo {
        expected: String,
        replacement: String,
        #[cfg(target_os = "macos")]
        source_id: String,
    },
    Automatic {
        expected: String,
        replacement: String,
    },
}

impl Request {
    pub fn automatic(&self) -> bool {
        matches!(self, Self::Automatic { .. })
    }
    #[cfg(target_os = "macos")]
    fn tracked(&self) -> bool {
        matches!(self, Self::Automatic { .. } | Self::Undo { .. })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Selection {
    Existing,
    Word,
    Characters(usize),
}

pub(crate) trait Effects {
    fn valid(&self) -> bool;
    fn defer_cancellation(&self) -> bool {
        false
    }
    fn supports_automatic(&self) -> bool;
    fn replace_terminal(
        &mut self,
        _expected: &str,
        _replacement: &str,
    ) -> Result<Option<SelectionOutcome>> {
        Ok(None)
    }
    fn select(&mut self, selection: Selection) -> Result<()>;
    fn copy(&mut self) -> Result<()>;
    fn read_copy(&mut self) -> Option<String>;
    fn convert(&self, selected: &str, request: &Request) -> Conversion;
    fn write(&mut self, replacement: &str) -> Result<()>;
    fn owns_clipboard(&mut self) -> bool;
    fn paste(&mut self, conversion: &Conversion, characters: usize) -> Result<SelectionOutcome>;
    fn recover_selection(&mut self, selection: Selection, missing: bool) -> Result<()>;
    fn stop_guard(&mut self);
    fn keep_clipboard(&mut self);
    fn restore(&mut self) -> Result<()>;
}

enum Phase {
    Working,
    Initial,
    Copy,
    Read,
    Paste {
        conversion: Conversion,
        characters: usize,
    },
    Restore,
    Done,
}

pub(crate) enum Progress {
    Waiting,
    Committed(SelectionOutcome),
    Finished(Option<SelectionOutcome>),
}

pub(crate) struct Pipeline<E: Effects> {
    effects: E,
    config: Config,
    request: Request,
    phase: Phase,
    selection: Selection,
    deadline: Instant,
}

impl<E: Effects> Pipeline<E> {
    pub fn new(effects: E, config: Config, request: Request, now: Instant) -> Self {
        Self {
            effects,
            config,
            request,
            phase: Phase::Initial,
            selection: Selection::Existing,
            deadline: now,
        }
    }

    pub fn automatic(&self) -> bool {
        self.request.automatic()
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn committed(&self) -> bool {
        matches!(self.phase, Phase::Restore | Phase::Done)
    }

    fn wait(&mut self, now: Instant, milliseconds: u64, phase: Phase) -> Progress {
        self.deadline = now + Duration::from_millis(milliseconds);
        self.phase = phase;
        Progress::Waiting
    }

    fn finish(&mut self, outcome: SelectionOutcome) -> Result<Progress> {
        self.effects.stop_guard();
        self.phase = Phase::Done;
        self.effects.restore()?;
        Ok(Progress::Finished(Some(outcome)))
    }

    pub fn cancel(&mut self) -> Result<()> {
        let recovery = if !self.committed() {
            self.effects.recover_selection(self.selection, false)
        } else {
            Ok(())
        };
        self.effects.stop_guard();
        self.phase = Phase::Done;
        let restoration = self.effects.restore();
        recovery.and(restoration)
    }

    pub fn poll(&mut self, now: Instant) -> Result<Progress> {
        let result = self.advance(now);
        if result.is_err() {
            let _ = self.cancel();
        }
        result
    }

    fn advance(&mut self, now: Instant) -> Result<Progress> {
        if !self.committed() && !self.effects.valid() {
            if self.effects.defer_cancellation() {
                self.deadline = now + Duration::from_millis(10);
                return Ok(Progress::Waiting);
            }
            self.cancel()?;
            return Ok(Progress::Finished(Some(SelectionOutcome::Cancelled)));
        }
        if now < self.deadline {
            return Ok(Progress::Waiting);
        }
        match std::mem::replace(&mut self.phase, Phase::Working) {
            Phase::Initial => {
                self.selection = match &self.request {
                    Request::Selection => Selection::Existing,
                    Request::PreviousWord => Selection::Word,
                    Request::Automatic {
                        expected,
                        replacement,
                    }
                    | Request::Undo {
                        expected,
                        replacement,
                        ..
                    } => {
                        if expected.is_empty() || !self.effects.supports_automatic() {
                            return self.finish(SelectionOutcome::NoSelection);
                        }
                        if replacement.is_empty() || replacement == expected {
                            return self.finish(SelectionOutcome::NoConvertibleText);
                        }
                        if let Some(outcome) =
                            self.effects.replace_terminal(expected, replacement)?
                        {
                            return self.finish(outcome);
                        }
                        Selection::Characters(expected.chars().count())
                    }
                };
                self.effects.select(self.selection)?;
                if !matches!(self.selection, Selection::Existing) {
                    return Ok(self.wait(now, 30, Phase::Copy));
                }
                self.effects.copy()?;
                Ok(self.wait(now, self.config.copy_delay_ms, Phase::Read))
            }
            Phase::Copy => {
                self.effects.copy()?;
                Ok(self.wait(now, self.config.copy_delay_ms, Phase::Read))
            }
            Phase::Read => {
                let Some(selected) = self.effects.read_copy() else {
                    self.effects.recover_selection(self.selection, true)?;
                    return self.finish(SelectionOutcome::NoSelection);
                };
                if let Request::Automatic { expected, .. } | Request::Undo { expected, .. } =
                    &self.request
                {
                    if selected != *expected && selected.trim() != expected.trim() {
                        self.effects.recover_selection(self.selection, false)?;
                        return self.finish(SelectionOutcome::TextMismatch);
                    }
                }
                let conversion = self.effects.convert(&selected, &self.request);
                if !conversion.changed {
                    return self.finish(SelectionOutcome::NoConvertibleText);
                }
                let characters = selected.chars().count();
                self.effects.write(&conversion.text)?;
                Ok(self.wait(
                    now,
                    self.config.paste_delay_ms,
                    Phase::Paste {
                        conversion,
                        characters,
                    },
                ))
            }
            Phase::Paste {
                conversion,
                characters,
            } => {
                if !self.effects.owns_clipboard() {
                    self.effects.recover_selection(self.selection, false)?;
                    return self.finish(SelectionOutcome::ClipboardChanged);
                }
                let outcome = self.effects.paste(&conversion, characters)?;
                self.effects.stop_guard();
                if self.config.restore_clipboard {
                    self.wait(now, self.config.restore_delay_ms, Phase::Restore);
                    Ok(Progress::Committed(outcome))
                } else {
                    self.effects.keep_clipboard();
                    self.phase = Phase::Done;
                    Ok(Progress::Finished(Some(outcome)))
                }
            }
            Phase::Restore => {
                self.phase = Phase::Done;
                self.effects.restore()?;
                Ok(Progress::Finished(None))
            }
            Phase::Done => {
                self.phase = Phase::Done;
                Ok(Progress::Finished(None))
            }
            Phase::Working => unreachable!("conversion phases are not reentrant"),
        }
    }
}

impl<E: Effects> Drop for Pipeline<E> {
    fn drop(&mut self) {
        // Quit or an event-loop failure must not strand Upyr's clipboard value.
        let _ = self.cancel();
    }
}

#[cfg(target_os = "macos")]
pub(crate) type ConversionTask = Pipeline<Desktop>;

#[cfg(target_os = "macos")]
pub(crate) struct Desktop {
    config: Config,
    restore_source: Option<String>,
    enigo: enigo::Enigo,
    clipboard: arboard::Clipboard,
    guard: Option<super::input_guard::InputGuard>,
    saved: Option<super::SavedClipboard>,
    probe: Option<super::CopyProbe>,
    ownership: Option<crate::clipboard_guard::ClipboardOwnership>,
}

#[cfg(target_os = "macos")]
impl Pipeline<Desktop> {
    pub fn desktop(config: Config, request: Request) -> Result<Self> {
        use anyhow::Context;
        let terminal = request.tracked() && crate::input_context::terminal_typing_enabled(&config);
        let restore_source = match &request {
            Request::Undo { source_id, .. } => Some(source_id.clone()),
            _ => None,
        };
        let effects = Desktop {
            restore_source,
            guard: Some(super::input_guard::InputGuard::new(terminal)?),
            enigo: enigo::Enigo::new(&enigo::Settings::default())
                .context("could not connect to desktop input")?,
            clipboard: arboard::Clipboard::new()
                .context("could not access the system clipboard")?,
            config: config.clone(),
            saved: None,
            probe: None,
            ownership: None,
        };
        Ok(Self::new(effects, config, request, Instant::now()))
    }
}

#[cfg(target_os = "macos")]
impl Desktop {
    fn switch_layout(
        &self,
        direction: crate::layout::Direction,
    ) -> Result<Option<crate::system_layout::SystemLayout>> {
        if let Some(id) = &self.restore_source {
            match crate::system_layout::select_source(id)? {
                crate::system_layout::SwitchOutcome::Switched { .. }
                | crate::system_layout::SwitchOutcome::AlreadyActive { .. } => {
                    Ok(crate::system_layout::current()?.and_then(|source| source.layout))
                }
                _ => anyhow::bail!("the original input source is no longer available"),
            }
        } else {
            Ok(self
                .config
                .switch_layout
                .then(|| super::follow_converted_layout(direction))
                .flatten())
        }
    }
}

#[cfg(target_os = "macos")]
impl Effects for Desktop {
    fn valid(&self) -> bool {
        self.guard.as_ref().is_some_and(|guard| guard.valid())
    }
    fn defer_cancellation(&self) -> bool {
        self.guard
            .as_ref()
            .is_some_and(|guard| guard.defer_cancellation())
    }
    fn supports_automatic(&self) -> bool {
        crate::input_context::active() != crate::input_context::InputContext::Terminal
            || crate::input_context::terminal_typing_enabled(&self.config)
    }
    fn replace_terminal(
        &mut self,
        expected: &str,
        replacement: &str,
    ) -> Result<Option<SelectionOutcome>> {
        if !crate::input_context::terminal_typing_enabled(&self.config) {
            return Ok(None);
        }
        let mapping = crate::system_layout::installed_mapping().ok().flatten();
        let plan = match (self.restore_source.is_some(), mapping.as_deref()) {
            (true, Some(mapping)) => {
                super::terminal_typing::plan_undo_with_mapping(expected, replacement, Some(mapping))
            }
            (false, Some(mapping)) => {
                super::terminal_typing::plan_with_mapping(expected, replacement, Some(mapping))
            }
            (true, None) => super::terminal_typing::plan_undo(expected, replacement),
            (false, None) => super::terminal_typing::plan(expected, replacement),
        };
        let Some(plan) = plan else {
            return Ok(Some(SelectionOutcome::NoConvertibleText));
        };
        if !self.guard.as_ref().is_some_and(|guard| guard.can_type()) {
            return Ok(Some(SelectionOutcome::Cancelled));
        }
        let Some(guard) = self.guard.as_ref() else {
            return Ok(Some(SelectionOutcome::Cancelled));
        };
        let target_pid = guard.target_pid();
        let Some(target_window) = guard.target_window() else {
            return Ok(Some(SelectionOutcome::Cancelled));
        };
        let stamp = guard.input_stamp();
        let prepared = super::terminal_typing::Prepared::new(&plan)?;
        if !self.guard.as_ref().is_some_and(|guard| guard.can_type()) {
            return Ok(Some(SelectionOutcome::Cancelled));
        }
        let mut layout_switched = None;
        if !prepared.post(target_pid, target_window, stamp, || {
            if !self
                .guard
                .as_ref()
                .is_some_and(|guard| guard.has_target_focus())
            {
                return false;
            }
            // Commit the layout on the main thread after validation, before
            // the target releases the complete Backspace/typing batch.
            match self.switch_layout(self.config.direction) {
                Ok(layout) => {
                    layout_switched = layout;
                    true
                }
                Err(error) => {
                    tracing::debug!(%error, "could not restore input source");
                    false
                }
            }
        })? {
            return Ok(Some(SelectionOutcome::Cancelled));
        }
        Ok(Some(SelectionOutcome::Converted {
            direction: self.config.direction,
            characters: plan.backspaces,
            layout_switched,
        }))
    }
    fn select(&mut self, selection: Selection) -> Result<()> {
        match selection {
            Selection::Existing => Ok(()),
            Selection::Word => super::select_previous_word(&mut self.enigo),
            Selection::Characters(count) => {
                super::select_previous_characters(&mut self.enigo, count)
            }
        }
    }
    fn copy(&mut self) -> Result<()> {
        self.saved = Some(super::snapshot(&mut self.clipboard));
        self.probe = Some(super::arm_copy_probe(&mut self.clipboard)?);
        super::send_shortcut(&mut self.enigo, super::Shortcut::Copy)
    }
    fn read_copy(&mut self) -> Option<String> {
        let selected = super::read_copied_text(&mut self.clipboard, self.probe.as_ref()?)?;
        self.ownership = Some(crate::clipboard_guard::ClipboardOwnership::after_write(
            &selected,
        ));
        Some(selected)
    }
    fn convert(&self, selected: &str, request: &Request) -> Conversion {
        let replacement = match request {
            Request::Automatic { replacement, .. } | Request::Undo { replacement, .. } => {
                Some(replacement.as_str())
            }
            _ => None,
        };
        super::selection_conversion(selected, self.config.direction, replacement)
    }
    fn write(&mut self, replacement: &str) -> Result<()> {
        crate::clipboard_guard::set_temporary_text(&mut self.clipboard, replacement)?;
        self.ownership = Some(crate::clipboard_guard::ClipboardOwnership::after_write(
            replacement,
        ));
        Ok(())
    }
    fn owns_clipboard(&mut self) -> bool {
        self.ownership
            .as_ref()
            .is_some_and(|owner| owner.is_current(&mut self.clipboard))
    }
    fn paste(&mut self, conversion: &Conversion, characters: usize) -> Result<SelectionOutcome> {
        // TIS selection can replay a pending key event on macOS. Select the
        // destination before posting Cmd+V, never while that event is queued.
        let layout_switched = self.switch_layout(conversion.direction)?;
        super::send_shortcut(&mut self.enigo, super::Shortcut::Paste)?;
        Ok(SelectionOutcome::Converted {
            direction: conversion.direction,
            characters,
            layout_switched,
        })
    }
    fn recover_selection(&mut self, selection: Selection, missing: bool) -> Result<()> {
        use enigo::Keyboard;
        if !self
            .guard
            .as_ref()
            .is_some_and(|guard| guard.can_recover_selection())
        {
            return Ok(());
        }
        if self.ownership.is_none() && self.probe.is_some() {
            let _ = self.read_copy();
        }
        match (selection, missing) {
            (Selection::Existing, _) => Ok(()),
            (Selection::Word, true) => super::undo_previous_word_selection(&mut self.enigo),
            (Selection::Characters(count), true) => {
                super::undo_previous_characters_selection(&mut self.enigo, count)
            }
            (_, false) => Ok(self
                .enigo
                .key(enigo::Key::RightArrow, enigo::Direction::Click)?),
        }
    }
    fn stop_guard(&mut self) {
        self.guard = None;
    }
    fn keep_clipboard(&mut self) {
        self.saved = None;
        self.ownership = None;
    }
    fn restore(&mut self) -> Result<()> {
        if let Some(saved) = self.saved.take() {
            if self.owns_clipboard() {
                super::restore(&mut self.clipboard, saved)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Direction, convert};
    use std::{cell::RefCell, rc::Rc};

    struct State {
        valid: bool,
        recoverable: bool,
        automatic_supported: bool,
        copied: Option<String>,
        clipboard: String,
        saved: Option<String>,
        owned: Option<String>,
        selections: usize,
        copies: usize,
        recoveries: usize,
        pasted: Option<String>,
        fail_paste: bool,
        fail_restore: bool,
        fn_held: bool,
        terminal: bool,
    }

    struct Fake(Rc<RefCell<State>>);
    impl Effects for Fake {
        fn valid(&self) -> bool {
            self.0.borrow().valid
        }
        fn defer_cancellation(&self) -> bool {
            self.0.borrow().fn_held
        }
        fn supports_automatic(&self) -> bool {
            self.0.borrow().automatic_supported
        }
        fn replace_terminal(
            &mut self,
            expected: &str,
            replacement: &str,
        ) -> Result<Option<SelectionOutcome>> {
            if !self.0.borrow().terminal {
                return Ok(None);
            }
            let plan = super::super::terminal_typing::plan(expected, replacement).unwrap();
            self.0.borrow_mut().pasted = Some(plan.replacement.to_owned());
            Ok(Some(SelectionOutcome::Converted {
                direction: Direction::EnglishToUkrainian,
                characters: plan.backspaces,
                layout_switched: None,
            }))
        }
        fn select(&mut self, selection: Selection) -> Result<()> {
            if let Selection::Characters(count) = selection {
                assert!(count > 0);
            }
            if !matches!(selection, Selection::Existing) {
                self.0.borrow_mut().selections += 1;
            }
            Ok(())
        }
        fn copy(&mut self) -> Result<()> {
            let mut state = self.0.borrow_mut();
            state.copies += 1;
            state.saved = Some(state.clipboard.clone());
            if let Some(selected) = state.copied.clone() {
                state.clipboard = selected;
            }
            Ok(())
        }
        fn read_copy(&mut self) -> Option<String> {
            let mut state = self.0.borrow_mut();
            let selected = state.copied.clone()?;
            state.owned = Some(state.clipboard.clone());
            Some(selected)
        }
        fn convert(&self, selected: &str, request: &Request) -> Conversion {
            match request {
                Request::Automatic { replacement, .. } | Request::Undo { replacement, .. } => {
                    Conversion {
                        text: replacement.clone(),
                        changed: selected != replacement,
                        direction: Direction::EnglishToUkrainian,
                    }
                }
                _ => convert(selected, Direction::EnglishToUkrainian),
            }
        }
        fn write(&mut self, replacement: &str) -> Result<()> {
            let mut state = self.0.borrow_mut();
            state.clipboard = replacement.to_owned();
            state.owned = Some(replacement.to_owned());
            Ok(())
        }
        fn owns_clipboard(&mut self) -> bool {
            let state = self.0.borrow();
            state.owned.as_ref() == Some(&state.clipboard)
        }
        fn paste(
            &mut self,
            conversion: &Conversion,
            characters: usize,
        ) -> Result<SelectionOutcome> {
            let mut state = self.0.borrow_mut();
            anyhow::ensure!(!state.fail_paste, "simulated paste failure");
            state.pasted = Some(state.clipboard.clone());
            Ok(SelectionOutcome::Converted {
                direction: conversion.direction,
                characters,
                layout_switched: None,
            })
        }
        fn recover_selection(&mut self, selection: Selection, _: bool) -> Result<()> {
            let mut state = self.0.borrow_mut();
            if state.recoverable {
                if state.owned.is_none() && state.saved.is_some() && state.copied.is_some() {
                    state.owned = Some(state.clipboard.clone());
                }
                if !matches!(selection, Selection::Existing) {
                    state.recoveries += 1;
                }
            }
            Ok(())
        }
        fn stop_guard(&mut self) {}
        fn keep_clipboard(&mut self) {
            self.0.borrow_mut().saved = None;
        }
        fn restore(&mut self) -> Result<()> {
            let mut state = self.0.borrow_mut();
            if let Some(saved) = state.saved.take() {
                anyhow::ensure!(!state.fail_restore, "simulated restore failure");
                if state.owned.as_ref() == Some(&state.clipboard) {
                    state.clipboard = saved;
                }
            }
            Ok(())
        }
    }

    fn task(request: Request, restore: bool) -> (Pipeline<Fake>, Rc<RefCell<State>>, Instant) {
        let state = Rc::new(RefCell::new(State {
            valid: true,
            recoverable: true,
            automatic_supported: true,
            copied: Some("ghbdsn".to_owned()),
            clipboard: "original clipboard".to_owned(),
            saved: None,
            owned: None,
            selections: 0,
            copies: 0,
            recoveries: 0,
            pasted: None,
            fail_paste: false,
            fail_restore: false,
            fn_held: false,
            terminal: false,
        }));
        let now = Instant::now();
        let config = Config {
            restore_clipboard: restore,
            ..Config::default()
        };
        (
            Pipeline::new(Fake(Rc::clone(&state)), config, request, now),
            state,
            now,
        )
    }

    fn next(task: &mut Pipeline<Fake>) -> Result<Progress> {
        task.poll(task.deadline())
    }
    fn ready_to_paste(task: &mut Pipeline<Fake>, now: Instant) {
        assert!(matches!(task.poll(now).unwrap(), Progress::Waiting));
        if !matches!(task.selection, Selection::Existing) {
            assert!(matches!(next(task).unwrap(), Progress::Waiting));
        }
        assert!(matches!(next(task).unwrap(), Progress::Waiting));
    }

    #[test]
    fn waiting_does_not_block_or_repeat_keyboard_actions() {
        let (mut task, state, now) = task(Request::Selection, true);
        assert!(!task.automatic());
        assert!(matches!(task.poll(now).unwrap(), Progress::Waiting));
        assert_eq!(state.borrow().copies, 1);
        assert!(matches!(
            task.poll(now + Duration::from_millis(1)).unwrap(),
            Progress::Waiting
        ));
        assert_eq!(state.borrow().copies, 1);
        assert!(state.borrow().pasted.is_none());
    }

    #[test]
    fn commit_precedes_restore_and_new_user_copies_survive_cleanup() {
        let (mut task, state, now) = task(Request::Selection, true);
        ready_to_paste(&mut task, now);
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Committed(SelectionOutcome::Converted { .. })
        ));
        assert!(task.committed());
        assert_eq!(state.borrow().pasted.as_deref(), Some("привіт"));
        state.borrow_mut().clipboard = "a new user copy".to_owned();
        assert!(matches!(next(&mut task).unwrap(), Progress::Finished(None)));
        drop(task);
        assert_eq!(state.borrow().clipboard, "a new user copy");
    }

    #[test]
    fn typing_before_paste_cancels_without_moving_the_new_caret() {
        let (mut task, state, now) = task(Request::PreviousWord, true);
        ready_to_paste(&mut task, now);
        state.borrow_mut().valid = false;
        state.borrow_mut().recoverable = false;
        assert!(matches!(
            task.poll(now).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Cancelled))
        ));
        assert!(state.borrow().pasted.is_none());
        assert_eq!(state.borrow().recoveries, 0);
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn globe_cancellation_recovers_the_selection_and_clipboard() {
        let (mut task, state, now) = task(Request::PreviousWord, true);
        task.poll(now).unwrap();
        next(&mut task).unwrap();
        state.borrow_mut().valid = false; // Modifier-only activity, same focus.
        task.poll(now).unwrap();
        drop(task);
        assert_eq!(state.borrow().recoveries, 1);
        assert!(state.borrow().pasted.is_none());
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn changed_clipboard_is_neither_pasted_nor_overwritten() {
        let (mut task, state, now) = task(Request::Selection, true);
        ready_to_paste(&mut task, now);
        state.borrow_mut().clipboard = "external copy".to_owned();
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::ClipboardChanged))
        ));
        assert!(state.borrow().pasted.is_none());
        assert_eq!(state.borrow().clipboard, "external copy");
    }

    #[test]
    fn disabling_restoration_keeps_the_conversion_even_when_the_task_drops() {
        let (mut task, state, now) = task(Request::Selection, false);
        ready_to_paste(&mut task, now);
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Converted { .. }))
        ));
        drop(task);
        assert_eq!(state.borrow().clipboard, "привіт");
    }

    #[test]
    fn undo_uses_the_recorded_text_and_refuses_a_moved_or_edited_selection() {
        for (selected, matches) in [("привіт ", true), ("changed ", false)] {
            let (mut pipeline, state, now) = task(
                Request::Undo {
                    expected: "привіт ".into(),
                    replacement: "ghbdsn ".into(),
                    #[cfg(target_os = "macos")]
                    source_id: "ABC".into(),
                },
                true,
            );
            state.borrow_mut().copied = Some(selected.into());
            pipeline.poll(now).unwrap();
            next(&mut pipeline).unwrap();
            let progress = next(&mut pipeline).unwrap();
            if matches {
                assert!(matches!(progress, Progress::Waiting));
                assert!(matches!(
                    next(&mut pipeline).unwrap(),
                    Progress::Committed(SelectionOutcome::Converted { .. })
                ));
                next(&mut pipeline).unwrap();
                assert_eq!(state.borrow().pasted.as_deref(), Some("ghbdsn "));
            } else {
                assert!(matches!(
                    progress,
                    Progress::Finished(Some(SelectionOutcome::TextMismatch))
                ));
                assert!(state.borrow().pasted.is_none());
            }
            assert_eq!(state.borrow().clipboard, "original clipboard");
        }
    }

    #[test]
    fn automatic_correction_requires_the_observed_source() {
        let (mut task, state, now) = task(
            Request::Automatic {
                expected: "different".to_owned(),
                replacement: "інше".to_owned(),
            },
            true,
        );
        assert!(task.automatic());
        task.poll(now).unwrap();
        next(&mut task).unwrap();
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::TextMismatch))
        ));
        assert!(state.borrow().pasted.is_none());
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn unsupported_terminals_do_not_receive_selection_or_copy_commands() {
        let (mut task, state, now) = task(
            Request::Automatic {
                expected: "ghbdsn".to_owned(),
                replacement: "привіт".to_owned(),
            },
            true,
        );
        state.borrow_mut().automatic_supported = false;
        assert!(matches!(
            task.poll(now).unwrap(),
            Progress::Finished(Some(SelectionOutcome::NoSelection))
        ));
        assert_eq!(state.borrow().selections, 0);
        assert_eq!(state.borrow().copies, 0);
    }

    #[test]
    fn a_paste_failure_restores_owned_data_without_retrying_paste() {
        let (mut task, state, now) = task(Request::PreviousWord, true);
        ready_to_paste(&mut task, now);
        state.borrow_mut().fail_paste = true;
        assert!(next(&mut task).is_err());
        drop(task);
        assert!(state.borrow().pasted.is_none());
        assert_eq!(state.borrow().clipboard, "original clipboard");
        assert_eq!(state.borrow().recoveries, 1);
    }

    #[test]
    fn dropping_a_queued_task_never_touches_the_clipboard() {
        let (task, state, _) = task(Request::Selection, true);
        drop(task);
        assert_eq!(state.borrow().copies, 0);
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn enabled_terminal_replacement_never_selects_copies_or_touches_the_clipboard() {
        let (mut task, state, now) = task(
            Request::Automatic {
                expected: "ghbdsn ".to_owned(),
                replacement: "привіт ".to_owned(),
            },
            true,
        );
        state.borrow_mut().terminal = true;
        assert!(matches!(
            task.poll(now).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Converted { characters: 7, .. }))
        ));
        drop(task);
        assert_eq!(state.borrow().pasted.as_deref(), Some("привіт "));
        assert_eq!(state.borrow().selections, 0);
        assert_eq!(state.borrow().copies, 0);
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn no_recovery_arrow_is_sent_while_globe_is_held() {
        let (mut task, state, now) = task(Request::PreviousWord, true);
        ready_to_paste(&mut task, now);
        state.borrow_mut().valid = false;
        state.borrow_mut().fn_held = true;
        assert!(matches!(next(&mut task).unwrap(), Progress::Waiting));
        assert_eq!(state.borrow().recoveries, 0);
        assert!(state.borrow().pasted.is_none());
        state.borrow_mut().fn_held = false;
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Cancelled))
        ));
        assert_eq!(state.borrow().recoveries, 1);
    }

    #[test]
    fn cleanup_failure_never_recovers_a_committed_selection() {
        let (mut task, state, now) = task(Request::PreviousWord, true);
        ready_to_paste(&mut task, now);
        assert!(matches!(next(&mut task).unwrap(), Progress::Committed(_)));
        state.borrow_mut().fail_restore = true;
        assert!(next(&mut task).is_err());
        drop(task);
        assert_eq!(state.borrow().pasted.as_deref(), Some("привіт"));
        assert_eq!(state.borrow().recoveries, 0);
    }

    #[test]
    fn cleanup_failure_does_not_repeat_mismatch_recovery() {
        let (mut task, state, now) = task(
            Request::Automatic {
                expected: "different".to_owned(),
                replacement: "інше".to_owned(),
            },
            true,
        );
        task.poll(now).unwrap();
        next(&mut task).unwrap();
        state.borrow_mut().fail_restore = true;
        assert!(next(&mut task).is_err());
        drop(task);
        assert!(state.borrow().pasted.is_none());
        assert_eq!(state.borrow().recoveries, 1);
    }
}
