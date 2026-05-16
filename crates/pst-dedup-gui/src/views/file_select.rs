//! File selection view — add/remove PST files.

use crate::app::{AppState, PstDedupApp};
use eframe::egui;

pub fn show(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    ui.add_space(12.0);
    ui.label("Select one or more PST files to deduplicate:");
    ui.add_space(8.0);

    if ui.button("📁  Add PST Files...").clicked() {
        app.open_file_dialog();
    }

    ui.add_space(8.0);

    if app.pst_files().is_empty() {
        ui.colored_label(
            egui::Color32::GRAY,
            "No files selected. Click above to add PST files.",
        );
    } else {
        ui.label(format!("{} file(s) selected:", app.pst_files().len()));
        ui.add_space(4.0);

        let mut remove_index: Option<usize> = None;

        egui::ScrollArea::vertical()
            .max_height(350.0)
            .show(ui, |ui| {
                for (i, path) in app.pst_files().iter().enumerate() {
                    ui.horizontal(|ui| {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.display().to_string());

                        let size = std::fs::metadata(path)
                            .map(|m| format_size(m.len()))
                            .unwrap_or_else(|_| "?".into());

                        ui.label(format!("{}  ({})", name, size));
                        if ui.small_button("✕").clicked() {
                            remove_index = Some(i);
                        }
                    });
                }
            });

        if let Some(idx) = remove_index {
            app.pst_files_mut().remove(idx);
        }
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.add_enabled_ui(!app.pst_files().is_empty(), |ui| {
            if ui.button("Next →  Configure").clicked() {
                app.set_state(AppState::Settings);
            }
        });
    });
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}
