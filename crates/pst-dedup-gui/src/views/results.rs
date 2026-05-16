//! Results view — scan summary, duplicate table, and export options.

use crate::app::PstDedupApp;
use dedup_engine::{format_bytes, truncate_utf8, DedupResult};
use eframe::egui;
use std::path::Path;

pub fn show(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    let results = match app.results() {
        Some(r) => r.clone(),
        None => {
            ui.label("No results available.");
            return;
        }
    };

    ui.add_space(12.0);
    ui.heading("Scan Complete");
    ui.add_space(8.0);

    // Summary stats
    egui::Grid::new("summary_stats")
        .num_columns(2)
        .spacing([40.0, 4.0])
        .show(ui, |ui| {
            ui.strong("Total messages:");
            ui.label(format!("{}", results.total_messages));
            ui.end_row();

            ui.strong("Unique:");
            ui.label(format!("{}", results.unique_count));
            ui.end_row();

            ui.strong("Duplicates:");
            ui.colored_label(
                egui::Color32::from_rgb(220, 120, 50),
                format!("{}", results.duplicate_count),
            );
            ui.end_row();

            ui.label("  Tier 1 (Message-ID):");
            ui.label(format!("{}", results.tier1_hits));
            ui.end_row();

            ui.label("  Tier 2 (Content Hash):");
            ui.label(format!("{}", results.tier2_hits));
            ui.end_row();

            ui.strong("Est. savings:");
            ui.label(format_bytes(results.savings_bytes));
            ui.end_row();

            ui.strong("Duration:");
            ui.label(format!("{:.1}s", results.duration_secs));
            ui.end_row();

            ui.strong("Throughput:");
            let mps = if results.duration_secs > 0.0 {
                results.total_messages as f64 / results.duration_secs
            } else {
                0.0
            };
            ui.label(format!("{:.0} msgs/sec", mps));
            ui.end_row();
        });

    ui.add_space(8.0);

    // Warning banner for partial results
    if results.failed_files > 0 {
        ui.colored_label(
            egui::Color32::YELLOW,
            format!(
                "Warning: {} file(s) could not be scanned. Results are partial.",
                results.failed_files
            ),
        );
        ui.add_space(4.0);
    }

    // Per-file breakdown
    if !results.file_stats.is_empty() {
        ui.collapsing("Per-file breakdown", |ui| {
            egui::Grid::new("file_breakdown")
                .num_columns(5)
                .spacing([20.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("File");
                    ui.strong("Messages");
                    ui.strong("Duplicates");
                    ui.strong("Skipped");
                    ui.strong("Status");
                    ui.end_row();

                    for fs in &results.file_stats {
                        ui.label(&fs.name);
                        ui.label(format!("{}", fs.messages));
                        ui.label(format!("{}", fs.duplicates));
                        ui.label(format!("{}", fs.skipped_messages));
                        if let Some(err) = &fs.error {
                            ui.colored_label(egui::Color32::RED, truncate_utf8(err, 40));
                        } else if fs.skipped_messages > 0 {
                            ui.colored_label(
                                egui::Color32::YELLOW,
                                format!("{} skipped", fs.skipped_messages),
                            );
                        } else {
                            ui.colored_label(egui::Color32::GREEN, "OK");
                        }
                        ui.end_row();
                    }
                });
        });
    }

    ui.add_space(8.0);

    // Duplicate table (scrollable, showing first N)
    let duplicates: Vec<_> = results
        .rows
        .iter()
        .filter(|r| matches!(r.result, DedupResult::DuplicateOf { .. }))
        .take(500) // Show first 500 for performance
        .collect();

    if !duplicates.is_empty() {
        ui.collapsing(
            format!(
                "Duplicate details (showing {} of {})",
                duplicates.len(),
                results.duplicate_count
            ),
            |ui| {
                egui::ScrollArea::both().max_height(300.0).show(ui, |ui| {
                    egui::Grid::new("dup_table")
                        .num_columns(5)
                        .spacing([12.0, 2.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Subject");
                            ui.strong("Sender");
                            ui.strong("PST");
                            ui.strong("Tier");
                            ui.strong("Original PST");
                            ui.end_row();

                            for row in &duplicates {
                                if let DedupResult::DuplicateOf { original, tier } = &row.result {
                                    let subj = truncate_utf8(&row.message.subject, 40);
                                    ui.label(subj);
                                    ui.label(truncate_utf8(&row.message.sender, 25));
                                    ui.label(&row.message.pst_name);
                                    ui.label(tier.to_string());
                                    ui.label(&original.pst_name);
                                    ui.end_row();
                                }
                            }
                        });
                });
            },
        );
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    // Export controls
    ui.horizontal(|ui| {
        if ui.button("📄  Export CSV Report").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name("dedup_report.csv")
                .save_file()
            {
                export_csv(app, &path);
            }
        }

        ui.add_space(12.0);

        if ui.button("📧  Export Unique Emails (EML)").clicked() {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title("Select output folder for EML files")
                .pick_folder()
            {
                let (exported, failed, err) =
                    crate::worker::export_unique_eml(&results, &dir, &results.source_files);
                app.set_export_result(exported, failed, err);
            }
        }

        ui.add_space(24.0);

        if ui.button("Start Over").clicked() {
            app.reset();
        }
    });

    // Export result feedback
    if let Some((exported, failed, err)) = app.export_result() {
        ui.add_space(8.0);
        if failed > 0 || err.is_some() {
            ui.colored_label(
                egui::Color32::YELLOW,
                format!(
                    "Export finished: {} written, {} failed.{}",
                    exported,
                    failed,
                    err.as_ref()
                        .map(|e| format!(" Last error: {}", truncate_utf8(e, 80)))
                        .unwrap_or_default()
                ),
            );
        } else {
            ui.colored_label(
                egui::Color32::GREEN,
                format!("Export complete: {} EML files written.", exported),
            );
        }
    }
}

fn export_csv(app: &PstDedupApp, path: &Path) {
    if let Some(results) = app.results() {
        match dedup_engine::write_csv_report(path, &results.rows) {
            Ok(()) => {
                tracing::info!("Report written to {}", path.display());
                // Append summary
                let _ = dedup_engine::report::write_summary_report(
                    path,
                    results.total_messages,
                    results.unique_count,
                    results.duplicate_count,
                    results.tier1_hits,
                    results.tier2_hits,
                    results.savings_bytes,
                );
            }
            Err(e) => {
                tracing::error!("Failed to write report: {}", e);
            }
        }
    }
}
