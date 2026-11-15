use super::i18n::tr;
use crate::application_layout::ApplicationLayoutRule;
use eframe::egui;
use upyr_core::InputLayout;

pub(super) const HELP: &str = "Choose the layout when an app becomes active. You can still change it manually until the next activation. Works independently of automatic correction; paused with Upyr.";
pub(super) const APP_HINT: &str = "Application name or bundle ID";

pub(super) fn draw(rules: &mut Vec<ApplicationLayoutRule>, ui: &mut egui::Ui) {
    super::section_heading(ui, tr("Application layouts"), tr(HELP));
    ui.add_space(12.0);
    let mut remove = None;
    for (index, rule) in rules.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut rule.application)
                        .hint_text(tr(APP_HINT))
                        .desired_width(220.0),
                );
                egui::ComboBox::from_id_salt("layout")
                    .selected_text(label(rule.layout))
                    .show_ui(ui, |ui| {
                        for layout in [InputLayout::English, InputLayout::Ukrainian] {
                            ui.selectable_value(&mut rule.layout, layout, label(layout));
                        }
                    });
                if ui.button(tr("Remove")).clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        rules.remove(index);
    }
    if rules.is_empty() {
        ui.label(tr("No application rules yet."));
    }
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(rules.len() < 128, egui::Button::new(tr("Add application")))
            .clicked()
        {
            rules.push(ApplicationLayoutRule {
                application: String::new(),
                layout: InputLayout::English,
            });
        }
        let has_raycast = rules
            .iter()
            .any(|r| r.application.eq_ignore_ascii_case("com.raycast.macos"));
        if ui
            .add_enabled(
                !has_raycast && rules.len() < 128,
                egui::Button::new(tr("Raycast → English")),
            )
            .clicked()
        {
            rules.push(ApplicationLayoutRule::raycast());
        }
    });
    ui.small(tr("Use an exact app name, such as Raycast, or its bundle ID. Upyr does not switch layouts for apps without a rule."));
}

fn label(layout: InputLayout) -> &'static str {
    match layout {
        InputLayout::English => tr("English"),
        InputLayout::Ukrainian => tr("Ukrainian"),
    }
}
