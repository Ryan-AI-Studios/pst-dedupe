//! Workspace panels: sources, PST inventory, jobs, stats, process actions.

use eframe::egui;

use crate::app::DeskApp;
use crate::matter_ui::MatterSnapshot;
use crate::params;
use crate::progress_ui;

pub fn show(ui: &mut egui::Ui, app: &mut DeskApp) {
    let Some(root) = app.matter_root.clone() else {
        ui.label("Create or open a matter to begin.");
        return;
    };

    ui.heading(format!(
        "Matter: {}",
        app.matter_name.as_deref().unwrap_or(root.as_str())
    ));
    ui.label(format!("Path: {root}"));
    if !app.snapshot.matter_id.is_empty() {
        ui.monospace(format!("id: {}", short(&app.snapshot.matter_id)));
    }
    ui.add_space(6.0);

    // Live progress from watch (polled each frame).
    let snap = app.progress_rx.borrow().clone();
    progress_ui::show_progress_panel(ui, &snap);
    ui.add_space(6.0);

    // Actions row
    ui.horizontal(|ui| {
        let busy = app.runner_busy();
        let dialog = app.dialog.is_open();
        let pickers_enabled = !dialog;

        if ui
            .add_enabled(pickers_enabled && !busy, egui::Button::new("Add folder…"))
            .on_hover_text("Ingest a Purview export folder or directory tree")
            .clicked()
        {
            app.spawn_add_folder();
        }
        if ui
            .add_enabled(pickers_enabled && !busy, egui::Button::new("Add ZIP…"))
            .clicked()
        {
            app.spawn_add_zip();
        }
        if ui
            .add_enabled(pickers_enabled && !busy, egui::Button::new("Add PST…"))
            .clicked()
        {
            app.spawn_add_pst();
        }

        ui.separator();

        if ui
            .add_enabled(
                !busy && app.selected_pst.is_some(),
                egui::Button::new("Extract selected"),
            )
            .clicked()
        {
            app.start_extract_selected();
        }
        if ui
            .add_enabled(
                !busy && !app.snapshot.psts.is_empty(),
                egui::Button::new("Extract all"),
            )
            .on_hover_text("Queue sequential extract jobs (single-flight runner)")
            .clicked()
        {
            app.start_extract_all();
        }

        ui.separator();

        if ui
            .add_enabled(!busy, egui::Button::new("Run dedupe"))
            .on_hover_text("Tiered matter dedupe: Message-ID → logical_hash → family attachments")
            .clicked()
        {
            app.start_dedupe();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Run threading"))
            .on_hover_text(
                "Email threading: Message-ID graph → subject → ConversationIndex → family inherit",
            )
            .clicked()
        {
            app.start_thread();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Run near-dup"))
            .on_hover_text(
                "Near-duplicate detection: MinHash shingles + LSH clusters (pivot/member roles)",
            )
            .clicked()
        {
            app.start_neardup();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Extract Office text"))
            .on_hover_text(
                "Extract plain text from DOCX/XLSX/PPTX natives in CAS (kind=office_extract)",
            )
            .clicked()
        {
            app.start_office_extract();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Extract PDF text"))
            .on_hover_text(
                "Extract embedded text from PDF natives in CAS (kind=pdf_extract); low-text → needs OCR",
            )
            .clicked()
        {
            app.start_pdf_extract();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Extract ICS"))
            .on_hover_text(
                "Parse ICS/calendar natives in CAS into calendar items (kind=ics_extract); multi-event → archive + single-event children",
            )
            .clicked()
        {
            app.start_ics_extract();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Build / Update search index"))
            .on_hover_text("Incremental Tantivy FTS index (kind=fts_index, reset:false)")
            .clicked()
        {
            app.start_fts_index();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Rebuild search index"))
            .on_hover_text(
                "Full FTS rebuild: drop handles, remove index/, clear fts_*, re-index (reset:true)",
            )
            .clicked()
        {
            app.start_fts_rebuild();
        }

        ui.separator();

        // Cull preset pick + Run cull (flag-only data reduction).
        // Clone user presets so the combo can mutably borrow `cull_preset`.
        let user_cull_presets: Vec<(String, String)> = app
            .snapshot
            .cull_presets
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let cull_selected_text = app.cull_preset_display_name();
        egui::ComboBox::from_id_salt("cull_preset")
            .selected_text(cull_selected_text)
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.label(egui::RichText::new("Built-in").strong().small());
                for name in params::CULL_BUILTIN_PRESETS {
                    ui.selectable_value(&mut app.cull_preset, (*name).to_string(), *name);
                }
                if !user_cull_presets.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("User presets").strong().small());
                    for (id, name) in &user_cull_presets {
                        let value = format!("{}{}", params::CULL_USER_PRESET_PREFIX, id);
                        ui.selectable_value(&mut app.cull_preset, value, name);
                    }
                }
            });
        if ui
            .add_enabled(!busy, egui::Button::new("Run cull"))
            .on_hover_text(
                "Flag-only data reduction: built-in or matter-saved user preset \
                 (included vs culled + reasons)",
            )
            .clicked()
        {
            app.start_cull();
        }

        ui.separator();

        // Promote policy pick + Promote to review (flag-only membership).
        egui::ComboBox::from_id_salt("promote_policy")
            .selected_text(app.promote_policy.as_str())
            .width(180.0)
            .show_ui(ui, |ui| {
                for name in params::PROMOTE_POLICIES {
                    ui.selectable_value(&mut app.promote_policy, (*name).to_string(), *name);
                }
            });
        if ui
            .add_enabled(!busy, egui::Button::new("Promote to review"))
            .on_hover_text(
                "Build review corpus membership (in_review + review_order). \
                 policy=auto → cull_included if cull has run, else unique_only. \
                 Bidirectional family expand on by default.",
            )
            .clicked()
        {
            app.start_promote();
        }
        if ui
            .button("Open Review")
            .on_hover_text("Open the Review corpus list and body viewer")
            .clicked()
        {
            app.open_review();
        }

        ui.separator();

        let job_id = snap.job_id.clone();
        let can_cancel = snap.state == "running" && !job_id.is_empty();
        if ui
            .add_enabled(can_cancel, egui::Button::new("Cancel"))
            .clicked()
        {
            app.cancel_active();
        }
        let can_resume = app.can_resume();
        if ui
            .add_enabled(can_resume, egui::Button::new("Resume"))
            .on_hover_text("Resume paused/failed job, or leftover Running after crash")
            .clicked()
        {
            app.resume_active();
        }

        if ui.button("Refresh").clicked() {
            app.refresh_matter_lists();
        }
    });

    if app.dialog.is_open() {
        ui.label("File dialog open… (buttons disabled until it returns)");
    }

    ui.add_space(8.0);
    show_stats(ui, &app.snapshot);
    ui.add_space(6.0);
    show_sources(ui, &app.snapshot);
    ui.add_space(6.0);
    show_psts(ui, app);
    ui.add_space(6.0);
    show_jobs(ui, &app.snapshot);
}

fn show_stats(ui: &mut egui::Ui, snap: &MatterSnapshot) {
    ui.group(|ui| {
        ui.heading("Counts");
        ui.label(format!("Sources: {}", snap.sources.len()));
        ui.label(format!("Discovered PSTs: {}", snap.psts.len()));
        ui.label(format!("Items (all): {}", snap.item_count));
        ui.label(format!("Jobs: {}", snap.jobs.len()));
        ui.label(format!(
            "Dedupe: unique={}  duplicate={}",
            snap.dedup_unique, snap.dedup_duplicate
        ));
        ui.label(format!("SQLite journal_mode: {}", snap.journal_mode));
    });
}

fn show_sources(ui: &mut egui::Ui, snap: &MatterSnapshot) {
    ui.group(|ui| {
        ui.heading("Sources");
        if snap.sources.is_empty() {
            ui.label("No sources yet. Add a folder, ZIP, or PST.");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("sources_scroll")
            .max_height(140.0)
            .show(ui, |ui| {
                egui::Grid::new("sources_grid")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Kind");
                        ui.strong("Status");
                        ui.strong("Path");
                        ui.strong("Id");
                        ui.end_row();
                        for s in &snap.sources {
                            ui.label(&s.kind);
                            ui.label(&s.status);
                            ui.label(&s.path);
                            ui.monospace(short(&s.id));
                            ui.end_row();
                        }
                    });
            });
    });
}

fn show_psts(ui: &mut egui::Ui, app: &mut DeskApp) {
    ui.group(|ui| {
        ui.heading("Discovered PSTs");
        if app.snapshot.psts.is_empty() {
            ui.label("No PST inventory rows yet (run ingest first).");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("psts_scroll")
            .max_height(160.0)
            .show(ui, |ui| {
                for pst in &app.snapshot.psts {
                    let selected = app.selected_pst.as_deref() == Some(pst.item_id.as_str());
                    let size = pst
                        .size_bytes
                        .map(|b| format!("{b} B"))
                        .unwrap_or_else(|| "—".into());
                    let label = format!(
                        "{}  [{}]  {}  {}",
                        pst.path,
                        pst.status,
                        size,
                        short(&pst.item_id)
                    );
                    if ui.selectable_label(selected, label).clicked() {
                        app.selected_pst = Some(pst.item_id.clone());
                    }
                }
            });
    });
}

fn show_jobs(ui: &mut egui::Ui, snap: &MatterSnapshot) {
    ui.group(|ui| {
        ui.heading("Jobs");
        if snap.jobs.is_empty() {
            ui.label("No jobs yet.");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("jobs_scroll")
            .max_height(160.0)
            .show(ui, |ui| {
                egui::Grid::new("jobs_grid")
                    .num_columns(6)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Kind");
                        ui.strong("State");
                        ui.strong("Id");
                        ui.strong("Started");
                        ui.strong("Finished");
                        ui.strong("Error");
                        ui.end_row();
                        for j in snap.jobs.iter().rev() {
                            ui.label(&j.kind);
                            ui.label(&j.state);
                            ui.monospace(short(&j.id));
                            ui.label(j.started_at.as_deref().unwrap_or("—"));
                            ui.label(j.finished_at.as_deref().unwrap_or("—"));
                            ui.label(j.error_summary.as_deref().unwrap_or("—"));
                            ui.end_row();
                        }
                    });
            });
    });
}

fn short(id: &str) -> &str {
    if id.len() > 10 {
        &id[..10]
    } else {
        id
    }
}
