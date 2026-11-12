//! Conversion phases yield deadlines instead of sleeping in the application
//! event loop. All native input, clipboard and layout calls stay on its thread.

use super::SelectionOutcome;
use crate::{config::Config, layout::Conversion};
use anyhow::Result;
use std::time::{Duration, Instant};

const SELECTION_SETTLE_MS: u64 = 30;
const SELECTION_PROBE_MS: u64 = 15;
// A longer configured delay may be a deliberate app-compatibility workaround.
const MAX_READY_PASTE_DELAY_MS: u64 = 40;

#[derive(Debug)]
pub(crate) enum Request {
    Selection,
    /// Transliterate or re-case the selection; never switches layouts.
    Transform(upyr_core::TextTransform),
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
    /// Upyr tracked the text itself, so it knows what the selection must hold.
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

/// Right arrows that collapse an abandoned character selection back to the
/// caret. `missing` means the copy produced no text in time.
/// `late_copy` means the copied text arrived after that deadline, which proves
/// the selection exists: one arrow collapses it, while `count` arrows would
/// carry the caret past the word.
fn recovery_arrows(selection: Selection, missing: bool, late_copy: bool) -> usize {
    match (selection, missing && !late_copy) {
        (Selection::Existing, _) => 0,
        (Selection::Characters(count), true) => count,
        _ => 1,
    }
}

pub(crate) trait Effects {
    fn catch_up(&self, _request: &Request) -> Option<Request> {
        None
    }
    fn catch_up_selected(&self, _request: &Request, _selected: &str) -> Option<Request> {
        None
    }
    fn valid(&self) -> bool;
    fn defer_cancellation(&self) -> bool {
        false
    }
    fn typing_overtook(&self) -> bool {
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
    /// Queued typing is held behind this selection, so it cannot replace the
    /// selected text while an early readiness probe is in flight.
    fn selection_input_held(&self) -> bool {
        false
    }
    /// The selected text as the app reports it through Accessibility, read
    /// without a copy. `None` when the app does not expose it.
    /// `early` requires a short IPC timeout rather than waiting for a busy app.
    fn selected_text(&mut self, _early: bool) -> Option<String> {
        None
    }
    fn replace_selected(
        &mut self,
        _conversion: &Conversion,
        _characters: usize,
    ) -> Result<Option<SelectionOutcome>> {
        Ok(None)
    }
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
    Copy {
        fallback_at: Instant,
    },
    Read,
    Paste {
        conversion: Conversion,
        characters: usize,
        /// The replaced text, for a manual conversion only.
        manual_source: Option<String>,
    },
    Restore,
    Done,
}

enum SelectionRead {
    Accessibility,
    Clipboard,
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
    /// The text a manual conversion replaced, once it has been pasted.
    manual_source: Option<String>,
    retry_tracking: bool,
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
            manual_source: None,
            retry_tracking: false,
        }
    }

    /// What the user converted by hand (the selection or the previous word),
    /// once the replacement has been pasted. Automatic edits, undo and text
    /// transforms are not the user's own layout fixes.
    pub fn manual_source(&self) -> Option<&str> {
        self.manual_source.as_deref()
    }

    pub fn automatic(&self) -> bool {
        self.request.automatic()
    }
    pub fn retry_tracking(&self) -> bool {
        self.retry_tracking
    }
    pub fn automatic_edit(&self) -> Option<(&str, &str)> {
        match &self.request {
            Request::Automatic {
                expected,
                replacement,
            } => Some((expected, replacement)),
            _ => None,
        }
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

    /// Whether `selected` holds the text a tracked request expects. The other
    /// requests convert whatever is selected.
    fn expected(&self, selected: &str) -> bool {
        match &self.request {
            Request::Automatic { expected, .. } | Request::Undo { expected, .. } => {
                selected == expected || selected.trim() == expected.trim()
            }
            _ => true,
        }
    }

    fn expected_exact(&self, selected: &str) -> bool {
        match &self.request {
            Request::Automatic { expected, .. } | Request::Undo { expected, .. } => {
                selected == expected
            }
            _ => false,
        }
    }

    fn extend_overtaken_selection(
        &mut self,
        now: Instant,
        selected: &str,
    ) -> Result<Option<Progress>> {
        let Some(request) = self.effects.catch_up_selected(&self.request, selected) else {
            return Ok(None);
        };
        let Request::Automatic { expected, .. } = &request else {
            return Ok(None);
        };
        let total = expected.chars().count();
        let Some(additional) = total
            .checked_sub(selected.chars().count())
            .filter(|n| *n > 0)
        else {
            return Ok(None);
        };
        self.effects.select(Selection::Characters(additional))?;
        self.request = request;
        self.selection = Selection::Characters(total);
        tracing::debug!(additional, "extended selection after delayed editor input");
        Ok(Some(self.wait(
            now,
            SELECTION_PROBE_MS,
            Phase::Copy {
                fallback_at: now + Duration::from_millis(SELECTION_SETTLE_MS),
            },
        )))
    }

    /// Replace a verified selection directly when supported, otherwise paste.
    fn selected(
        &mut self,
        now: Instant,
        selected: String,
        read_from: SelectionRead,
    ) -> Result<Progress> {
        let conversion = self.effects.convert(&selected, &self.request);
        if !conversion.changed {
            // A selection left behind would be replaced by the next key.
            self.effects.recover_selection(self.selection, false)?;
            return self.finish(SelectionOutcome::NoConvertibleText);
        }
        let characters = selected.chars().count();
        let manual_source = matches!(self.request, Request::Selection | Request::PreviousWord)
            .then(|| selected.clone());
        if self.expected_exact(&selected)
            && self.config.paste_delay_ms <= MAX_READY_PASTE_DELAY_MS
            && self.effects.selection_input_held()
        {
            if let Some(outcome) = self.effects.replace_selected(&conversion, characters)? {
                return self.finish(outcome);
            }
        }
        self.effects.write(&conversion.text)?;
        // AX confirms the whole selection was applied, held input keeps it
        // intact, and readback verifies publication of our clipboard value.
        // Yield to Paste even when ready: focus/input/ownership are checked
        // again there before the held stream is released.
        let ready = matches!(read_from, SelectionRead::Accessibility)
            && self.expected_exact(&selected)
            && self.config.paste_delay_ms <= MAX_READY_PASTE_DELAY_MS
            && self.effects.selection_input_held()
            && self.effects.owns_clipboard();
        let paste_delay_ms = if ready { 0 } else { self.config.paste_delay_ms };
        tracing::debug!(ready, paste_delay_ms, "scheduled paste after selection");
        Ok(self.wait(
            now,
            paste_delay_ms,
            Phase::Paste {
                conversion,
                characters,
                manual_source,
            },
        ))
    }

    fn advance(&mut self, now: Instant) -> Result<Progress> {
        if !self.committed() && !self.effects.valid() {
            if self.effects.defer_cancellation() {
                self.deadline = now + Duration::from_millis(10);
                return Ok(Progress::Waiting);
            }
            self.retry_tracking = matches!(self.phase, Phase::Initial)
                && self.request.automatic()
                && self.effects.typing_overtook();
            self.cancel()?;
            return Ok(Progress::Finished(Some(SelectionOutcome::Cancelled)));
        }
        if now < self.deadline {
            return Ok(Progress::Waiting);
        }
        match std::mem::replace(&mut self.phase, Phase::Working) {
            Phase::Initial => {
                if let Some(request) = self.effects.catch_up(&self.request) {
                    self.request = request;
                }
                self.selection = match &self.request {
                    Request::Selection | Request::Transform(_) => Selection::Existing,
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
                            self.retry_tracking = matches!(outcome, SelectionOutcome::Cancelled)
                                && self.request.automatic()
                                && self.effects.typing_overtook();
                            return self.finish(outcome);
                        }
                        Selection::Characters(expected.chars().count())
                    }
                };
                self.effects.select(self.selection)?;
                if !matches!(self.selection, Selection::Existing) {
                    let probe_early = self.request.tracked() && self.effects.selection_input_held();
                    return Ok(self.wait(
                        now,
                        if probe_early {
                            SELECTION_PROBE_MS
                        } else {
                            SELECTION_SETTLE_MS
                        },
                        Phase::Copy {
                            fallback_at: now + Duration::from_millis(SELECTION_SETTLE_MS),
                        },
                    ));
                }
                self.effects.copy()?;
                Ok(self.wait(now, self.config.copy_delay_ms, Phase::Read))
            }
            Phase::Copy { fallback_at } => {
                // A copy puts the text on the pasteboard unmarked, where
                // clipboard-history apps can record it. Text Upyr tracked can
                // be checked without one when the app exposes its selection.
                if self.request.tracked() {
                    let early = now < fallback_at;
                    if let Some(selected) = self.effects.selected_text(early) {
                        let matches = if early {
                            // A partially applied selection may match only
                            // after trimming. Early readiness needs every
                            // character, including boundary whitespace.
                            self.expected_exact(&selected)
                        } else {
                            self.expected(&selected)
                        };
                        if matches {
                            return self.selected(now, selected, SelectionRead::Accessibility);
                        }
                        if !early {
                            if let Some(progress) =
                                self.extend_overtaken_selection(now, &selected)?
                            {
                                return Ok(progress);
                            }
                        }
                    }
                }
                if now < fallback_at {
                    // At most one early probe. Unsupported, busy or not-yet-
                    // selected editors keep the original 30 ms fallback.
                    self.deadline = fallback_at;
                    self.phase = Phase::Copy { fallback_at };
                    return Ok(Progress::Waiting);
                }
                self.effects.copy()?;
                Ok(self.wait(now, self.config.copy_delay_ms, Phase::Read))
            }
            Phase::Read => {
                let Some(selected) = self.effects.read_copy() else {
                    self.effects.recover_selection(self.selection, true)?;
                    return self.finish(SelectionOutcome::NoSelection);
                };
                if !self.expected(&selected) {
                    if let Some(progress) = self.extend_overtaken_selection(now, &selected)? {
                        return Ok(progress);
                    }
                    self.effects.recover_selection(self.selection, false)?;
                    return self.finish(SelectionOutcome::TextMismatch);
                }
                self.selected(now, selected, SelectionRead::Clipboard)
            }
            Phase::Paste {
                conversion,
                characters,
                manual_source,
            } => {
                if !self.effects.owns_clipboard() {
                    self.effects.recover_selection(self.selection, false)?;
                    return self.finish(SelectionOutcome::ClipboardChanged);
                }
                let outcome = self.effects.paste(&conversion, characters)?;
                self.manual_source = manual_source;
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
    automatic: bool,
    terminal_anchor: Option<u64>,
    config: Config,
    restore_source: Option<String>,
    keyboard: super::keyboard::Lease<enigo::Enigo>,
    clipboard: arboard::Clipboard,
    guard: Option<super::input_guard::InputGuard>,
    /// The target's keyboard input waits behind this edit while held.
    hold: Option<GuiHold>,
    /// Set once a release (paste, refusal fallback or recovery) has handled
    /// the selection, so no second recovery can move the caret again.
    selection_released: bool,
    saved: Option<super::SavedClipboard>,
    probe: Option<super::CopyProbe>,
    ownership: Option<crate::clipboard_guard::ClipboardOwnership>,
}

#[cfg(target_os = "macos")]
struct GuiHold {
    pid: i32,
    window: i64,
}

/// Holds the target's keys from before the selection until the paste, so
/// typing that continues through an automatic correction follows it instead of
/// replacing the selected word.
#[cfg(target_os = "macos")]
fn start_hold(guard: &super::input_guard::InputGuard, config: &Config) -> Option<GuiHold> {
    let window = guard.target_window()?;
    let pid = guard.target_pid();
    // Selection, copy and paste with a wide margin. Only a stalled event loop
    // outlives it, and the hold then breaks instead of pasting.
    let budget = SELECTION_SETTLE_MS
        .saturating_add(config.copy_delay_ms)
        .saturating_add(config.paste_delay_ms)
        .saturating_add(750);
    match super::terminal_typing::hold_target(
        pid,
        guard.hold_signal(),
        Duration::from_millis(budget),
    ) {
        Ok(true) => Some(GuiHold { pid, window }),
        Ok(false) => {
            tracing::debug!("input hold was not established; typing will cancel the edit");
            None
        }
        Err(error) => {
            tracing::debug!(%error, "could not hold input during the edit");
            None
        }
    }
}

#[cfg(target_os = "macos")]
impl Pipeline<Desktop> {
    /// `held` is input the target process's tap already holds behind this
    /// edit (a held Enter): the edit adopts that hold instead of starting one,
    /// so Enter follows the paste. A hold that cannot be adopted is released.
    pub fn desktop_with_hold(
        config: Config,
        request: Request,
        held: Option<(i32, std::sync::Arc<super::terminal_typing::HoldSignal>)>,
    ) -> Result<Self> {
        use anyhow::Context;
        let started = Instant::now();
        let terminal = request.tracked() && crate::input_context::terminal_typing_enabled(&config);
        // Establish the hold before cross-process AX and guard setup. Those
        // calls can be cold: extra characters must not reach the selection
        // while they run and make the recognized prefix stale.
        let held = held.or_else(|| {
            if terminal || !request.tracked() {
                return None;
            }
            let pid = super::input_guard::frontmost_pid()?;
            let signal = std::sync::Arc::default();
            let duration = Duration::from_millis(
                SELECTION_SETTLE_MS
                    .saturating_add(config.copy_delay_ms)
                    .saturating_add(config.paste_delay_ms)
                    .saturating_add(750),
            );
            super::terminal_typing::hold_target(pid, std::sync::Arc::clone(&signal), duration)
                .ok()
                .filter(|held| *held)
                .map(|_| (pid, signal))
        });
        let restore_source = match &request {
            Request::Undo { source_id, .. } => Some(source_id.clone()),
            _ => None,
        };
        let guard = match &held {
            Some((_, signal)) => {
                super::input_guard::InputGuard::with_hold(terminal, std::sync::Arc::clone(signal))
            }
            None => super::input_guard::InputGuard::new(terminal),
        };
        let guard = match guard {
            Ok(guard) => guard,
            Err(error) => {
                if let Some((pid, _)) = held {
                    let _ = super::terminal_typing::release_held(pid);
                }
                return Err(error);
            }
        };
        tracing::debug!(
            elapsed_us = started.elapsed().as_micros(),
            "prepared conversion activity guard"
        );
        let hold = match held {
            Some((pid, _)) => {
                let adopted = (!terminal && guard.target_pid() == pid)
                    .then(|| guard.target_window())
                    .flatten()
                    .map(|window| GuiHold { pid, window });
                if adopted.is_none() {
                    // Focus moved or the target is a terminal: let Enter go.
                    let _ = super::terminal_typing::release_held(pid);
                }
                adopted
            }
            None => (!terminal && request.tracked())
                .then(|| start_hold(&guard, &config))
                .flatten(),
        };
        tracing::debug!(
            elapsed_us = started.elapsed().as_micros(),
            "prepared conversion input hold"
        );
        let devices = super::keyboard::lease().and_then(|keyboard| {
            arboard::Clipboard::new()
                .context("could not access the system clipboard")
                .map(|clipboard| (keyboard, clipboard))
        });
        let (keyboard, clipboard) = match devices {
            Ok(devices) => devices,
            Err(error) => {
                // Held keys must not wait for the deadline behind an edit
                // that will never start.
                if let Some(hold) = &hold {
                    let _ = super::terminal_typing::release_held(hold.pid);
                }
                return Err(error);
            }
        };
        let effects = Desktop {
            automatic: request.automatic(),
            terminal_anchor: super::TYPING_TIMESTAMP.get(),
            restore_source,
            guard: Some(guard),
            keyboard,
            clipboard,
            hold,
            selection_released: false,
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
    fn catch_up(&self, request: &Request) -> Option<Request> {
        self.hold.as_ref()?;
        let Request::Automatic {
            expected,
            replacement,
        } = request
        else {
            return None;
        };
        if !expected.chars().all(char::is_alphabetic) {
            return None;
        }
        let text = crate::caret::text_before_caret()?;
        let correction =
            crate::auto_correct::catch_up_prefix(expected, replacement, &text, &self.config)?;
        tracing::debug!(
            additional = correction.expected_source.chars().count() - expected.chars().count(),
            "caught up with delivered prefix"
        );
        Some(Request::Automatic {
            expected: correction.expected_source,
            replacement: correction.replacement,
        })
    }
    fn catch_up_selected(&self, request: &Request, selected: &str) -> Option<Request> {
        self.hold.as_ref()?;
        let Request::Automatic {
            expected,
            replacement,
        } = request
        else {
            return None;
        };
        let text = crate::caret::text_through_selection(selected)?;
        let correction = crate::auto_correct::catch_up_selected_prefix(
            expected,
            replacement,
            selected,
            &text,
            &self.config,
        )?;
        Some(Request::Automatic {
            expected: correction.expected_source,
            replacement: correction.replacement,
        })
    }
    fn valid(&self) -> bool {
        self.guard.as_ref().is_some_and(|guard| guard.valid())
    }
    fn defer_cancellation(&self) -> bool {
        self.guard
            .as_ref()
            .is_some_and(|guard| guard.defer_cancellation())
    }
    fn typing_overtook(&self) -> bool {
        self.guard
            .as_ref()
            .is_some_and(|guard| guard.typing_overtook())
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
        if self.automatic && self.terminal_anchor.is_none() {
            return Ok(Some(SelectionOutcome::Cancelled));
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
        if !prepared.post(
            target_pid,
            target_window,
            stamp,
            self.automatic.then_some(self.terminal_anchor).flatten(),
            || {
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
            },
        )? {
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
            Selection::Word => super::select_previous_word(&mut self.keyboard.get()),
            Selection::Characters(count) => {
                if let Some(hold) = &self.hold {
                    return super::terminal_typing::Prepared::select_characters(count, hold.pid);
                }
                super::select_previous_characters(&mut self.keyboard.get(), count)
            }
        }
    }
    fn selection_input_held(&self) -> bool {
        self.hold.is_some()
    }
    fn selected_text(&mut self, early: bool) -> Option<String> {
        let selected = if early {
            crate::caret::selected_text_if_ready()
        } else {
            crate::caret::selected_text()
        };
        tracing::debug!(
            early,
            readable = selected.is_some(),
            "selection read through Accessibility"
        );
        selected
    }
    fn copy(&mut self) -> Result<()> {
        self.saved = Some(super::snapshot(&mut self.clipboard));
        self.probe = Some(super::arm_copy_probe(&mut self.clipboard)?);
        super::send_shortcut(&mut self.keyboard.get(), super::Shortcut::Copy)
    }
    fn replace_selected(
        &mut self,
        conversion: &Conversion,
        characters: usize,
    ) -> Result<Option<SelectionOutcome>> {
        let prepared = super::terminal_typing::Prepared::selection(&conversion.text)?;
        let Some(hold) = self.hold.take() else {
            return Ok(None);
        };
        let mut layout_switched = None;
        let mut layout_error = None;
        let released = prepared.release(hold.pid, hold.window, || {
            if !self
                .guard
                .as_ref()
                .is_some_and(|guard| guard.has_target_focus())
            {
                return false;
            }
            match self.switch_layout(conversion.direction) {
                Ok(layout) => {
                    layout_switched = layout;
                    true
                }
                Err(error) => {
                    layout_error = Some(error);
                    false
                }
            }
        });
        self.selection_released = released.is_ok();
        if !released? {
            return match layout_error {
                Some(error) => Err(error),
                None => Ok(Some(SelectionOutcome::Cancelled)),
            };
        }
        tracing::debug!("replaced verified selection with Unicode events");
        Ok(Some(SelectionOutcome::Converted {
            direction: conversion.direction,
            characters,
            layout_switched,
        }))
    }
    fn read_copy(&mut self) -> Option<String> {
        let selected = super::read_copied_text(&mut self.clipboard, self.probe.as_ref()?)?;
        self.ownership = Some(crate::clipboard_guard::ClipboardOwnership::after_write(
            &selected,
        ));
        Some(selected)
    }
    fn convert(&self, selected: &str, request: &Request) -> Conversion {
        if let Request::Transform(transform) = request {
            return super::transformed(selected, *transform);
        }
        let replacement = match request {
            Request::Automatic { replacement, .. } | Request::Undo { replacement, .. } => {
                Some(replacement.as_str())
            }
            _ => None,
        };
        super::selection_conversion(selected, self.config.direction, replacement)
    }
    fn write(&mut self, replacement: &str) -> Result<()> {
        if self.saved.is_none() {
            // No copy saved the user's clipboard first.
            self.saved = Some(super::snapshot(&mut self.clipboard));
        }
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
        if self.hold.is_some() {
            let prepared = super::terminal_typing::Prepared::paste()?;
            if let Some(hold) = self.hold.take() {
                let mut layout_switched = None;
                let mut layout_error = None;
                // The target's stream waits at the trigger while TIS changes,
                // so no pending key can replay into the new layout early.
                let released = prepared.release(hold.pid, hold.window, || {
                    match self.switch_layout(conversion.direction) {
                        Ok(layout) => {
                            layout_switched = layout;
                            true
                        }
                        Err(error) => {
                            layout_error = Some(error);
                            false
                        }
                    }
                });
                self.selection_released = released.is_ok();
                if !released? {
                    // Refused: the fallback released the selection unless a
                    // pointer or focus change had already moved the caret.
                    return match layout_error {
                        Some(error) => Err(error),
                        None => Ok(SelectionOutcome::Cancelled),
                    };
                }
                return Ok(SelectionOutcome::Converted {
                    direction: conversion.direction,
                    characters,
                    layout_switched,
                });
            }
        }
        // TIS selection can replay a pending key event on macOS. Select the
        // destination before posting Cmd+V, never while that event is queued.
        let layout_switched = self.switch_layout(conversion.direction)?;
        super::send_shortcut(&mut self.keyboard.get(), super::Shortcut::Paste)?;
        Ok(SelectionOutcome::Converted {
            direction: conversion.direction,
            characters,
            layout_switched,
        })
    }
    fn recover_selection(&mut self, selection: Selection, missing: bool) -> Result<()> {
        use enigo::Keyboard;
        if self.selection_released {
            return Ok(());
        }
        let recoverable = self
            .guard
            .as_ref()
            .is_some_and(|guard| guard.can_recover_selection());
        // A copy that lands after its deadline still proves the selection.
        let late_copy = recoverable
            && self.ownership.is_none()
            && self.probe.is_some()
            && self.read_copy().is_some();
        let arrows = if recoverable {
            recovery_arrows(selection, missing, late_copy)
        } else {
            0
        };
        if self.hold.is_some() {
            let prepared = super::terminal_typing::Prepared::right_arrows(arrows)?;
            if let Some(hold) = self.hold.take() {
                // The arrows go in ahead of every held key, which then follows
                // at the restored caret.
                let released = prepared.release(hold.pid, hold.window, || true);
                self.selection_released = released.is_ok();
                released?;
                return Ok(());
            }
        }
        if !recoverable {
            return Ok(());
        }
        match selection {
            Selection::Word if missing && !late_copy => {
                super::undo_previous_word_selection(&mut self.keyboard.get())
            }
            _ => {
                let mut keyboard = self.keyboard.get();
                for _ in 0..arrows {
                    keyboard.key(enigo::Key::RightArrow, enigo::Direction::Click)?;
                }
                Ok(())
            }
        }
    }
    fn stop_guard(&mut self) {
        // Every path ends the hold, so held keys are never stranded.
        if let Some(hold) = self.hold.take() {
            self.selection_released = true;
            let released = super::terminal_typing::Prepared::right_arrows(0)
                .and_then(|prepared| prepared.release(hold.pid, hold.window, || true));
            if let Err(error) = released {
                tracing::debug!(%error, "could not release held input");
            }
        }
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
        /// What Accessibility reports as selected, if the app exposes it.
        accessible: Option<String>,
        selection_held: bool,
        accessibility_reads: Vec<bool>,
        clipboard_readable: bool,
        unicode_selection: bool,
        typing_overtook: bool,
        selected_context: Option<String>,
        selected_counts: Vec<usize>,
    }

    struct Fake(Rc<RefCell<State>>);
    impl Effects for Fake {
        fn valid(&self) -> bool {
            self.0.borrow().valid
        }
        fn defer_cancellation(&self) -> bool {
            self.0.borrow().fn_held
        }
        fn typing_overtook(&self) -> bool {
            self.0.borrow().typing_overtook
        }
        fn catch_up_selected(&self, request: &Request, selected: &str) -> Option<Request> {
            let state = self.0.borrow();
            let Request::Automatic {
                expected,
                replacement,
            } = request
            else {
                return None;
            };
            let config = Config {
                direction: Direction::EnglishToUkrainian,
                ..Config::default()
            };
            let correction = crate::auto_correct::catch_up_selected_prefix(
                expected,
                replacement,
                selected,
                state.selected_context.as_deref()?,
                &config,
            )?;
            Some(Request::Automatic {
                expected: correction.expected_source,
                replacement: correction.replacement,
            })
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
                self.0.borrow_mut().selected_counts.push(count);
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
        fn selection_input_held(&self) -> bool {
            self.0.borrow().selection_held
        }
        fn selected_text(&mut self, early: bool) -> Option<String> {
            let mut state = self.0.borrow_mut();
            state.accessibility_reads.push(early);
            state.accessible.clone()
        }
        fn replace_selected(
            &mut self,
            conversion: &Conversion,
            characters: usize,
        ) -> Result<Option<SelectionOutcome>> {
            let mut state = self.0.borrow_mut();
            if !state.unicode_selection {
                return Ok(None);
            }
            state.pasted = Some(conversion.text.clone());
            Ok(Some(SelectionOutcome::Converted {
                direction: conversion.direction,
                characters,
                layout_switched: None,
            }))
        }
        fn read_copy(&mut self) -> Option<String> {
            let mut state = self.0.borrow_mut();
            let selected = state.copied.clone()?;
            state.owned = Some(state.clipboard.clone());
            Some(selected)
        }
        fn convert(&self, selected: &str, request: &Request) -> Conversion {
            if let Request::Transform(transform) = request {
                return super::super::transformed(selected, *transform);
            }
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
            if state.saved.is_none() {
                state.saved = Some(state.clipboard.clone());
            }
            state.clipboard = replacement.to_owned();
            state.owned = Some(replacement.to_owned());
            Ok(())
        }
        fn owns_clipboard(&mut self) -> bool {
            let state = self.0.borrow();
            state.clipboard_readable && state.owned.as_ref() == Some(&state.clipboard)
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
            accessible: None,
            selection_held: false,
            accessibility_reads: Vec::new(),
            clipboard_readable: true,
            unicode_selection: false,
            typing_overtook: false,
            selected_context: None,
            selected_counts: Vec::new(),
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
    fn a_late_fifth_letter_extends_the_held_selection_before_replacement() {
        let (mut task, state, now) = task(
            Request::Automatic {
                expected: "dbgh".into(),
                replacement: "випр".into(),
            },
            true,
        );
        {
            let mut state = state.borrow_mut();
            state.selection_held = true;
            state.unicode_selection = true;
            state.accessible = Some("bghf".into());
            state.selected_context = Some("earlier dbghf".into());
        }
        assert!(matches!(task.poll(now).unwrap(), Progress::Waiting));
        assert!(matches!(next(&mut task).unwrap(), Progress::Waiting));
        assert_eq!(state.borrow().selected_counts, [4]);
        assert!(matches!(next(&mut task).unwrap(), Progress::Waiting));
        assert_eq!(state.borrow().selected_counts, [4, 1]);
        state.borrow_mut().accessible = Some("dbghf".into());
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Converted { .. }))
        ));
        assert_eq!(state.borrow().pasted.as_deref(), Some("випра"));
        assert_eq!(task.automatic_edit(), Some(("dbghf", "випра")));
        assert_eq!(state.borrow().copies, 0);
    }

    #[test]
    fn only_an_unstarted_automatic_edit_keeps_tracking_after_overtaking() {
        for (automatic, started, overtaken, retry) in [
            (true, false, true, true),
            (true, false, false, false),
            (true, true, true, false),
            (false, false, true, false),
        ] {
            let request = if automatic {
                Request::Automatic {
                    expected: "ghbdsn ".into(),
                    replacement: "привіт ".into(),
                }
            } else {
                Request::PreviousWord
            };
            let (mut task, state, now) = task(request, true);
            if started {
                assert!(matches!(task.poll(now).unwrap(), Progress::Waiting));
            }
            state.borrow_mut().valid = false;
            state.borrow_mut().typing_overtook = overtaken;
            assert!(matches!(
                task.poll(now).unwrap(),
                Progress::Finished(Some(SelectionOutcome::Cancelled))
            ));
            assert_eq!(task.retry_tracking(), retry);
            assert!(state.borrow().pasted.is_none());
        }
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
    fn a_text_transform_replaces_the_selection_with_the_transformed_text() {
        let (mut task, state, now) = task(
            Request::Transform(upyr_core::TextTransform::Transliterate),
            false,
        );
        state.borrow_mut().copied = Some("Київ".to_owned());
        ready_to_paste(&mut task, now);
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Converted { .. }))
        ));
        assert_eq!(state.borrow().pasted.as_deref(), Some("Kyiv"));
    }

    #[test]
    fn a_manual_conversion_reports_the_text_it_converted() {
        let (mut manual, state, now) = task(Request::PreviousWord, false);
        state.borrow_mut().copied = Some("jr ".to_owned());
        assert_eq!(manual.manual_source(), None);
        ready_to_paste(&mut manual, now);
        next(&mut manual).unwrap();
        assert_eq!(manual.manual_source(), Some("jr "));

        // Automatic edits and text transforms are not the user's own fixes.
        for request in [
            Request::Automatic {
                expected: "ghbdsn".to_owned(),
                replacement: "привіт".to_owned(),
            },
            Request::Transform(upyr_core::TextTransform::CycleCase),
        ] {
            let (mut task, _, now) = task(request, false);
            ready_to_paste(&mut task, now);
            next(&mut task).unwrap();
            assert_eq!(task.manual_source(), None);
        }
    }

    #[test]
    fn an_unconvertible_selection_is_released_before_finishing() {
        // Leaving the word selected would let the next key delete it.
        let (mut task, state, now) = task(Request::PreviousWord, true);
        state.borrow_mut().copied = Some("123".to_owned());
        task.poll(now).unwrap();
        next(&mut task).unwrap();
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::NoConvertibleText))
        ));
        assert_eq!(state.borrow().recoveries, 1);
        assert!(state.borrow().pasted.is_none());
    }

    #[test]
    fn recovery_collapses_a_late_copied_selection_with_one_arrow() {
        assert_eq!(recovery_arrows(Selection::Existing, true, false), 0);
        assert_eq!(recovery_arrows(Selection::Characters(7), false, false), 1);
        // Nothing was copied: the shift-selection never took effect.
        assert_eq!(recovery_arrows(Selection::Characters(7), true, false), 7);
        // The copy only arrived late: the selection exists, and N arrows
        // would move the caret past the word.
        assert_eq!(recovery_arrows(Selection::Characters(7), true, true), 1);
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

    fn automatic() -> Request {
        Request::Automatic {
            expected: "ghbdsn".to_owned(),
            replacement: "привіт".to_owned(),
        }
    }

    #[test]
    fn an_exact_held_selection_can_proceed_before_the_settle_deadline() {
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().selection_held = true;
        state.borrow_mut().accessible = Some("ghbdsn".into());
        task.poll(now).unwrap();
        assert_eq!(task.deadline(), now + Duration::from_millis(15));
        assert!(matches!(next(&mut task).unwrap(), Progress::Waiting));
        // Exact AX + held input + clipboard readback remove the remaining
        // wait, but posting paste still requires a separate guarded poll.
        assert_eq!(task.deadline(), now + Duration::from_millis(15));
        assert_eq!(state.borrow().accessibility_reads, [true]);
        assert_eq!(state.borrow().selections, 1);
        assert_eq!(state.borrow().copies, 0);
        assert!(state.borrow().pasted.is_none());
        assert!(matches!(next(&mut task).unwrap(), Progress::Committed(_)));
        next(&mut task).unwrap();
        assert_eq!(state.borrow().pasted.as_deref(), Some("привіт"));
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn verified_unicode_selection_finishes_without_clipboard_cleanup_delay() {
        let (mut task, state, now) = task(automatic(), true);
        {
            let mut state = state.borrow_mut();
            state.selection_held = true;
            state.accessible = Some("ghbdsn".into());
            state.unicode_selection = true;
        }
        task.poll(now).unwrap();
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Converted { .. }))
        ));
        let state = state.borrow();
        assert_eq!(state.pasted.as_deref(), Some("привіт"));
        assert_eq!(state.clipboard, "original clipboard");
        assert!(state.saved.is_none());
        assert!(state.owned.is_none());
        assert_eq!(state.copies, 0);
    }

    #[test]
    fn unicode_selection_requires_exact_text_a_live_hold_and_default_paste_timing() {
        for (selected, held, delay) in [
            ("ghbdsn ", true, 40),
            ("ghbdsn", false, 40),
            ("ghbdsn", true, 100),
        ] {
            let (mut task, state, now) = task(automatic(), true);
            task.config.paste_delay_ms = delay;
            {
                let mut state = state.borrow_mut();
                state.selection_held = held;
                state.accessible = Some(selected.into());
                state.unicode_selection = true;
            }
            task.poll(now).unwrap();
            next(&mut task).unwrap();
            assert!(state.borrow().pasted.is_none());
        }
    }

    #[test]
    fn selection_ready_only_at_the_regular_checkpoint_can_also_paste_without_waiting() {
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().selection_held = true;
        task.poll(now).unwrap();
        next(&mut task).unwrap(); // The early AX probe is not ready.
        assert_eq!(task.deadline(), now + Duration::from_millis(30));
        state.borrow_mut().accessible = Some("ghbdsn".into());
        next(&mut task).unwrap();
        assert_eq!(task.deadline(), now + Duration::from_millis(30));
        assert_eq!(state.borrow().accessibility_reads, [true, false]);
        assert_eq!(state.borrow().copies, 0);
        assert!(state.borrow().pasted.is_none());
        assert!(matches!(next(&mut task).unwrap(), Progress::Committed(_)));
    }

    #[test]
    fn an_unconfirmed_clipboard_keeps_the_configured_paste_wait() {
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().selection_held = true;
        state.borrow_mut().accessible = Some("ghbdsn".into());
        state.borrow_mut().clipboard_readable = false;
        task.poll(now).unwrap();
        next(&mut task).unwrap();
        let paste_at = now + Duration::from_millis(15 + task.config.paste_delay_ms);
        assert_eq!(task.deadline(), paste_at);
        assert!(matches!(
            task.poll(paste_at - Duration::from_millis(1)).unwrap(),
            Progress::Waiting
        ));
        assert!(state.borrow().pasted.is_none());
        state.borrow_mut().clipboard_readable = true;
        assert!(matches!(next(&mut task).unwrap(), Progress::Committed(_)));
        next(&mut task).unwrap();
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn longer_user_paste_delays_are_respected_even_when_ready() {
        for delay in [0, 20, 40, 41, 200] {
            let (mut task, state, now) = task(automatic(), true);
            task.config.paste_delay_ms = delay;
            state.borrow_mut().selection_held = true;
            state.borrow_mut().accessible = Some("ghbdsn".into());
            task.poll(now).unwrap();
            next(&mut task).unwrap();
            let retained_delay = if delay > 40 { delay } else { 0 };
            assert_eq!(
                task.deadline(),
                now + Duration::from_millis(15 + retained_delay)
            );
            assert!(state.borrow().pasted.is_none());
        }
    }

    #[test]
    fn a_copied_selection_retains_the_paste_wait_even_when_held_and_exact() {
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().selection_held = true;
        task.poll(now).unwrap();
        next(&mut task).unwrap(); // Early AX probe unavailable.
        next(&mut task).unwrap(); // Normal AX probe unavailable; copy.
        let copied_at = task.deadline();
        next(&mut task).unwrap(); // Read the matching copied text.
        assert_eq!(state.borrow().copies, 1);
        assert_eq!(
            task.deadline(),
            copied_at + Duration::from_millis(task.config.paste_delay_ms)
        );
        assert!(state.borrow().pasted.is_none());
        assert!(matches!(next(&mut task).unwrap(), Progress::Committed(_)));
    }

    #[test]
    fn exact_ax_without_held_typing_retains_the_paste_wait() {
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().accessible = Some("ghbdsn".into());
        task.poll(now).unwrap();
        next(&mut task).unwrap();
        assert_eq!(
            task.deadline(),
            now + Duration::from_millis(30 + task.config.paste_delay_ms)
        );
        assert_eq!(state.borrow().copies, 0);
        assert!(state.borrow().pasted.is_none());
    }

    #[test]
    fn early_missing_partial_or_trim_only_matches_keep_the_original_fallback() {
        for accessible in [None, Some("hbdsn"), Some("ghbdsn ")] {
            let (mut task, state, now) = task(automatic(), true);
            state.borrow_mut().selection_held = true;
            state.borrow_mut().accessible = accessible.map(str::to_owned);
            task.poll(now).unwrap();
            next(&mut task).unwrap();
            assert_eq!(task.deadline(), now + Duration::from_millis(30));
            assert_eq!(state.borrow().clipboard, "original clipboard");
            assert_eq!(state.borrow().copies, 0);
            assert_eq!(state.borrow().accessibility_reads, [true]);
            task.poll(now + Duration::from_millis(29)).unwrap();
            assert_eq!(state.borrow().accessibility_reads, [true]);
            next(&mut task).unwrap();
            assert_eq!(state.borrow().accessibility_reads, [true, false]);
            // The normal fallback still accepts whitespace differences. An
            // unavailable or wrong AX value still uses the configured copy wait.
            let copied = accessible != Some("ghbdsn ");
            assert_eq!(state.borrow().copies, usize::from(copied));
            let delay = if copied {
                task.config.copy_delay_ms
            } else {
                task.config.paste_delay_ms
            };
            assert_eq!(task.deadline(), now + Duration::from_millis(30 + delay));
            assert!(state.borrow().pasted.is_none());
        }
    }

    #[test]
    fn a_late_event_loop_uses_one_normal_probe_instead_of_replaying_early_work() {
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().selection_held = true;
        state.borrow_mut().accessible = Some("ghbdsn".into());
        task.poll(now).unwrap();
        task.poll(now + Duration::from_millis(60)).unwrap();
        assert_eq!(state.borrow().accessibility_reads, [false]);
        assert_eq!(state.borrow().selections, 1);
        assert_eq!(state.borrow().copies, 0);
    }

    #[test]
    fn focus_loss_before_the_early_probe_never_reads_or_writes_the_clipboard() {
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().selection_held = true;
        task.poll(now).unwrap();
        state.borrow_mut().valid = false;
        state.borrow_mut().recoverable = false;
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Finished(Some(SelectionOutcome::Cancelled))
        ));
        assert!(state.borrow().accessibility_reads.is_empty());
        assert_eq!(state.borrow().copies, 0);
        assert_eq!(state.borrow().clipboard, "original clipboard");
        assert!(state.borrow().pasted.is_none());
    }

    #[test]
    fn early_readiness_does_not_bypass_focus_or_clipboard_checks_before_paste() {
        for focus_lost in [true, false] {
            let (mut task, state, now) = task(automatic(), true);
            state.borrow_mut().selection_held = true;
            state.borrow_mut().accessible = Some("ghbdsn".into());
            task.poll(now).unwrap();
            next(&mut task).unwrap();
            if focus_lost {
                state.borrow_mut().valid = false;
                state.borrow_mut().recoverable = false;
            } else {
                state.borrow_mut().clipboard = "external copy".into();
            }
            let outcome = next(&mut task).unwrap();
            assert!(matches!(
                outcome,
                Progress::Finished(Some(
                    SelectionOutcome::Cancelled | SelectionOutcome::ClipboardChanged
                ))
            ));
            assert!(state.borrow().pasted.is_none());
            assert_eq!(
                state.borrow().clipboard,
                if focus_lost {
                    "original clipboard"
                } else {
                    "external copy"
                }
            );
        }
    }

    #[test]
    fn undo_can_use_exact_readiness_but_manual_selection_keeps_its_settle_wait() {
        let (mut undo, state, now) = task(
            Request::Undo {
                expected: "привіт ".into(),
                replacement: "ghbdsn ".into(),
                #[cfg(target_os = "macos")]
                source_id: "ABC".into(),
            },
            true,
        );
        state.borrow_mut().selection_held = true;
        state.borrow_mut().accessible = Some("привіт ".into());
        undo.poll(now).unwrap();
        next(&mut undo).unwrap();
        assert_eq!(state.borrow().accessibility_reads, [true]);
        assert_eq!(state.borrow().copies, 0);
        assert!(matches!(next(&mut undo).unwrap(), Progress::Committed(_)));
        assert_eq!(state.borrow().pasted.as_deref(), Some("ghbdsn "));

        let (mut manual, state, now) = task(Request::PreviousWord, true);
        state.borrow_mut().selection_held = true;
        manual.poll(now).unwrap();
        assert_eq!(manual.deadline(), now + Duration::from_millis(30));
        next(&mut manual).unwrap();
        assert!(state.borrow().accessibility_reads.is_empty());
        assert_eq!(state.borrow().copies, 1);
    }

    #[test]
    fn a_correction_read_through_accessibility_never_copies() {
        // A copy puts the typed word on the pasteboard unmarked, where
        // clipboard-history apps can record it.
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().accessible = Some("ghbdsn".to_owned());
        state.borrow_mut().copied = None;
        assert!(matches!(task.poll(now).unwrap(), Progress::Waiting));
        assert_eq!(task.deadline(), now + Duration::from_millis(30));
        assert!(matches!(next(&mut task).unwrap(), Progress::Waiting));
        assert!(matches!(
            next(&mut task).unwrap(),
            Progress::Committed(SelectionOutcome::Converted { characters: 6, .. })
        ));
        next(&mut task).unwrap();
        assert_eq!(state.borrow().copies, 0);
        assert_eq!(state.borrow().pasted.as_deref(), Some("привіт"));
        assert_eq!(state.borrow().clipboard, "original clipboard");
    }

    #[test]
    fn other_accessible_text_falls_back_to_copying() {
        // The app may not have applied the whole selection yet.
        let (mut task, state, now) = task(automatic(), true);
        state.borrow_mut().accessible = Some("hbdsn".to_owned());
        ready_to_paste(&mut task, now);
        assert!(matches!(next(&mut task).unwrap(), Progress::Committed(_)));
        assert_eq!(state.borrow().copies, 1);
        assert_eq!(state.borrow().pasted.as_deref(), Some("привіт"));
    }

    #[test]
    fn a_manual_conversion_still_copies() {
        // Nothing to check the accessible text against.
        let (mut task, state, now) = task(Request::PreviousWord, true);
        state.borrow_mut().accessible = Some("ghbdsn".to_owned());
        ready_to_paste(&mut task, now);
        assert_eq!(state.borrow().copies, 1);
    }
}
