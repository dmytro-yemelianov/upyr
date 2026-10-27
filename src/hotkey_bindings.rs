//! Replace shortcut registrations without leaving the old configuration partly
//! unregistered when the OS rejects a new shortcut.

use anyhow::{Context, Result};
use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};

pub(crate) fn replace(manager: &GlobalHotKeyManager, old: &[HotKey], new: &[HotKey]) -> Result<()> {
    replace_with(
        old,
        new,
        |hotkey| Ok(manager.register(hotkey)?),
        |hotkey| Ok(manager.unregister(hotkey)?),
    )
}

fn replace_with(
    old: &[HotKey],
    new: &[HotKey],
    mut register: impl FnMut(HotKey) -> Result<()>,
    mut unregister: impl FnMut(HotKey) -> Result<()>,
) -> Result<()> {
    let mut added = Vec::new();
    for hotkey in new.iter().filter(|hotkey| !old.contains(hotkey)) {
        if let Err(error) = register(*hotkey) {
            let rollback = added
                .into_iter()
                .rev()
                .filter_map(|hotkey| unregister(hotkey).err())
                .collect::<Vec<_>>();
            return Err(error).context(failure_context("register new shortcut", rollback));
        }
        added.push(*hotkey);
    }
    let mut removed = Vec::new();
    for hotkey in old.iter().filter(|hotkey| !new.contains(hotkey)) {
        if let Err(error) = unregister(*hotkey) {
            let mut rollback = removed
                .into_iter()
                .filter_map(|hotkey| register(hotkey).err())
                .collect::<Vec<_>>();
            rollback.extend(
                added
                    .into_iter()
                    .rev()
                    .filter_map(|hotkey| unregister(hotkey).err()),
            );
            return Err(error).context(failure_context("unregister old shortcut", rollback));
        }
        removed.push(*hotkey);
    }
    Ok(())
}

fn failure_context(action: &str, rollback: Vec<anyhow::Error>) -> String {
    if rollback.is_empty() {
        format!("failed to {action}; previous shortcut registrations were restored")
    } else {
        let details = rollback
            .into_iter()
            .map(|error| format!("{error:#}"))
            .collect::<Vec<_>>()
            .join("; ");
        format!("failed to {action}; shortcut rollback also failed: {details}")
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn mac_key_code(code: u16) -> Option<global_hotkey::hotkey::Code> {
    use global_hotkey::hotkey::Code;
    Some(match code {
        0 => Code::KeyA,
        1 => Code::KeyS,
        2 => Code::KeyD,
        3 => Code::KeyF,
        4 => Code::KeyH,
        5 => Code::KeyG,
        6 => Code::KeyZ,
        7 => Code::KeyX,
        8 => Code::KeyC,
        9 => Code::KeyV,
        11 => Code::KeyB,
        12 => Code::KeyQ,
        13 => Code::KeyW,
        14 => Code::KeyE,
        15 => Code::KeyR,
        16 => Code::KeyY,
        17 => Code::KeyT,
        18 => Code::Digit1,
        19 => Code::Digit2,
        20 => Code::Digit3,
        21 => Code::Digit4,
        22 => Code::Digit6,
        23 => Code::Digit5,
        24 => Code::Equal,
        25 => Code::Digit9,
        26 => Code::Digit7,
        27 => Code::Minus,
        28 => Code::Digit8,
        29 => Code::Digit0,
        30 => Code::BracketRight,
        31 => Code::KeyO,
        32 => Code::KeyU,
        33 => Code::BracketLeft,
        34 => Code::KeyI,
        35 => Code::KeyP,
        36 => Code::Enter,
        37 => Code::KeyL,
        38 => Code::KeyJ,
        39 => Code::Quote,
        40 => Code::KeyK,
        41 => Code::Semicolon,
        42 => Code::Backslash,
        43 => Code::Comma,
        44 => Code::Slash,
        45 => Code::KeyN,
        46 => Code::KeyM,
        47 => Code::Period,
        48 => Code::Tab,
        49 => Code::Space,
        50 => Code::Backquote,
        51 => Code::Backspace,
        53 => Code::Escape,
        64 => Code::F17,
        79 => Code::F18,
        80 => Code::F19,
        90 => Code::F20,
        96 => Code::F5,
        97 => Code::F6,
        98 => Code::F7,
        99 => Code::F3,
        100 => Code::F8,
        101 => Code::F9,
        103 => Code::F11,
        105 => Code::F13,
        106 => Code::F16,
        107 => Code::F14,
        109 => Code::F10,
        111 => Code::F12,
        113 => Code::F15,
        115 => Code::Home,
        116 => Code::PageUp,
        117 => Code::Delete,
        118 => Code::F4,
        119 => Code::End,
        120 => Code::F2,
        121 => Code::PageDown,
        122 => Code::F1,
        123 => Code::ArrowLeft,
        124 => Code::ArrowRight,
        125 => Code::ArrowDown,
        126 => Code::ArrowUp,
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn matches_mac(hotkey: HotKey, code: u16, flags: usize) -> bool {
    use global_hotkey::hotkey::Modifiers;
    let mut mods = Modifiers::empty();
    for (bit, modifier) in [
        (17, Modifiers::SHIFT),
        (18, Modifiers::CONTROL),
        (19, Modifiers::ALT),
        (20, Modifiers::SUPER),
    ] {
        if flags & (1 << bit) != 0 {
            mods |= modifier;
        }
    }
    mac_key_code(code).is_some_and(|code| hotkey.key == code && hotkey.mods == mods)
}

#[cfg(all(target_os = "macos", test))]
#[test]
fn undo_chord_is_distinct_from_paste_and_navigation() {
    let undo: HotKey = "CmdOrCtrl+Alt+Z".parse().unwrap();
    assert!(matches_mac(undo, 6, (1 << 20) | (1 << 19)));
    assert!(!matches_mac(undo, 9, (1 << 20) | (1 << 19)));
    assert!(!matches_mac(undo, 123, (1 << 20) | (1 << 19)));
    assert!(!matches_mac(undo, 6, 1 << 20));
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::hotkey::{Code, Modifiers};
    use std::cell::RefCell;

    fn keys() -> [HotKey; 4] {
        [Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD]
            .map(|code| HotKey::new(Some(Modifiers::CONTROL), code))
    }

    #[test]
    fn an_occupied_second_shortcut_preserves_both_originals() {
        let [a, b, c, d] = keys();
        let active = RefCell::new(vec![a, b, d]); // d belongs to another app.
        let result = replace_with(
            &[a, b],
            &[c, d],
            |hotkey| {
                anyhow::ensure!(!active.borrow().contains(&hotkey), "shortcut occupied");
                active.borrow_mut().push(hotkey);
                Ok(())
            },
            |hotkey| {
                active.borrow_mut().retain(|item| *item != hotkey);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(*active.borrow(), [a, b, d]);
    }

    #[test]
    fn a_failed_removal_rolls_back_the_entire_replacement() {
        let [a, b, c, d] = keys();
        let active = RefCell::new(vec![a, b]);
        let result = replace_with(
            &[a, b],
            &[c, d],
            |hotkey| {
                active.borrow_mut().push(hotkey);
                Ok(())
            },
            |hotkey| {
                anyhow::ensure!(hotkey != b, "OS rejected removal");
                active.borrow_mut().retain(|item| *item != hotkey);
                Ok(())
            },
        );
        assert!(result.is_err());
        let active = active.borrow();
        assert_eq!(active.len(), 2);
        assert!(active.contains(&a) && active.contains(&b));
    }

    #[test]
    fn swapping_the_two_actions_keeps_the_existing_os_registrations() {
        let [a, b, _, _] = keys();
        replace_with(
            &[a, b],
            &[b, a],
            |_| panic!("already registered"),
            |_| panic!("still in use"),
        )
        .unwrap();
    }
}
