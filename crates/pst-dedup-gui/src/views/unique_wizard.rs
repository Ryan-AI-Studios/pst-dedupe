//! Unique-PST wizard views: select → options → run → done (0072).

use crate::app::{AppState, PstDedupApp};
use crate::unique_wizard::{open_folder_nonblocking, UniqueWizardForm};
use dedup_engine::integrity::ScanMode;
use dedup_engine::keepset::{FamilyPolicy, KeepPolicy};
use eframe::egui;
use pst_dedup_cli::unique_pst_cmd::FolderLayoutArg;

pub fn show_select(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    ui.add_space(12.0);
    ui.heading("Unique PST Export — Select sources");
    ui.label(
        "Pick one or more source PST files (read-only). Same keep-set path as CLI unique-pst.",
    );
    ui.add_space(8.0);

    if ui.button("📁  Add PST Files…").clicked() {
        app.open_unique_input_dialog();
    }

    ui.add_space(8.0);
    let form = app.unique_form();
    if form.inputs.is_empty() {
        ui.colored_label(egui::Color32::GRAY, "No files selected.");
    } else {
        ui.label(format!("{} file(s):", form.inputs.len()));
        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for (i, path) in form.inputs.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(path.display().to_string());
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = remove {
            app.unique_form_mut().inputs.remove(i);
        }
    }

    ui.add_space(16.0);
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("← Back").clicked() {
            app.leave_unique_wizard();
        }
        ui.add_space(12.0);
        ui.add_enabled_ui(!app.unique_form().inputs.is_empty(), |ui| {
            if ui.button("Next → Options").clicked() {
                app.set_state(AppState::UniqueOptions);
            }
        });
    });
}

pub fn show_options(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    ui.add_space(12.0);
    ui.heading("Unique PST Export — Options");
    ui.add_space(8.0);

    // Out path (Save File dialog on main thread)
    ui.horizontal(|ui| {
        ui.strong("Output PST:");
        if let Some(out) = &app.unique_form().out {
            ui.label(out.display().to_string());
        } else {
            ui.colored_label(egui::Color32::GRAY, "(not set)");
        }
        if ui.button("Save as…").clicked() {
            app.open_unique_out_dialog();
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("Report dir:");
        let report = app.unique_form().effective_report_dir();
        if let Some(r) = report {
            ui.label(r.display().to_string());
        } else {
            ui.colored_label(egui::Color32::GRAY, "(derived from output)");
        }
        if ui.button("Choose…").clicked() {
            app.open_unique_report_dir_dialog();
        }
    });

    ui.add_space(12.0);

    // Policy
    ui.horizontal(|ui| {
        ui.label("Policy:");
        let mut policy = app.unique_form().policy;
        egui::ComboBox::from_id_salt("unique_policy")
            .selected_text(policy.as_str())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut policy, KeepPolicy::FirstSeen, "first_seen");
                ui.selectable_value(&mut policy, KeepPolicy::KeepLargest, "keep_largest");
                ui.selectable_value(&mut policy, KeepPolicy::PreferPath, "prefer_path");
            });
        app.unique_form_mut().policy = policy;
    });

    if app.unique_form().policy == KeepPolicy::PreferPath {
        ui.horizontal(|ui| {
            ui.label("Prefer path contains:");
            ui.add(
                egui::TextEdit::singleline(&mut app.unique_form_mut().prefer_path_text)
                    .hint_text("comma-separated substrings, e.g. Primary,Archive")
                    .desired_width(320.0),
            );
        });
        ui.colored_label(
            egui::Color32::GRAY,
            "Winners prefer source/folder paths matching any substring (case-insensitive).",
        );
    }

    ui.horizontal(|ui| {
        ui.label("Family:");
        let mut fam = app.unique_form().family_policy;
        egui::ComboBox::from_id_salt("unique_family")
            .selected_text(fam.as_str())
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut fam,
                    FamilyPolicy::KeepAttachmentsWithParent,
                    "keep_attachments_with_parent",
                );
                ui.selectable_value(&mut fam, FamilyPolicy::ParentsOnly, "parents_only");
            });
        app.unique_form_mut().family_policy = fam;
    });

    ui.horizontal(|ui| {
        ui.label("Folder layout:");
        let mut layout = app.unique_form().folder_layout;
        egui::ComboBox::from_id_salt("unique_layout")
            .selected_text(layout.as_str())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut layout, FolderLayoutArg::Preserve, "preserve");
                ui.selectable_value(&mut layout, FolderLayoutArg::Flat, "flat");
            });
        app.unique_form_mut().folder_layout = layout;
    });

    ui.horizontal(|ui| {
        ui.label("Mode:");
        let mut mode = app.unique_form().mode;
        egui::ComboBox::from_id_salt("unique_mode")
            .selected_text(mode.as_str())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mode, ScanMode::BestEffort, "best-effort");
                ui.selectable_value(&mut mode, ScanMode::Strict, "strict");
            });
        app.unique_form_mut().mode = mode;
    });

    ui.add_space(8.0);
    {
        let form = app.unique_form_mut();
        let mut max_en = form.max_volume_enabled;
        ui.checkbox(&mut max_en, "Limit volume size (max-volume-bytes)");
        form.max_volume_enabled = max_en;
        if max_en {
            ui.horizontal(|ui| {
                ui.label("Max bytes:");
                ui.text_edit_singleline(&mut form.max_volume_text);
            });
        }
        ui.checkbox(&mut form.no_tier2, "Disable Tier 2 (no_tier2)");
        ui.checkbox(
            &mut form.no_attachments,
            "No attachments (parents only write)",
        );
        ui.checkbox(&mut form.overwrite, "Overwrite existing out / report dir");
    }

    // Overwrite confirm modal
    if app.unique_form().confirm_overwrite {
        egui::Window::new("Confirm overwrite")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label("Output PST and/or report directory already exist.");
                ui.label("Overwrite existing files?");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        app.unique_form_mut().confirm_overwrite = false;
                    }
                    if ui.button("Overwrite and Run").clicked() {
                        app.unique_form_mut().overwrite = true;
                        app.unique_form_mut().confirm_overwrite = false;
                        app.start_unique_pst(ui.ctx().clone());
                    }
                });
            });
    }

    if let Some(err) = app.unique_form_error() {
        ui.add_space(8.0);
        ui.colored_label(egui::Color32::RED, err);
    }

    ui.add_space(16.0);
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("← Back").clicked() {
            app.set_state(AppState::UniqueSelect);
        }
        ui.add_space(12.0);
        let can = app.unique_form().can_run();
        ui.add_enabled_ui(can, |ui| {
            if ui.button("▶  Run Unique PST Export").clicked() {
                if app.unique_form().needs_overwrite_confirm() {
                    app.unique_form_mut().confirm_overwrite = true;
                } else {
                    app.start_unique_pst(ui.ctx().clone());
                }
            }
        });
        if !can {
            let hint = app
                .unique_form()
                .validate_for_run()
                .err()
                .unwrap_or_else(|| "Set at least one input and an output .pst path.".into());
            ui.colored_label(egui::Color32::GRAY, hint);
        }
    });
}

pub fn show_running(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    let progress = app
        .unique_progress()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    ui.add_space(12.0);
    ui.heading("Unique PST Export — Running");
    ui.add_space(8.0);

    ui.label(format!("Stage: {}", progress.stage));
    if progress.volume_index > 0 {
        ui.label(format!("Volume: {}", progress.volume_index));
    }
    ui.label(format!(
        "Messages written (this volume): {}",
        progress.messages_written
    ));
    ui.label(format!(
        "Messages written (cumulative): {}",
        progress.messages_written_cumulative
    ));
    if let Some(w) = progress.winners_total {
        ui.label(format!("Winners total: {w}"));
    }
    ui.label(format!(
        "Physical size: {}",
        dedup_engine::format_bytes(progress.physical_bytes)
    ));

    ui.add_space(8.0);
    // Prefer cumulative / winners_total so multi-volume runs do not reset the bar.
    let fraction = match progress.winners_total {
        Some(w) if w > 0 => (progress.messages_written_cumulative as f32 / w as f32).min(1.0),
        _ => {
            if progress.complete {
                1.0
            } else {
                0.15
            }
        }
    };
    let bar_text = if progress.winners_total.is_some() {
        format!(
            "{} — {} / {}",
            progress.stage,
            progress.messages_written_cumulative,
            progress.winners_total.unwrap_or(0)
        )
    } else {
        progress.stage.clone()
    };
    ui.add(
        egui::ProgressBar::new(fraction)
            .text(bar_text)
            .animate(!progress.complete),
    );

    ui.add_space(12.0);
    if !progress.complete && ui.button("Cancel").clicked() {
        app.cancel_unique_pst();
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new("Log / Details")
        .default_open(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if progress.log_lines.is_empty() {
                        ui.colored_label(egui::Color32::GRAY, "(no log lines yet)");
                    } else {
                        for line in &progress.log_lines {
                            ui.monospace(line);
                        }
                    }
                });
        });
}

pub fn show_done(ui: &mut egui::Ui, app: &mut PstDedupApp) {
    let progress = app
        .unique_progress()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let outcome = progress.outcome;

    ui.add_space(12.0);
    ui.heading("Unique PST Export — Done");
    ui.add_space(8.0);

    match &outcome {
        Some(o) if o.cancelled => {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Cancelled (partial report may be available).",
            );
        }
        Some(o) if o.ok => {
            ui.colored_label(egui::Color32::GREEN, "Export completed successfully.");
        }
        Some(o) => {
            ui.colored_label(
                egui::Color32::from_rgb(220, 120, 50),
                format!(
                    "Export finished with errors: {}",
                    o.error_message.as_deref().unwrap_or("see report")
                ),
            );
        }
        None => {
            ui.colored_label(
                egui::Color32::RED,
                progress.error.as_deref().unwrap_or("No outcome available."),
            );
        }
    }

    if let Some(o) = &outcome {
        ui.add_space(8.0);
        egui::Grid::new("unique_done_stats")
            .num_columns(2)
            .spacing([40.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Messages written:");
                ui.label(format!("{}", o.messages_written_total));
                ui.end_row();
                ui.strong("Unique (keep-set):");
                ui.label(format!("{}", o.unique));
                ui.end_row();
                ui.strong("Volumes:");
                ui.label(format!("{}", o.volume_count));
                ui.end_row();
                ui.strong("Output:");
                ui.label(o.out.display().to_string());
                ui.end_row();
                ui.strong("Report dir:");
                ui.label(o.report_dir.display().to_string());
                ui.end_row();
                ui.strong("Summary:");
                ui.label(o.summary_path.display().to_string());
                ui.end_row();
            });

        if !o.volumes.is_empty() {
            ui.add_space(12.0);
            ui.strong("Volume digests");
            ui.add_space(4.0);
            // Full digests must be visible without opening report files (DoD-7).
            egui::ScrollArea::both().max_height(220.0).show(ui, |ui| {
                egui::Grid::new("unique_volume_table")
                    .num_columns(4)
                    .striped(true)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("#");
                        ui.strong("Path");
                        ui.strong("Bytes");
                        ui.strong("Messages");
                        ui.end_row();
                        for v in &o.volumes {
                            ui.label(format!("{}", v.volume_index));
                            ui.label(&v.path);
                            ui.label(dedup_engine::format_bytes(v.bytes));
                            ui.label(format!("{}", v.messages_written));
                            ui.end_row();

                            ui.label("");
                            ui.vertical(|ui| {
                                ui.label("SHA-256");
                                ui.monospace(egui::RichText::new(&v.sha256_hex).small());
                                ui.label("MD5");
                                ui.monospace(
                                    egui::RichText::new(if v.md5_hex.is_empty() {
                                        "—"
                                    } else {
                                        v.md5_hex.as_str()
                                    })
                                    .small(),
                                );
                            });
                            ui.label("");
                            ui.label("");
                            ui.end_row();
                        }
                    });
            });
        }
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new("Log / Details")
        .default_open(false)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for line in &progress.log_lines {
                        ui.monospace(line);
                    }
                });
        });

    ui.add_space(16.0);
    ui.separator();
    ui.horizontal(|ui| {
        if let Some(o) = &outcome {
            // Only offer Open report when summary.json exists (honest cancelled summary
            // is written on pre-scan cancel after prepare_report_dir).
            let summary_ok = o.summary_path.is_file();
            if !o.report_dir.as_os_str().is_empty()
                && summary_ok
                && ui.button("Open report folder").clicked()
            {
                open_folder_nonblocking(&o.report_dir);
            }
            if !o.out.as_os_str().is_empty() && ui.button("Open output folder").clicked() {
                open_folder_nonblocking(&o.out);
            }
        }
        ui.add_space(16.0);
        if ui.button("Back to file select").clicked() {
            app.leave_unique_wizard();
        }
        if ui.button("Export another…").clicked() {
            let inputs = app.unique_form().inputs.clone();
            *app.unique_form_mut() = UniqueWizardForm::with_inputs(inputs);
            app.set_state(AppState::UniqueSelect);
        }
    });
}
