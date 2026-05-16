//! Settings view — configure dedup parameters before scanning.

use eframe::egui;
use crate::app::{AppState, PstDedupApp};

pub fn show(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    ui.add_space(12.0);
    ui.heading("Dedup Configuration");
    ui.add_space(8.0);

    // Tier 1 is always on
    ui.label("Tier 1 — Message-ID matching: always enabled");
    ui.add_space(8.0);

    // Tier 2 toggle
    let mut enable_tier2 = app.config().enable_tier2;
    ui.checkbox(&mut enable_tier2, "Enable Tier 2 — Content hash fallback");
    ui.label("  (For emails missing a Message-ID header)");
    app.config_mut().enable_tier2 = enable_tier2;

    if enable_tier2 {
        ui.add_space(4.0);
        ui.indent("tier2_opts", |ui| {
            // Body hash length
            let mut body_kb = (app.config().body_hash_len / 1024) as f32;
            ui.horizontal(|ui| {
                ui.label("Body preview for hash:");
                ui.add(egui::Slider::new(&mut body_kb, 1.0..=8.0).suffix(" KB"));
            });
            app.config_mut().body_hash_len = (body_kb as usize) * 1024;

            // Attachment metadata
            let mut include_att = app.config().include_attachments;
            ui.checkbox(
                &mut include_att,
                "Include attachment names/sizes in hash",
            );
            app.config_mut().include_attachments = include_att;
        });
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    // Output directory
    ui.horizontal(|ui| {
        ui.label("Output directory:");
        if let Some(dir) = app.config().output_dir.as_ref() {
            ui.label(dir.display().to_string());
        } else {
            ui.colored_label(egui::Color32::GRAY, "(will use PST file directory)");
        }
        if ui.button("Change...").clicked() {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                app.config_mut().output_dir = Some(dir);
            }
        }
    });

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button("← Back").clicked() {
            app.set_state(AppState::FileSelect);
        }
        ui.add_space(16.0);
        if ui.button("▶  Start Scan").clicked() {
            app.start_scan();
        }
    });
}
