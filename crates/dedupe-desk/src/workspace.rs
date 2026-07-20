//! Workspace panels: sources, PST inventory, jobs, stats, process actions.
//! Case Overview (track 0038) KPIs + rollup tables.

use eframe::egui;
use matter_core::CaseOverview;

use crate::app::DeskApp;
use crate::matter_ui::{
    format_bytes, overview_category_label, overview_custodian_label, MatterSnapshot,
};
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

        // Processing profile (track 0043): select, apply defaults, save-as, run.
        {
            let user_profiles: Vec<(String, String)> = app
                .snapshot
                .processing_profiles
                .iter()
                .filter(|p| !p.is_builtin)
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect();
            let selected_label =
                params::profile_selection_label(&app.selected_profile_id, &user_profiles);
            egui::ComboBox::from_id_salt("processing_profile")
                .selected_text(selected_label)
                .width(160.0)
                .show_ui(ui, |ui| {
                    ui.label(egui::RichText::new("Built-in").strong().small());
                    for name in params::PROFILE_BUILTIN_NAMES {
                        let id = format!("builtin:{name}");
                        ui.selectable_value(&mut app.selected_profile_id, id, *name);
                    }
                    if !user_profiles.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("User").strong().small());
                        for (id, name) in &user_profiles {
                            ui.selectable_value(
                                &mut app.selected_profile_id,
                                id.clone(),
                                name.as_str(),
                            );
                        }
                    }
                });
            if ui
                .add_enabled(!busy, egui::Button::new("Apply defaults"))
                .on_hover_text(
                    "Seed workspace cull/promote/OCR toggles from the selected profile \
                     (does not start a job)",
                )
                .clicked()
            {
                app.apply_profile_defaults();
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Run profile"))
                .on_hover_text(
                    "Sequential profile_run: child jobs per enabled stage in canonical order \
                     (classify→extract→ocr→fts→dedupe→thread→neardup→cull→promote). \
                     Built-ins use cumulative reset:false.",
                )
                .clicked()
            {
                app.start_profile_run();
            }
            ui.add(
                egui::TextEdit::singleline(&mut app.profile_save_as_name)
                    .desired_width(100.0)
                    .hint_text("Save as name"),
            );
            if ui
                .add_enabled(!busy, egui::Button::new("Save as…"))
                .on_hover_text("Clone selected profile + current cull/promote/OCR into a user profile")
                .clicked()
            {
                app.save_profile_as();
            }
        }

        ui.separator();

        // Workflow (track 0044): select, optional run_params, run.
        {
            let user_workflows: Vec<(String, String)> = app
                .snapshot
                .workflows
                .iter()
                .filter(|w| !w.is_builtin)
                .map(|w| (w.id.clone(), w.name.clone()))
                .collect();
            let selected_label =
                params::workflow_selection_label(&app.selected_workflow_id, &user_workflows);
            egui::ComboBox::from_id_salt("workflow_select")
                .selected_text(selected_label)
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.label(egui::RichText::new("Built-in").strong().small());
                    for name in params::WORKFLOW_BUILTIN_NAMES {
                        let id = format!("builtin:{name}");
                        ui.selectable_value(&mut app.selected_workflow_id, id, *name);
                    }
                    if !user_workflows.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("User").strong().small());
                        for (id, name) in &user_workflows {
                            ui.selectable_value(
                                &mut app.selected_workflow_id,
                                id.clone(),
                                name.as_str(),
                            );
                        }
                    }
                });
            if ui
                .add_enabled(!busy, egui::Button::new("Run workflow"))
                .on_hover_text(
                    "Sequential workflow_run: child jobs per node (job / profile_run / gate). \
                     AST-only param binding (${key}); hard gates ignore soft_fail. \
                     Fill path / source_id / pst_item_id for ingest/extract built-ins.",
                )
                .clicked()
            {
                app.start_workflow_run();
            }
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
        {
            let ocr_ok = app.ocr_run_enabled();
            let tip = app.ocr_run_tooltip();
            let ocr_btn = ui
                .add_enabled(!busy && ocr_ok, egui::Button::new("Run OCR"))
                .on_hover_text(tip);
            if ocr_btn.clicked() {
                app.start_ocr();
            }
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
            .add_enabled(!busy, egui::Button::new("Classify file types"))
            .on_hover_text(
                "Assign taxonomy_v1 file categories from path/mime/magic (kind=classify); retires bare attachment category",
            )
            .clicked()
        {
            app.start_classify();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Run entity scan"))
            .on_hover_text(
                "Offline PII/entity packs (email, phone, SSN, card, $) — mask+hash only (kind=entity_scan)",
            )
            .clicked()
        {
            app.start_entity_scan();
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Build people graph"))
            .on_hover_text(
                "Two-pass people–comms graph from headers (kind=people_graph); BCC separate; no self-loop edges",
            )
            .clicked()
        {
            app.start_people_graph();
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
        if ui
            .add_enabled(!busy, egui::Button::new("Produce review set…"))
            .on_hover_text(
                "Export in_review items as NATIVES + TEXT + Concordance DAT/CSV. \
                 Withheld items are skipped (or fail-closed). Family expand is off \
                 by default — broken-family QC is track 0041.",
            )
            .clicked()
        {
            app.open_produce_dialog();
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
        if ui
            .add_enabled(!app.overview_loading, egui::Button::new("Refresh overview"))
            .on_hover_text("Reload case overview KPIs (background SQL; concurrent readers)")
            .clicked()
        {
            app.request_overview_refresh();
        }
    });

    // Workflow details + optional AST bind fields (track 0044).
    ui.horizontal(|ui| {
        let desc = app
            .snapshot
            .workflows
            .iter()
            .find(|w| w.id == app.selected_workflow_id)
            .and_then(|w| w.description.as_deref())
            .unwrap_or("");
        if !desc.is_empty() {
            ui.label(egui::RichText::new(desc).small().weak());
        } else {
            ui.label(
                egui::RichText::new(format!("Workflow: {}", app.selected_workflow_id))
                    .small()
                    .weak(),
            );
        }
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Workflow params").small().strong());
        ui.add(
            egui::TextEdit::singleline(&mut app.workflow_source_path)
                .desired_width(180.0)
                .hint_text("source_path"),
        );
        ui.add(
            egui::TextEdit::singleline(&mut app.workflow_source_id)
                .desired_width(120.0)
                .hint_text("source_id"),
        );
        ui.add(
            egui::TextEdit::singleline(&mut app.workflow_pst_item_id)
                .desired_width(120.0)
                .hint_text("pst_item_id"),
        );
        if ui
            .small_button("Use selected PST")
            .on_hover_text("Copy source_id + pst_item_id from the inventory selection")
            .clicked()
        {
            if let Some(sel) = app.selected_pst.as_ref() {
                if let Some(pst) = app.snapshot.psts.iter().find(|p| p.item_id == *sel) {
                    app.workflow_pst_item_id = pst.item_id.clone();
                    app.workflow_source_id = pst.source_id.clone();
                }
            }
        }
    });

    if app.dialog.is_open() {
        ui.label("File dialog open… (buttons disabled until it returns)");
    }

    ui.add_space(8.0);
    show_overview(ui, app);
    ui.add_space(6.0);
    show_stats(ui, &app.snapshot);
    ui.add_space(6.0);
    show_sources(ui, &app.snapshot);
    ui.add_space(6.0);
    show_psts(ui, app);
    ui.add_space(6.0);
    show_jobs(ui, &app.snapshot);
}

fn show_overview(ui: &mut egui::Ui, app: &mut DeskApp) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.heading("Overview");
            if app.overview_loading {
                ui.label(egui::RichText::new("Loading…").italics().weak());
            }
            let export_enabled = !app.report_export_busy && app.matter_root.is_some();
            if ui
                .add_enabled(export_enabled, egui::Button::new("Export matter report…"))
                .on_hover_text(
                    "Write CSV progress/metrics pack (summary + rollups + jobs) under \
                     exports/reports/ or a chosen folder. No subjects/bodies. PDF deferred.",
                )
                .clicked()
            {
                let ctx = ui.ctx().clone();
                app.spawn_report_export(&ctx);
            }
            if app.report_export_busy {
                let busy = app
                    .report_export_status
                    .as_deref()
                    .unwrap_or("Exporting report…");
                ui.label(egui::RichText::new(busy).italics().weak());
            }
        });

        // Success path only (busy status is shown next to the button above).
        if !app.report_export_busy {
            if let Some(st) = app.report_export_status.clone() {
                ui.label(
                    egui::RichText::new(st)
                        .small()
                        .color(egui::Color32::DARK_GREEN),
                );
            }
        }
        if let Some(err) = app.report_export_error.clone() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                format!("Report export: {err}"),
            );
        }

        let Some(ov) = app.case_overview.as_ref() else {
            if app.overview_loading {
                ui.label("Loading case overview…");
            } else {
                ui.label(
                    "No overview yet. Click Refresh overview (or Refresh) after opening a matter.",
                );
            }
            return;
        };

        if ov.totals.items_total == 0 {
            ui.label("No items yet.");
        }

        // KPI row
        ui.horizontal_wrapped(|ui| {
            kpi_card(ui, "Items", &ov.totals.items_total.to_string(), None);
            kpi_card(
                ui,
                "Top-level size",
                &format_bytes(ov.totals.size_bytes_top_level),
                Some(
                    "Sum of size_bytes for standalone + parent items only \
                     (role IS NULL or role ≠ attachment). Excludes attachment \
                     rows so PST/parent + children are not double-counted.",
                ),
            );
            let review_label = if ov.review.in_review == 0 {
                "0 / 0".to_string()
            } else {
                format!(
                    "{} / {} ({} unreviewed)",
                    ov.review.reviewed_count, ov.review.in_review, ov.review.unreviewed_count
                )
            };
            kpi_card(
                ui,
                "Review progress",
                &review_label,
                Some("Reviewed = in-review items with ≥1 code applied."),
            );
            kpi_card(
                ui,
                "Errors",
                &ov.errors.total.to_string(),
                Some("Matter-scoped item_errors rows. See Errors by code table."),
            );
            kpi_card(ui, "Needs OCR", &ov.ocr.pdf_needs_ocr.to_string(), None);
            kpi_card(
                ui,
                "Withhold",
                &ov.privilege.withhold.to_string(),
                Some(
                    "Items with privilege_withhold = 1 or an item_privilege withhold flag (union).",
                ),
            );
        });

        ui.add_space(4.0);
        ui.label(format!(
            "Top-level items: {} · Sources: {} · Parents: {} · Generated: {}",
            ov.totals.top_level_items,
            ov.totals.sources_total,
            ov.totals.families_total,
            ov.generated_at
        ));

        ui.add_space(6.0);
        ui.columns(3, |cols| {
            label_count_table(
                &mut cols[0],
                "File categories",
                &ov.by_file_category,
                overview_category_label,
                ov.other_categories_count,
            );
            label_count_table(
                &mut cols[1],
                "Custodians",
                &ov.by_custodian,
                overview_custodian_label,
                ov.other_custodians_count,
            );
            label_count_table(
                &mut cols[2],
                "By status",
                &ov.by_status,
                |s| if s.is_empty() { "(none)" } else { s },
                0,
            );
        });

        ui.add_space(4.0);
        ui.columns(3, |cols| {
            show_dedup_cull(&mut cols[0], ov);
            label_count_table(
                &mut cols[1],
                "Errors by code",
                &ov.errors.by_code,
                |s| if s.is_empty() { "(none)" } else { s },
                ov.errors.other_codes_count,
            );
            show_overview_jobs(&mut cols[2], ov);
        });
    });
}

fn kpi_card(ui: &mut egui::Ui, title: &str, value: &str, tooltip: Option<&str>) {
    ui.group(|ui| {
        ui.set_min_width(110.0);
        ui.label(egui::RichText::new(title).small().strong());
        let resp = ui.label(egui::RichText::new(value).heading());
        if let Some(tip) = tooltip {
            resp.on_hover_text(tip);
        }
    });
}

fn label_count_table(
    ui: &mut egui::Ui,
    title: &str,
    rows: &[matter_core::LabelCount],
    label_fn: fn(&str) -> &str,
    other: u64,
) {
    ui.group(|ui| {
        ui.label(egui::RichText::new(title).strong());
        if rows.is_empty() {
            ui.label(egui::RichText::new("—").weak());
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(format!("ov_{title}"))
            .max_height(120.0)
            .show(ui, |ui| {
                egui::Grid::new(format!("ov_grid_{title}"))
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Label");
                        ui.strong("Count");
                        ui.end_row();
                        for r in rows {
                            ui.label(label_fn(&r.label));
                            ui.label(r.count.to_string());
                            ui.end_row();
                        }
                        if other > 0 {
                            ui.label("(other)");
                            ui.label(other.to_string());
                            ui.end_row();
                        }
                    });
            });
    });
}

fn show_dedup_cull(ui: &mut egui::Ui, ov: &CaseOverview) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("Dedup / Cull").strong());
        ui.label(format!(
            "Dedupe: unique={}  duplicate={}  skipped={}  unset={}",
            ov.dedup.unique, ov.dedup.duplicate, ov.dedup.skipped, ov.dedup.null_role
        ));
        if ov.cull.never_run {
            ui.label("Cull: never run");
        } else {
            ui.label(format!(
                "Cull: included={}  culled={}  other={}",
                ov.cull.included, ov.cull.culled, ov.cull.other
            ));
        }
        ui.label(format!(
            "Privilege claimed (active): {} · Has text: {} · Has native: {}",
            ov.privilege.claimed, ov.ocr.has_text, ov.ocr.has_native
        ));
    });
}

fn show_overview_jobs(ui: &mut egui::Ui, ov: &CaseOverview) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("Jobs").strong());
        ui.label(format!(
            "pending={} running={} paused={} failed={} cancelled={} succeeded={}",
            ov.jobs.pending,
            ov.jobs.running,
            ov.jobs.paused,
            ov.jobs.failed,
            ov.jobs.cancelled,
            ov.jobs.succeeded
        ));
        if ov.jobs.recent.is_empty() {
            ui.label(egui::RichText::new("No recent jobs.").weak());
            return;
        }
        for j in &ov.jobs.recent {
            let done = j
                .completed_count
                .map(|c| format!(" · done={c}"))
                .unwrap_or_default();
            ui.label(format!("{} [{}]{done}", j.kind, j.state));
        }
    });
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
                    .num_columns(7)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Kind");
                        ui.strong("State");
                        ui.strong("Id");
                        ui.strong("Parent");
                        ui.strong("Started");
                        ui.strong("Finished");
                        ui.strong("Error");
                        ui.end_row();
                        for j in snap.jobs.iter().rev() {
                            // Indent child jobs under orchestrators (workflow_run / profile_run).
                            let kind_label = if j.parent_job_id.is_some() {
                                format!("  └ {}", j.kind)
                            } else {
                                j.kind.clone()
                            };
                            ui.label(kind_label);
                            ui.label(&j.state);
                            ui.monospace(short(&j.id));
                            if let Some(ref pid) = j.parent_job_id {
                                ui.monospace(format!("parent: {}", short(pid)));
                            } else {
                                ui.label("—");
                            }
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
