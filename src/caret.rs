//! Where the text caret is on screen, read from the focused element's
//! Accessibility attributes (macOS). Apps that expose no caret give `None`,
//! and callers fall back to the pointer.

/// A rectangle with its origin at the bottom left and y growing up (Cocoa),
/// or at the top left of the main display with y growing down (Quartz and
/// Accessibility), as each use says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Gap between the caret line and what the panel draws.
const GAP: f64 = 6.0;
/// Gap between the caret and what a superscript panel draws.
const SIDE_GAP: f64 = 3.0;

/// Where a panel goes next to the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Just below the caret line, starting at the caret; above the line
    /// when the screen has no room below.
    Below,
    /// Like a superscript: just after the caret, its bottom at the middle of
    /// the caret line, so it covers neither the text before the caret nor
    /// the next line; below the line when the screen has no room above.
    Superscript,
}

/// A panel shown next to the caret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panel {
    pub width: f64,
    pub height: f64,
    /// Transparent space left and right of what the panel draws.
    pub margin_x: f64,
    /// Transparent space above and below what the panel draws.
    pub margin_y: f64,
    pub place: Place,
}

/// Cocoa origin for `panel` next to `caret` (Quartz coordinates), kept on
/// the screen's `visible` frame (Cocoa). `main_height` is the height of the
/// main display, which both coordinate systems share.
pub fn panel_origin(caret: Rect, main_height: f64, panel: Panel, visible: Rect) -> (f64, f64) {
    let caret_top = main_height - caret.y;
    let caret_bottom = caret_top - caret.height;
    let drawn_height = panel.height - 2.0 * panel.margin_y;
    // Origins that put what the panel draws just below or just above the line.
    let below = caret_bottom - GAP - panel.height + panel.margin_y;
    let above = caret_top + GAP - panel.margin_y;
    let (x, y) = match panel.place {
        Place::Below => {
            let fits = below + panel.margin_y >= visible.y;
            (caret.x, if fits { below } else { above })
        }
        Place::Superscript => {
            let bottom = caret_bottom + caret.height / 2.0;
            let fits = bottom + drawn_height <= visible.y + visible.height;
            (
                caret.x + caret.width + SIDE_GAP - panel.margin_x,
                if fits { bottom - panel.margin_y } else { below },
            )
        }
    };
    let right_edge = visible.x + visible.width - panel.width;
    (x.min(right_edge).max(visible.x), y)
}

/// Bounds an editor reports for its caret, sanity-checked: some apps answer
/// with an empty, enormous or off-screen rectangle.
pub fn plausible(caret: Rect) -> bool {
    [caret.x, caret.y, caret.width, caret.height]
        .iter()
        .all(|value| value.is_finite())
        && (0.0..=400.0).contains(&caret.width)
        && caret.height > 0.0
        && caret.height <= 400.0
}

#[cfg(target_os = "macos")]
pub use native::caret;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod native {
    use super::{Rect, plausible};
    use std::ffi::{CStr, c_void};

    type Ref = *const c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct CFRange {
        location: isize,
        length: isize,
    }

    /// `CGRect`: origin then size, four doubles.
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct CGRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    const AX_VALUE_CGRECT: u32 = 3;
    const AX_VALUE_CFRANGE: u32 = 4;
    const UTF8: u32 = 0x0800_0100;
    /// A caret query must never stall the event loop behind a busy app.
    const TIMEOUT_SECONDS: f32 = 0.05;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> Ref;
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: Ref,
            attribute: Ref,
            parameter: Ref,
            value: *mut Ref,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
        fn AXValueCreate(kind: u32, value: *const c_void) -> Ref;
        fn AXValueGetValue(value: Ref, kind: u32, out: *mut c_void) -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: Ref);
        fn CFStringCreateWithCString(allocator: Ref, string: *const i8, encoding: u32) -> Ref;
    }

    /// A Core Foundation object this module owns (a Copy or Create result).
    struct Owned(Ref);

    impl Owned {
        fn new(value: Ref) -> Option<Self> {
            (!value.is_null()).then_some(Self(value))
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    fn name(text: &CStr) -> Option<Owned> {
        Owned::new(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) })
    }

    fn attribute(element: &Owned, attribute: &CStr) -> Option<Owned> {
        let attribute = name(attribute)?;
        let mut value = std::ptr::null();
        if unsafe { AXUIElementCopyAttributeValue(element.0, attribute.0, &mut value) } != 0 {
            return None;
        }
        Owned::new(value)
    }

    fn bounds(element: &Owned, range: CFRange) -> Option<Rect> {
        let attribute = name(c"AXBoundsForRange")?;
        let parameter =
            Owned::new(unsafe { AXValueCreate(AX_VALUE_CFRANGE, (&raw const range).cast()) })?;
        let mut value = std::ptr::null();
        if unsafe {
            AXUIElementCopyParameterizedAttributeValue(
                element.0,
                attribute.0,
                parameter.0,
                &mut value,
            )
        } != 0
        {
            return None;
        }
        let value = Owned::new(value)?;
        let mut rect = CGRect::default();
        (unsafe { AXValueGetValue(value.0, AX_VALUE_CGRECT, (&raw mut rect).cast()) } != 0)
            .then_some(Rect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            })
    }

    /// The caret of the focused text element in Quartz coordinates, or None
    /// when the app does not expose one.
    pub fn caret() -> Option<Rect> {
        let system = Owned::new(unsafe { AXUIElementCreateSystemWide() })?;
        unsafe { AXUIElementSetMessagingTimeout(system.0, TIMEOUT_SECONDS) };
        // Some apps answer only through their application element.
        let element = attribute(&system, c"AXFocusedUIElement").or_else(|| {
            let application = attribute(&system, c"AXFocusedApplication")?;
            unsafe { AXUIElementSetMessagingTimeout(application.0, TIMEOUT_SECONDS) };
            attribute(&application, c"AXFocusedUIElement")
        })?;
        unsafe { AXUIElementSetMessagingTimeout(element.0, TIMEOUT_SECONDS) };
        let selection = attribute(&element, c"AXSelectedTextRange")?;
        let mut range = CFRange::default();
        if unsafe { AXValueGetValue(selection.0, AX_VALUE_CFRANGE, (&raw mut range).cast()) } == 0 {
            return None;
        }
        let at = range.location.saturating_add(range.length).max(0);
        let empty = CFRange {
            location: at,
            length: 0,
        };
        if let Some(rect) = bounds(&element, empty).filter(|rect| plausible(*rect)) {
            return Some(rect);
        }
        // Editors that only measure characters: the caret is at the right
        // edge of the character before it.
        let before = CFRange {
            location: at.checked_sub(1)?,
            length: 1,
        };
        bounds(&element, before)
            .map(|rect| Rect {
                x: rect.x + rect.width,
                width: 0.0,
                ..rect
            })
            .filter(|rect| plausible(*rect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 875.0,
    };

    /// The letters label: drawn edge to edge, below the caret.
    const LABEL: Panel = Panel {
        width: 94.0,
        height: 42.0,
        margin_x: 0.0,
        margin_y: 0.0,
        place: Place::Below,
    };

    /// One small flag, 33 × 21, in a 39 × 50 panel.
    const FLAG: Panel = Panel {
        width: 39.0,
        height: 50.0,
        margin_x: 3.0,
        margin_y: 14.5,
        place: Place::Superscript,
    };

    const CARET: Rect = Rect {
        x: 700.0,
        y: 300.0,
        width: 1.0,
        height: 18.0,
    };

    #[test]
    fn the_panel_sits_just_below_the_caret_line() {
        // A caret 300 points from the top of a 900-point display.
        assert_eq!(
            panel_origin(CARET, 900.0, LABEL, SCREEN),
            (700.0, 900.0 - 318.0 - GAP - 42.0)
        );
    }

    #[test]
    fn the_panel_goes_above_the_caret_near_the_bottom_and_stays_on_screen() {
        let caret = Rect {
            x: 1420.0,
            y: 880.0,
            width: 1.0,
            height: 16.0,
        };
        let (x, y) = panel_origin(caret, 900.0, LABEL, SCREEN);
        assert_eq!(x, 1440.0 - 94.0);
        assert_eq!(y, 900.0 - 880.0 + GAP);
    }

    #[test]
    fn a_superscript_flag_starts_after_the_caret_raised_off_the_line() {
        // The caret line spans 582..600; the flag is drawn from just after
        // the caret, with its bottom at the middle of the line.
        assert_eq!(
            panel_origin(CARET, 900.0, FLAG, SCREEN),
            (701.0 + SIDE_GAP - 3.0, 591.0 - 14.5)
        );
    }

    #[test]
    fn a_superscript_flag_drops_below_the_line_at_the_top_and_stays_on_screen() {
        // The line spans 852..870, and the flag would end above 875.
        let top = Rect { y: 30.0, ..CARET };
        assert_eq!(
            panel_origin(top, 900.0, FLAG, SCREEN).1,
            852.0 - GAP - 50.0 + 14.5
        );
        let right = Rect { x: 1430.0, ..CARET };
        assert_eq!(panel_origin(right, 900.0, FLAG, SCREEN).0, 1440.0 - 39.0);
    }

    #[test]
    fn implausible_caret_bounds_are_rejected() {
        let caret = |x: f64, y: f64, width: f64, height: f64| Rect {
            x,
            y,
            width,
            height,
        };
        assert!(plausible(caret(200.0, 300.0, 1.0, 18.0)));
        assert!(
            plausible(caret(-1200.0, 300.0, 0.0, 18.0)),
            "a display left of the main one"
        );
        assert!(!plausible(caret(0.0, 0.0, 0.0, 0.0)), "empty");
        assert!(
            !plausible(caret(200.0, 300.0, 1.0, 2000.0)),
            "a whole document"
        );
        assert!(!plausible(caret(f64::NAN, 300.0, 1.0, 18.0)));
        assert!(!plausible(caret(200.0, 300.0, -4.0, 18.0)));
    }
}
