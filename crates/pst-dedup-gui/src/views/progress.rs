//! Progress view — shows live scan statistics during processing.

use crate::app::PstDedupApp;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    let progress = app
        .progress()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    ui.add_space(12.0);
    ui.heading("Scanning...");
    ui.add_space(8.0);

    // Current file
    ui.label(format!(
        "File {}/{}: {}",
        progress.current_file_index + 1,
        progress.total_files,
        progress.current_file,
    ));

    ui.add_space(8.0);

    // Overall progress bar
    let fraction = if progress.messages_estimated > 0 {
        progress.messages_processed as f32 / progress.messages_estimated as f32
    } else {
        0.0
    };

    ui.add(
        egui::ProgressBar::new(fraction.min(1.0))
            .text(format!(
                "{} / ~{} messages",
                progress.messages_processed, progress.messages_estimated
            ))
            .animate(true),
    );

    ui.add_space(12.0);

    // Stats grid
    egui::Grid::new("scan_stats")
        .num_columns(2)
        .spacing([40.0, 4.0])
        .show(ui, |ui| {
            ui.label("Throughput:");
            ui.label(format!("{:.0} msgs/sec", progress.messages_per_sec));
            ui.end_row();

            ui.label("Unique:");
            ui.label(format!("{}", progress.unique_count));
            ui.end_row();

            ui.label("Duplicates:");
            ui.label(format!("{}", progress.duplicate_count));
            ui.end_row();
        });

    if let Some(err) = &progress.error {
        ui.add_space(8.0);
        ui.colored_label(egui::Color32::YELLOW, format!("Warning: {}", err));
    }

    ui.add_space(16.0);

    // Cancel button
    if ui.button("Cancel").clicked() {
        let mut p = app.progress().lock().unwrap_or_else(|e| e.into_inner());
        p.cancelled = true;
    }
}
