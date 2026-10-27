//! Complete country/region collection and per-input-source flag picker.

use super::{NativeController, action_button, label, popup, rect, set_checked};
use crate::{
    config::{Config, IndicatorStyle},
    feedback::{
        globe_flags::{self, Flag},
        globe_indicator,
    },
    system_layout::{self, ActiveInputSource},
};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, rc::Retained, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSButtonType, NSColor, NSFont, NSPopUpButton, NSScrollView,
    NSSearchField, NSTextAlignment, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSLocale, NSPoint, NSSize, NSString};
use objc2_quartz_core::CALayer;

const WIDTH: f64 = 780.0;
const HEIGHT: f64 = 780.0;
const COLUMNS: usize = 4;
const CARD_WIDTH: f64 = 164.0;
const CARD_HEIGHT: f64 = 126.0;
const GAP: f64 = 12.0;

struct Entry {
    code: &'static str,
    name: String,
    button: Retained<NSButton>,
    root: Retained<CALayer>,
    search: String,
    card: Retained<NSView>,
}

pub(super) struct FlagsGallery {
    window: Retained<NSWindow>,
    search: Retained<NSSearchField>,
    count: Retained<NSTextField>,
    scroll: Retained<NSScrollView>,
    document: Retained<NSView>,
    empty: Retained<NSTextField>,
    entries: Vec<Entry>,
    layouts: Vec<(ActiveInputSource, Vec<String>)>,
    layout: Retained<NSPopUpButton>,
    preview: Retained<NSView>,
    choice: Retained<NSTextField>,
    automatic: Retained<NSButton>,
    save: Retained<NSButton>,
    revert: Retained<NSButton>,
    status: Retained<NSTextField>,
}

impl FlagsGallery {
    pub fn new(controller: &NativeController, mtm: MainThreadMarker) -> Self {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, WIDTH, HEIGHT),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("All Country Flags"));
        window.center();
        let content = window.contentView().expect("flag gallery content view");
        let title = label("All country flags", rect(24.0, 728.0, 730.0, 30.0), mtm);
        title.setFont(Some(&NSFont::boldSystemFontOfSize(22.0)));
        content.addSubview(&title);
        content.addSubview(&label(
            "Select a keyboard layout, choose a flag, then Save Flags.",
            rect(24.0, 700.0, 732.0, 22.0),
            mtm,
        ));
        content.addSubview(&label(
            "Keyboard layout",
            rect(24.0, 664.0, 120.0, 24.0),
            mtm,
        ));
        let layouts = system_layout::indicator_sources().unwrap_or_default();
        let names: Vec<_> = layouts
            .iter()
            .map(|(source, _)| source.source_name.as_str())
            .collect();
        let layout = popup(&names, rect(148.0, 660.0, 340.0, 30.0), mtm);
        unsafe {
            layout.setTarget(Some(controller));
            layout.setAction(Some(sel!(flagLayoutChanged:)));
        }
        content.addSubview(&layout);
        let automatic = action_button(
            "Use Automatic Flag",
            rect(504.0, 660.0, 252.0, 30.0),
            controller,
            sel!(useAutomaticFlag:),
            mtm,
        );
        content.addSubview(&automatic);
        let preview = NSView::initWithFrame(NSView::alloc(mtm), rect(24.0, 552.0, 732.0, 100.0));
        content.addSubview(&preview);
        let choice = label("", rect(24.0, 526.0, 732.0, 22.0), mtm);
        content.addSubview(&choice);
        let search =
            NSSearchField::initWithFrame(NSSearchField::alloc(mtm), rect(24.0, 486.0, 470.0, 30.0));
        search.setPlaceholderString(Some(&NSString::from_str("Search country name or code")));
        search.setSendsSearchStringImmediately(true);
        unsafe {
            search.setTarget(Some(controller));
            search.setAction(Some(sel!(searchCountryFlags:)));
        }
        content.addSubview(&search);
        let count = label("", rect(512.0, 490.0, 244.0, 24.0), mtm);
        content.addSubview(&count);
        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(20.0, 64.0, 740.0, 406.0));
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        let document = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 720.0, 406.0));
        scroll.setDocumentView(Some(&document));
        content.addSubview(&scroll);
        let empty = label(
            "No flags match your search.",
            rect(160.0, 220.0, 430.0, 26.0),
            mtm,
        );
        document.addSubview(&empty);

        let status = label("", rect(24.0, 16.0, 464.0, 38.0), mtm);
        status.setMaximumNumberOfLines(2);
        status.setUsesSingleLineMode(false);
        content.addSubview(&status);
        let revert = action_button(
            "Revert",
            rect(500.0, 16.0, 110.0, 32.0),
            controller,
            sel!(revertCountryFlags:),
            mtm,
        );
        let save = action_button(
            "Save Flags",
            rect(620.0, 16.0, 136.0, 32.0),
            controller,
            sel!(saveCountryFlags:),
            mtm,
        );
        content.addSubview(&revert);
        content.addSubview(&save);
        window.setInitialFirstResponder(Some(&search));
        let locale = NSLocale::currentLocale();
        let english =
            NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &NSString::from_str("en"));
        let mut flags: Vec<_> = globe_flags::images()
            .map(|(code, png)| {
                let name = locale
                    .localizedStringForCountryCode(&NSString::from_str(code))
                    .map_or_else(|| code.to_owned(), |name| name.to_string());
                let english_name = english
                    .localizedStringForCountryCode(&NSString::from_str(code))
                    .map_or_else(|| code.to_owned(), |name| name.to_string());
                (code, png, name, english_name)
            })
            .collect();
        flags.sort_by_cached_key(|(code, _, name, _)| (name.to_lowercase(), *code));
        let entries = flags
            .into_iter()
            .enumerate()
            .map(|(index, (code, png, name, english_name))| {
                let (card, button, root) = card(code, &name, png, index, controller, mtm);
                document.addSubview(&card);
                Entry {
                    code,
                    search: format!("{code} {name} {english_name}").to_lowercase(),
                    name,
                    button,
                    root,
                    card,
                }
            })
            .collect();
        let gallery = Self {
            window,
            search,
            count,
            scroll,
            document,
            empty,
            entries,
            layouts,
            layout,
            preview,
            choice,
            automatic,
            save,
            revert,
            status,
        };
        gallery.refresh();
        gallery
    }

    pub fn show(&self) {
        self.window.deminiaturize(None);
        self.window.makeKeyAndOrderFront(None);
        self.window.makeFirstResponder(Some(&self.search));
    }

    pub fn select_layout(&self, source: Option<&str>) {
        if let Some(index) = source.and_then(|id| {
            self.layouts
                .iter()
                .position(|(source, _)| source.source_id == id)
        }) {
            self.layout.selectItemAtIndex(index as isize);
        }
    }

    pub fn selected_layout_id(&self) -> Option<&str> {
        self.selected_layout()
            .map(|(source, _)| source.source_id.as_str())
    }

    fn selected_layout(&self) -> Option<&(ActiveInputSource, Vec<String>)> {
        usize::try_from(self.layout.indexOfSelectedItem())
            .ok()
            .and_then(|index| self.layouts.get(index))
    }

    pub fn code(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|entry| entry.code)
    }

    pub fn update(&self, config: &Config, dirty: bool) {
        for child in self.preview.subviews().to_vec() {
            child.removeFromSuperview();
        }
        let source = self.selected_layout();
        let active = source.map(|(source, _)| source.source_id.as_str());
        let preview_config = Config {
            indicator_style: IndicatorStyle::Flag,
            ..config.clone()
        };
        let view = globe_indicator::preview(
            &preview_config,
            active,
            self.preview.bounds(),
            self.window.mtm(),
        );
        self.preview.addSubview(&view);
        let flag = source.and_then(|(source, languages)| {
            globe_flags::flag(
                &source.source_id,
                languages,
                &config.globe_switch.layout_flags,
            )
        });
        let selected = match &flag {
            Some(Flag::Region(code)) => Some(code.as_str()),
            _ => None,
        };
        let accent = NSColor::controlAccentColor().CGColor();
        for entry in &self.entries {
            let chosen = selected == Some(entry.code);
            entry.root.setBorderColor(Some(&accent));
            entry.root.setBorderWidth(if chosen { 2.0 } else { 0.0 });
            entry.button.setEnabled(active.is_some());
            set_checked(&entry.button, chosen);
        }
        let override_set =
            active.is_some_and(|id| config.globe_switch.layout_flags.contains_key(id));
        let choice = match flag {
            Some(Flag::Region(code)) => self
                .entries
                .iter()
                .find(|entry| entry.code == code)
                .map_or(code.clone(), |entry| format!("{} ({code})", entry.name)),
            Some(Flag::Text(text)) => text,
            None => "Layout name".to_owned(),
        };
        self.choice
            .setStringValue(&NSString::from_str(&if active.is_none() {
                "Add a keyboard input source in macOS Settings to assign a flag.".to_owned()
            } else if override_set {
                format!("Selected: {choice}")
            } else {
                format!("Automatic: {choice}")
            }));
        self.layout.setEnabled(!self.layouts.is_empty());
        self.automatic.setEnabled(override_set);
        self.save.setEnabled(dirty);
        self.revert.setEnabled(dirty);
        self.set_status(
            true,
            if dirty {
                "Preview only. Choose Save Flags to apply your changes."
            } else {
                "Flag choices are saved."
            },
        );
    }

    pub fn set_status(&self, success: bool, message: &str) {
        self.status.setStringValue(&NSString::from_str(message));
        let color = if success {
            NSColor::secondaryLabelColor()
        } else {
            NSColor::systemRedColor()
        };
        self.status.setTextColor(Some(&color));
    }

    pub fn refresh(&self) {
        let query = self.search.stringValue().to_string().trim().to_lowercase();
        let exact_code = self
            .entries
            .iter()
            .any(|entry| entry.code.eq_ignore_ascii_case(&query));
        let matches: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| {
                if exact_code {
                    entry.code.eq_ignore_ascii_case(&query)
                } else {
                    entry.search.contains(&query)
                }
            })
            .collect();
        self.count.setStringValue(&NSString::from_str(&format!(
            "{} of {} flags",
            matches.len(),
            self.entries.len()
        )));
        let rows = matches.len().div_ceil(COLUMNS);
        let height =
            (GAP + rows as f64 * (CARD_HEIGHT + GAP)).max(self.scroll.contentSize().height);
        self.document.setFrameSize(NSSize::new(720.0, height));
        self.empty.setHidden(!matches.is_empty());
        for entry in &self.entries {
            entry.card.setHidden(true);
        }
        for (index, entry) in matches.iter().enumerate() {
            entry.card.setFrameOrigin(NSPoint::new(
                GAP + (index % COLUMNS) as f64 * (CARD_WIDTH + GAP),
                height - (index / COLUMNS + 1) as f64 * (CARD_HEIGHT + GAP),
            ));
            entry.card.setHidden(false);
        }
        self.scroll.contentView().scrollToPoint(NSPoint::new(
            0.0,
            (height - self.scroll.contentSize().height).max(0.0),
        ));
        self.scroll
            .reflectScrolledClipView(&self.scroll.contentView());
    }
}

fn card(
    code: &str,
    name: &str,
    png: &[u8],
    index: usize,
    controller: &NativeController,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, Retained<NSButton>, Retained<CALayer>) {
    let card = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, CARD_WIDTH, CARD_HEIGHT));
    let root = CALayer::new();
    root.setCornerRadius(10.0);
    root.setBackgroundColor(Some(&NSColor::controlBackgroundColor().CGColor()));
    card.setLayer(Some(&root));
    card.setWantsLayer(true);
    if let Some(flag) = globe_indicator::pixel_flag_layer(png) {
        flag.setPosition(NSPoint::new(CARD_WIDTH / 2.0, 92.0));
        root.addSublayer(&flag);
    }
    let country = label(name, rect(6.0, 30.0, CARD_WIDTH - 12.0, 34.0), mtm);
    country.setAlignment(NSTextAlignment::Center);
    country.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    country.setMaximumNumberOfLines(2);
    country.setUsesSingleLineMode(false);
    country.setToolTip(Some(&NSString::from_str(&format!("{name} ({code})"))));
    card.addSubview(&country);
    let code_label = label(code, rect(6.0, 8.0, CARD_WIDTH - 12.0, 18.0), mtm);
    code_label.setAlignment(NSTextAlignment::Center);
    code_label.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
    code_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    card.addSubview(&code_label);
    // The transparent native button covers the artwork and labels so every
    // point in the card activates the same accessible, keyboard-focusable control.
    let button = NSButton::initWithFrame(NSButton::alloc(mtm), card.bounds());
    button.setTitle(&NSString::from_str(&format!("{name} ({code})")));
    button.setToolTip(Some(&NSString::from_str(&format!(
        "Choose {name} ({code})"
    ))));
    button.setButtonType(NSButtonType::PushOnPushOff);
    button.setTransparent(true);
    button.setTag(index as isize);
    unsafe {
        button.setTarget(Some(controller));
        button.setAction(Some(sel!(chooseCountryFlag:)));
    }
    card.addSubview(&button);
    (card, button, root)
}
