//! Preferences › Appearance: select the system, light or dark appearance mode.

use crate::PdfCraftApp;
use crate::theme::{self, ThemePreference, Tokens};

fn mode_label(preference: ThemePreference) -> &'static str {
    match preference {
        ThemePreference::System => "Use system setting",
        ThemePreference::Light => "Light",
        ThemePreference::Dark => "Dark",
    }
}

fn mode_command(preference: ThemePreference) -> &'static str {
    match preference {
        ThemePreference::System => "view.theme.system",
        ThemePreference::Light => "view.theme.light",
        ThemePreference::Dark => "view.theme.dark",
    }
}

/// Direct appearance selection using the shared Preferences components.
fn appearance_mode_selector(ui: &mut egui::Ui, app: &mut PdfCraftApp, _t: &Tokens) {
    ui.horizontal(|ui| {
        ui.label(tl!("Appearance mode"));
        egui::ComboBox::from_id_salt("appearance-mode")
            .selected_text(tl!(mode_label(app.theme_preference)))
            .show_ui(ui, |ui| {
                for preference in [
                    ThemePreference::System,
                    ThemePreference::Light,
                    ThemePreference::Dark,
                ] {
                    if ui
                        .selectable_label(
                            app.theme_preference == preference,
                            tl!(mode_label(preference)),
                        )
                        .clicked()
                    {
                        app.execute(mode_command(preference));
                        ui.close();
                    }
                }
            });
    });
}
/// Appearance preferences: one selector controls System, Light and Dark.
pub fn preferences_section(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) {
    crate::preferences_components::section_card(
        ui,
        t,
        tl!("Appearance"),
        |ui| {
            appearance_mode_selector(ui, app, t);
        },
    );
}
#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// The appearance mode is selected directly in Preferences.
    #[test]
    fn preferences_choose_the_appearance_mode() {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        let mut fonts = false;
        let mut h = Harness::builder().with_size(egui::vec2(640.0, 400.0)).build_ui_state(
            move |ui, app: &mut PdfCraftApp| {
                // The app's fonts (the semibold headings) take effect from the next frame.
                if !fonts {
                    ui.ctx().set_fonts(theme::font_definitions());
                    fonts = true;
                    return;
                }
                let t = Tokens::get(ui.ctx());
                preferences_section(ui, app, &t);
            },
            app,
        );
        h.run();
        assert!(h.query_by_label("Appearance mode").is_some());
        if let Ok(path) = std::env::var("PDFCRAFT_APPEARANCE_SHOT") {
            h.render().unwrap().save(path).unwrap();
        }

        // The appearance choices are only accessible while the dropdown is open.
        h.get_by_value("Light").click();
        h.run();
        h.get_by_label("Dark").click();
        h.run();
        assert_eq!(h.state().theme_preference, ThemePreference::Dark);

        // Reopen the dropdown before restoring the system appearance.
        h.get_by_value("Dark").click();
        h.run();
        h.get_by_label("Use system setting").click();
        h.run();
        assert_eq!(h.state().theme_preference, ThemePreference::System);
    }
}
