//! Shared layout components for the redesigned Preferences interface.
//! Uses the existing lhtPDF theme tokens and avoids popup controls.

use egui::{self, RichText};

use crate::theme::{self, Tokens};

/// A visually separated settings section.
pub(crate) fn section_card(
    ui: &mut egui::Ui,
    t: &Tokens,
    title: &str,
    content: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(t.card)
        .stroke(egui::Stroke::new(1.0, t.border))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_heading(ui, t, title);
            ui.add_space(8.0);
            content(ui);
        });
}

/// A section title using the current application typography.
pub(crate) fn section_heading(ui: &mut egui::Ui, t: &Tokens, title: &str) {
    ui.label(
        RichText::new(title)
            .font(theme::semibold(14.0))
            .color(t.text),
    );
}

/// A settings row with an optional description.
pub(crate) fn setting_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    title: &str,
    description: Option<&str>,
    content: impl FnOnce(&mut egui::Ui),
) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(title)
                .font(theme::semibold(12.5))
                .color(t.text),
        );

        if let Some(description) = description {
            ui.label(
                RichText::new(description)
                    .small()
                    .color(t.text_muted),
            );
        }

        ui.add_space(4.0);
        content(ui);
    });
}

/// A responsive group of inline selectable options.
///
/// Returns the index clicked in this frame, if any.
pub(crate) fn option_group(
    ui: &mut egui::Ui,
    selected: usize,
    labels: &[&str],
) -> Option<usize> {
    let mut clicked = None;

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        for (index, label) in labels.iter().enumerate() {
            if ui
                .selectable_label(index == selected, *label)
                .clicked()
            {
                clicked = Some(index);
            }
        }
    });

    clicked
}

/// Dropdown for Preferences with an independent egui popup layer.
///
/// The trigger owns its response, while every menu item owns a separate
/// response. No full-card click target or transparent overlay is created.
///
/// Use a unique `id_source` for each Preferences setting.
pub(crate) fn preferences_dropdown(
    ui: &mut egui::Ui,
    id_source: impl std::hash::Hash + std::fmt::Debug,
    selected: usize,
    labels: &[&str],
) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }

    let selected = selected.min(labels.len() - 1);
    let mut changed = None;

    egui::ComboBox::from_id_salt(id_source)
        .selected_text(labels[selected])
        .width(180.0)
        .show_ui(ui, |ui| {
            for (index, label) in labels.iter().enumerate() {
                if ui
                    .selectable_label(index == selected, *label)
                    .clicked()
                {
                    changed = Some(index);
                }
            }
        });
    changed
}