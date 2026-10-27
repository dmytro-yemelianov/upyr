//! GlobeSwitch's nonactivating flag row, ported from its MIT AppKit indicator.
#![allow(unsafe_code)]

use super::globe_flags::{self, Flag};
use crate::{config::Config, system_layout};
use block2::RcBlock;
use objc2::{
    AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSBackingStoreType, NSColor, NSEvent,
    NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSPanel, NSScreen,
    NSStatusWindowLevel, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{
    NSAttributedString, NSData, NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSValue,
};
use objc2_quartz_core::{
    CALayer, CAMediaTiming, CASpringAnimation, CATransaction, kCAFilterNearest,
};
use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
    rc::Rc,
};

const SLOT: f64 = 78.0;
const HEIGHT: f64 = 100.0;

define_class!(
    #[unsafe(super = NSPanel)]
    #[thread_kind = MainThreadOnly]
    struct FlagPanel;
    unsafe impl NSObjectProtocol for FlagPanel {}
    impl FlagPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool { false }
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool { false }
    }
);

#[derive(Clone, PartialEq, Eq)]
struct Item {
    id: String,
    name: String,
    flag: Option<Flag>,
}

struct Indicator {
    panel: Retained<FlagPanel>,
    root: Retained<CALayer>,
    items: Vec<Item>,
    layers: Vec<Retained<CALayer>>,
    active: Option<String>,
    generation: Rc<Cell<u64>>,
    backing_scale: f64,
}

thread_local! {
    static INDICATOR: RefCell<Option<Indicator>> = const { RefCell::new(None) };
}

pub(super) fn show(active: Option<&str>, previous: Option<&str>, config: &Config) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    // Both indicator styles share the same lifecycle; never leave the old
    // pointer label visible underneath the row after a settings change.
    super::platform::hide_indicator();
    let current = system_layout::current().ok().flatten();
    let Some(active) = active.or_else(|| current.as_ref().map(|source| source.source_id.as_str()))
    else {
        return false;
    };
    let items = items(active, previous, config);
    let Some(target) = items.iter().position(|item| item.id == active) else {
        return false;
    };
    let pointer = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .to_vec()
        .into_iter()
        .find(|screen| {
            let frame = screen.frame();
            pointer.x >= frame.origin.x
                && pointer.y >= frame.origin.y
                && pointer.x < frame.origin.x + frame.size.width
                && pointer.y < frame.origin.y + frame.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm));
    let Some(screen) = screen else { return false };
    INDICATOR.with_borrow_mut(|stored| {
        let indicator = stored.get_or_insert_with(|| Indicator::new(mtm));
        let was_showing = indicator.panel.isVisible();
        let width = SLOT * (2 * items.len() - 1) as f64;
        let visible = screen.visibleFrame();
        indicator.panel.setFrame_display(
            NSRect::new(
                NSPoint::new(
                    visible.origin.x + (visible.size.width - width) / 2.0,
                    visible.origin.y + visible.size.height * 0.16,
                ),
                NSSize::new(width, HEIGHT),
            ),
            false,
        );
        if indicator.items != items || indicator.backing_scale != screen.backingScaleFactor() {
            indicator.rebuild(items, screen.backingScaleFactor());
        }
        if !was_showing {
            let previous = previous.or(indicator.active.as_deref());
            let start = previous
                .and_then(|id| indicator.items.iter().position(|item| item.id == id))
                .unwrap_or(target);
            indicator.arrange(start, false);
        }
        indicator.arrange(target, true);
        indicator.active = Some(active.to_owned());
        indicator
            .generation
            .set(indicator.generation.get().wrapping_add(1));
        let panel = indicator.panel.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            unsafe { context.as_ref() }.setDuration(0.0);
            panel.animator().setAlphaValue(1.0);
        });
        NSAnimationContext::runAnimationGroup(&changes);
        indicator.panel.orderFrontRegardless();
    });
    true
}

pub(super) fn hide() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    INDICATOR.with_borrow(|stored| {
        let Some(indicator) = stored
            .as_ref()
            .filter(|indicator| indicator.panel.isVisible())
        else {
            return;
        };
        let generation = indicator.generation.clone();
        let shown = generation.get();
        let panel = indicator.panel.clone();
        let completed_panel = panel.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            unsafe { context.as_ref() }.setDuration(0.25);
            panel.animator().setAlphaValue(0.0);
        });
        let completion = RcBlock::new(move || {
            if generation.get() == shown {
                completed_panel.orderOut(None);
            }
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&completion));
    });
}

impl Indicator {
    fn new(mtm: MainThreadMarker) -> Self {
        let panel = unsafe {
            msg_send![FlagPanel::alloc(mtm), initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SLOT, HEIGHT)), styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel, backing: NSBackingStoreType::Buffered, defer: true]
        };
        let panel: Retained<FlagPanel> = panel;
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setLevel(NSStatusWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::Transient,
        );
        panel.setIgnoresMouseEvents(true);
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setHidesOnDeactivate(false);
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SLOT, HEIGHT)),
        );
        let root = CALayer::new();
        view.setLayer(Some(&root));
        view.setWantsLayer(true);
        panel.setContentView(Some(&view));
        Self {
            panel,
            root,
            items: Vec::new(),
            layers: Vec::new(),
            active: None,
            generation: Rc::new(Cell::new(0)),
            backing_scale: 0.0,
        }
    }

    fn rebuild(&mut self, items: Vec<Item>, scale: f64) {
        for layer in &self.layers {
            layer.removeFromSuperlayer();
        }
        self.layers = layers(&self.root, &items, scale);
        self.items = items;
        self.backing_scale = scale;
    }

    fn arrange(&self, selected: usize, animated: bool) {
        arrange_layers(
            &self.layers,
            selected,
            SLOT * (2 * self.layers.len() - 1) as f64 / 2.0,
            animated,
        );
    }
}

fn animate(layer: &CALayer, key: &str, value: &AnyObject, animated: bool) {
    let key = NSString::from_str(key);
    unsafe {
        if animated {
            let animation = CASpringAnimation::animationWithKeyPath(Some(&key));
            let presentation = layer.presentationLayer();
            let source = presentation.as_deref().unwrap_or(layer);
            let from: Option<Retained<AnyObject>> = msg_send![source, valueForKeyPath: &*key];
            animation.setFromValue(from.as_deref());
            animation.setToValue(Some(value));
            animation.setStiffness(320.0);
            animation.setDamping(28.0);
            animation.setDuration(animation.settlingDuration());
            layer.addAnimation_forKey(&animation, Some(&key));
        }
        let _: () = msg_send![layer, setValue: Some(value), forKeyPath: &*key];
    }
}

fn picture(item: &Item) -> Retained<NSImage> {
    let (text, size) = match &item.flag {
        Some(Flag::Text(text)) => (text.as_str(), 56.0),
        _ => (item.name.as_str(), 26.0),
    };
    let font = NSFont::boldSystemFontOfSize(size);
    let color = NSColor::whiteColor();
    let values: [&AnyObject; 2] = [&font, &color];
    let attributes = NSDictionary::from_slices(
        &[unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }],
        &values,
    );
    // Both attribute values have the types required by their AppKit keys.
    let text = unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attributes),
        )
    };
    let size: NSSize = unsafe { msg_send![&*text, size] };
    let draw = RcBlock::new(move |_bounds: NSRect| -> Bool {
        unsafe {
            let _: () = msg_send![&*text, drawAtPoint: NSPoint::new(0.0, 0.0)];
        }
        true.into()
    });
    NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(size.width.ceil(), size.height.ceil()),
        false,
        &draw,
    )
}

fn items(active: &str, previous: Option<&str>, config: &Config) -> Vec<Item> {
    let Ok(sources) = system_layout::indicator_sources() else {
        return Vec::new();
    };
    sources
        .into_iter()
        .filter(|(source, _)| {
            globe_flags::shown(
                &source.source_id,
                &config.globe_switch.skipped_layouts,
                previous,
                active,
            )
        })
        .map(|(source, languages)| Item {
            flag: globe_flags::flag(
                &source.source_id,
                &languages,
                &config.globe_switch.layout_flags,
            ),
            id: source.source_id,
            name: source.source_name,
        })
        .collect()
}

fn layers(root: &CALayer, items: &[Item], scale: f64) -> Vec<Retained<CALayer>> {
    items
        .iter()
        .map(|item| {
            let bitmap = match &item.flag {
                Some(Flag::Region(region)) => globe_flags::png(region).and_then(pixel_flag_layer),
                _ => None,
            };
            let layer = bitmap.unwrap_or_else(|| {
                let layer = CALayer::new();
                let image = picture(item);
                layer.setBounds(NSRect::new(NSPoint::new(0.0, 0.0), image.size()));
                layer.setContentsScale(scale);
                unsafe { layer.setContents(Some(&image.layerContentsForContentsScale(scale))) };
                layer
            });
            layer.setShadowOffset(NSSize::new(0.0, 0.0));
            root.addSublayer(&layer);
            layer
        })
        .collect()
}

fn arrange_layers(layers: &[Retained<CALayer>], selected: usize, centre: f64, animated: bool) {
    let glow = NSColor::controlAccentColor().CGColor();
    let shade = NSColor::blackColor().CGColor();
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    for (index, layer) in layers.iter().enumerate() {
        let active = index == selected;
        let position = unsafe {
            NSValue::valueWithPoint(NSPoint::new(
                centre + (index as f64 - selected as f64) * SLOT,
                HEIGHT / 2.0,
            ))
        };
        animate(layer, "position", &position, animated);
        animate(
            layer,
            "transform.scale",
            &NSNumber::new_f64(if active { 1.0 } else { 0.55 }),
            animated,
        );
        animate(
            layer,
            "opacity",
            &NSNumber::new_f32(if active { 1.0 } else { 0.45 }),
            animated,
        );
        layer.setShadowColor(Some(if active { &glow } else { &shade }));
        animate(
            layer,
            "shadowOpacity",
            &NSNumber::new_f32(if active { 0.9 } else { 0.35 }),
            animated,
        );
        animate(
            layer,
            "shadowRadius",
            &NSNumber::new_f64(if active { 10.0 } else { 3.0 }),
            animated,
        );
    }
    CATransaction::commit();
}

/// Embedded settings preview uses the exact flag assets, skips, overrides,
/// selected scaling, opacity and glow of the live GlobeSwitch indicator.
pub(crate) fn preview(
    config: &Config,
    active: Option<&str>,
    frame: NSRect,
    mtm: MainThreadMarker,
) -> Retained<NSView> {
    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    let root = CALayer::new();
    root.setBackgroundColor(Some(&NSColor::controlBackgroundColor().CGColor()));
    root.setCornerRadius(10.0);
    root.setMasksToBounds(true);
    view.setLayer(Some(&root));
    view.setWantsLayer(true);
    let current = system_layout::current().ok().flatten();
    let active = active.or_else(|| current.as_ref().map(|s| s.source_id.as_str()));
    if let Some(active) = active {
        let items = items(active, None, config);
        if let Some(target) = items.iter().position(|item| item.id == active) {
            let scale = view
                .window()
                .map_or(2.0, |window| window.backingScaleFactor());
            let layers = layers(&root, &items, scale);
            arrange_layers(&layers, target, frame.size.width / 2.0, false);
        }
    }
    view
}

/// Shared crisp 3× rendering for the live indicator and country gallery.
pub(crate) fn pixel_flag_layer(png: &[u8]) -> Option<Retained<CALayer>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))?;
    let layer = CALayer::new();
    layer.setBounds(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(66.0, 42.0)));
    unsafe { layer.setContents(Some(&image.layerContentsForContentsScale(1.0))) };
    layer.setMagnificationFilter(unsafe { kCAFilterNearest });
    layer.setMinificationFilter(unsafe { kCAFilterNearest });
    Some(layer)
}
