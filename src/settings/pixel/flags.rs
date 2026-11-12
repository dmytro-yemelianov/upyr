//! Pixel flag views over the same source list, assets and mapping as native UI.
use crate::{
    config::{Config, IndicatorStyle},
    feedback::globe_flags::{self, Flag},
    system_layout::{self, ActiveInputSource},
};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use objc2::AnyThread;
use objc2_foundation::{NSLocale, NSString};
use std::time::{Duration, Instant};

struct Entry {
    code: &'static str,
    name: String,
    search: String,
    png: &'static [u8],
    texture: Option<TextureHandle>,
}

#[derive(Default)]
pub(crate) struct FlagPicker {
    sources: Vec<(ActiveInputSource, Vec<String>)>,
    selected: usize,
    entries: Vec<Entry>,
    search: String,
    gallery: bool,
    problem: Option<String>,
    preview_until: Option<Instant>,
}

impl FlagPicker {
    pub fn poll_preview(&mut self, ctx: &egui::Context) {
        if let Some(deadline) = self.preview_until {
            if Instant::now() >= deadline {
                crate::feedback::hide_layout_indicator();
                self.preview_until = None;
            } else {
                ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
            }
        }
    }
    pub fn load(&mut self) {
        match system_layout::indicator_sources() {
            Ok(sources) => {
                self.sources = sources;
                self.problem = None;
                self.selected = system_layout::current()
                    .ok()
                    .flatten()
                    .and_then(|current| {
                        self.sources
                            .iter()
                            .position(|(source, _)| source.source_id == current.source_id)
                    })
                    .unwrap_or(0);
            }
            Err(error) => {
                self.problem = Some(format!("Could not read keyboard layouts: {error:#}"))
            }
        }
        if !self.entries.is_empty() {
            return;
        }
        let locale = NSLocale::currentLocale();
        let english =
            NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &NSString::from_str("en"));
        self.entries = globe_flags::images()
            .map(|(code, png)| {
                let country = NSString::from_str(code);
                let name = locale
                    .localizedStringForCountryCode(&country)
                    .map_or_else(|| code.to_owned(), |name| name.to_string());
                let english_name = english
                    .localizedStringForCountryCode(&country)
                    .map_or_else(|| code.to_owned(), |name| name.to_string());
                Entry {
                    code,
                    search: format!("{code} {name} {english_name}").to_lowercase(),
                    name,
                    png,
                    texture: None,
                }
            })
            .collect();
        self.entries
            .sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.code));
    }

    fn image(ui: &mut egui::Ui, entry: &mut Entry, size: egui::Vec2) {
        if entry.texture.is_none() {
            if let Ok(image) = image::load_from_memory(entry.png) {
                let rgba = image.to_rgba8();
                let dimensions = [rgba.width() as usize, rgba.height() as usize];
                entry.texture = Some(ui.ctx().load_texture(
                    format!("flag-{}", entry.code),
                    ColorImage::from_rgba_unmultiplied(dimensions, rgba.as_raw()),
                    TextureOptions::NEAREST,
                ));
            }
        }
        if let Some(texture) = &entry.texture {
            ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(size)
                    .alt_text(format!("{} flag", entry.name)),
            );
        } else {
            ui.label(entry.code);
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        config: &mut Config,
        status: &mut Option<(bool, String)>,
    ) {
        ui.heading("Layout flags");
        if let Some(problem) = &self.problem {
            ui.label(problem);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Preview layout");
            egui::ComboBox::from_id_salt("flag-layout")
                .selected_text(
                    self.sources
                        .get(self.selected)
                        .map_or("No keyboard layouts", |(source, _)| &source.source_name),
                )
                .show_ui(ui, |ui| {
                    for (index, (source, _)) in self.sources.iter().enumerate() {
                        ui.selectable_value(&mut self.selected, index, &source.source_name);
                    }
                });
            if ui.button("Refresh layouts").clicked() {
                self.load();
            }
        });
        let selected = self.sources.get(self.selected).cloned();
        if let Some((source, languages)) = &selected {
            ui.horizontal_wrapped(|ui| {
                let flag = globe_flags::flag(
                    &source.source_id,
                    languages,
                    &config.globe_switch.layout_flags,
                );
                if config.indicator_style == IndicatorStyle::Flag {
                    match flag {
                        Some(Flag::Region(code)) => {
                            if let Some(entry) =
                                self.entries.iter_mut().find(|entry| entry.code == code)
                            {
                                Self::image(ui, entry, egui::vec2(88.0, 56.0));
                            }
                        }
                        Some(Flag::Text(text)) => {
                            ui.heading(text);
                        }
                        None => {
                            ui.label(&source.source_name);
                        }
                    }
                } else if let Some(layout) = source.layout {
                    ui.heading(crate::feedback::indicator_text(
                        layout,
                        config.indicator_style,
                    ));
                }
                ui.label(
                    config
                        .globe_switch
                        .layout_flags
                        .get(&source.source_id)
                        .map_or("Automatic flag", String::as_str),
                );
                if ui.button("Preview indicator").clicked() {
                    let mut preview = config.clone();
                    preview.show_layout_indicator = true;
                    let shown = crate::feedback::globe_switched(None, source, &preview);
                    if shown {
                        self.preview_until = Some(
                            Instant::now()
                                + Duration::from_millis(preview.layout_indicator_duration_ms),
                        );
                        ui.ctx().request_repaint_after(Duration::from_millis(
                            preview.layout_indicator_duration_ms,
                        ));
                    }
                    *status = Some((
                        shown,
                        if shown {
                            "Indicator preview shown."
                        } else {
                            "Indicator preview unavailable."
                        }
                        .into(),
                    ));
                }
            });
            if ui.button("Use Automatic Flag").clicked() {
                config.globe_switch.layout_flags.remove(&source.source_id);
            }
        }
        ui.toggle_value(&mut self.gallery, "All Country Flags…");
        if self.gallery {
            let label = ui.label("Search country name or code");
            ui.add(egui::TextEdit::singleline(&mut self.search).id(egui::Id::new("flag-search")))
                .labelled_by(label.id);
            let query = self.search.trim().to_lowercase();
            let matches: Vec<_> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.search.contains(&query))
                .map(|(index, _)| index)
                .collect();
            ui.label(format!(
                "{} flags · choose one, then Save settings",
                matches.len()
            ));
            let columns = ((ui.available_width() / 170.0) as usize).max(1);
            let rows = matches.len().div_ceil(columns);
            egui::ScrollArea::vertical()
                .id_salt("flag-gallery")
                .max_height(280.0)
                .show_rows(ui, 100.0, rows, |ui, range| {
                    for row in range {
                        ui.horizontal(|ui| {
                            for index in matches.iter().skip(row * columns).take(columns) {
                                let entry = &mut self.entries[*index];
                                ui.push_id(entry.code, |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(155.0);
                                        Self::image(ui, entry, egui::vec2(66.0, 42.0));
                                        let label = format!("{} · {}", entry.code, entry.name);
                                        if ui
                                            .add_enabled(
                                                selected.is_some(),
                                                egui::Button::new(label).wrap(),
                                            )
                                            .clicked()
                                        {
                                            if let Some((source, _)) = &selected {
                                                config.globe_switch.layout_flags.insert(
                                                    source.source_id.clone(),
                                                    entry.code.into(),
                                                );
                                            }
                                        }
                                    });
                                });
                            }
                        });
                    }
                });
        }
        ui.separator();
    }
}

impl Drop for FlagPicker {
    fn drop(&mut self) {
        if self.preview_until.is_some() {
            crate::feedback::hide_layout_indicator();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_flags_render_and_picker_edits_only_the_selected_source() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut picker = FlagPicker {
            sources: vec![(
                ActiveInputSource {
                    layout: Some(system_layout::SystemLayout::English),
                    source_id: "ABC".into(),
                    source_name: "ABC".into(),
                },
                vec!["en".into()],
            )],
            entries: globe_flags::images()
                .map(|(code, png)| {
                    let decoded = image::load_from_memory(png).expect("bundled flag PNG");
                    assert!(decoded.width() > 0 && decoded.height() > 0);
                    Entry {
                        code,
                        name: code.into(),
                        search: code.to_lowercase(),
                        png,
                        texture: None,
                    }
                })
                .collect(),
            gallery: true,
            search: "ua".into(),
            selected: 0,
            problem: None,
            preview_until: None,
        };
        let mut config = Config {
            indicator_style: IndicatorStyle::Flag,
            ..Default::default()
        };
        config
            .globe_switch
            .layout_flags
            .insert("ABC".into(), "GB".into());
        config
            .globe_switch
            .layout_flags
            .insert("other".into(), "Custom label".into());
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 700.0),
                    )),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| picker.show(ui, &mut config, &mut None));
                },
            )
        };
        let first = frame(vec![]);
        assert!(!first.textures_delta.set.is_empty());
        for label in ["Use Automatic Flag", "UA · UA"] {
            let out = frame(vec![]);
            let tree = out.platform_output.accesskit_update.unwrap();
            let rect = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .expect(label)
                .1
                .bounds()
                .unwrap();
            let pos = egui::pos2(
                ((rect.x0 + rect.x1) / 2.0) as f32,
                ((rect.y0 + rect.y1) / 2.0) as f32,
            );
            for pressed in [true, false] {
                frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
        }
        assert_eq!(config.globe_switch.layout_flags.get("ABC").unwrap(), "UA");
        assert_eq!(
            config.globe_switch.layout_flags.get("other").unwrap(),
            "Custom label"
        );
        assert!(
            picker.preview_until.is_none(),
            "tests never trigger OS indicator previews"
        );
    }
}
