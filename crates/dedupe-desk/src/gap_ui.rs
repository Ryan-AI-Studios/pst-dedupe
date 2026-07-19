//! Gap analysis panel state + light matter reads/writes (track 0042).

use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::Utf8Path;
use eframe::egui;
use matter_core::{ExpectedCustodian, Matter};
use matter_gap::{import_opposing_dat, DatCaps};

/// UI intent to start a gap job (replaces magic `__start_*__` status strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingGapStart {
    Collection,
    Opposing,
}

/// Desk-side gap panel state.
#[derive(Default)]
pub struct GapState {
    pub roster: Vec<ExpectedCustodian>,
    pub roster_error: Option<String>,
    pub window_start: String,
    pub window_end: String,
    pub flag_unexpected: bool,
    pub last_report_path: Option<String>,
    pub last_status: Option<String>,
    /// Set by the panel buttons; consumed by the app frame loop.
    pub pending_start: Option<PendingGapStart>,
    pub last_import_id: Option<String>,
    pub imports: Vec<(String, String, u64)>, // id, path, row_count
    pub busy: bool,
    op_rx: Option<Receiver<GapOpResult>>,
}

enum GapOpResult {
    RosterLoaded {
        roster: Vec<ExpectedCustodian>,
        imports: Vec<(String, String, u64)>,
        last_report: Option<String>,
    },
    RosterImported {
        message: String,
        roster: Vec<ExpectedCustodian>,
    },
    OpposingImported {
        import_id: String,
        row_count: u64,
        path: String,
    },
    Error(String),
}

impl GapState {
    pub fn new() -> Self {
        Self {
            flag_unexpected: true,
            ..Default::default()
        }
    }

    pub fn request_reload(&mut self, matter_root: &Utf8Path) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.roster_error = None;
        let root = matter_root.to_path_buf();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<GapOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let roster = matter
                    .list_expected_custodians(true)
                    .map_err(|e| e.to_string())?;
                let imports = matter
                    .list_gap_imports()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|i| (i.id, i.path, i.row_count))
                    .collect();
                let last_report = matter
                    .load_latest_gap_run()
                    .map_err(|e| e.to_string())?
                    .and_then(|r| r.report_path);
                Ok(GapOpResult::RosterLoaded {
                    roster,
                    imports,
                    last_report,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(GapOpResult::Error));
        });
    }

    pub fn import_roster_csv(&mut self, matter_root: &Utf8Path, path: &Path) {
        if self.busy {
            return;
        }
        self.busy = true;
        let root = matter_root.to_path_buf();
        let path = path.to_path_buf();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<GapOpResult, String> {
                let matter = Matter::open(&root).map_err(|e| e.to_string())?;
                let r = matter_gap::import_roster_csv(&matter, &path).map_err(|e| e.to_string())?;
                let roster = matter
                    .list_expected_custodians(true)
                    .map_err(|e| e.to_string())?;
                Ok(GapOpResult::RosterImported {
                    message: format!(
                        "Roster: inserted={} updated={} rows={}",
                        r.inserted, r.updated, r.total_rows
                    ),
                    roster,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(GapOpResult::Error));
        });
    }

    pub fn import_opposing(&mut self, matter_root: &Utf8Path, path: &Path) {
        if self.busy {
            return;
        }
        self.busy = true;
        let root = matter_root.to_path_buf();
        let path = path.to_path_buf();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<GapOpResult, String> {
                let matter = Matter::open(&root).map_err(|e| e.to_string())?;
                let import_id = import_opposing_dat(&matter, &path, None, DatCaps::default())
                    .map_err(|e| e.to_string())?;
                let docs = matter
                    .list_gap_expected_docs(&import_id)
                    .map_err(|e| e.to_string())?;
                Ok(GapOpResult::OpposingImported {
                    import_id,
                    row_count: docs.len() as u64,
                    path: path.display().to_string(),
                })
            })();
            let _ = tx.send(result.unwrap_or_else(GapOpResult::Error));
        });
    }

    pub fn poll(&mut self) {
        let Some(rx) = self.op_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(GapOpResult::RosterLoaded {
                roster,
                imports,
                last_report,
            }) => {
                self.roster = roster;
                self.imports = imports;
                if last_report.is_some() {
                    self.last_report_path = last_report;
                }
                self.busy = false;
                self.op_rx = None;
            }
            Ok(GapOpResult::RosterImported { message, roster }) => {
                self.roster = roster;
                self.last_status = Some(message);
                self.busy = false;
                self.op_rx = None;
            }
            Ok(GapOpResult::OpposingImported {
                import_id,
                row_count,
                path,
            }) => {
                self.last_import_id = Some(import_id.clone());
                self.imports.insert(0, (import_id, path, row_count));
                self.last_status = Some(format!("Imported opposing DAT: {row_count} rows"));
                self.busy = false;
                self.op_rx = None;
            }
            Ok(GapOpResult::Error(e)) => {
                self.roster_error = Some(e);
                self.busy = false;
                self.op_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.op_rx = None;
            }
        }
    }
}

/// Draw the Gap analysis screen.
pub fn show(
    ui: &mut egui::Ui,
    gap: &mut GapState,
    matter_root: Option<&Utf8Path>,
    runner_busy: bool,
) {
    ui.heading("Gap analysis");
    ui.label(
        "Expected custodians vs inventory, optional date window, and opposing production \
         DAT set-diff (track 0042).",
    );
    ui.add_space(8.0);

    let Some(root) = matter_root else {
        ui.label("Open a matter to run gap analysis.");
        return;
    };

    if gap.roster.is_empty() && !gap.busy && gap.roster_error.is_none() {
        // Lazy first load
        gap.request_reload(root);
    }

    if let Some(err) = &gap.roster_error {
        ui.colored_label(egui::Color32::from_rgb(180, 50, 50), err);
    }
    if let Some(st) = &gap.last_status {
        ui.label(egui::RichText::new(st).weak());
    }

    ui.group(|ui| {
        ui.label(egui::RichText::new("Expected custodians").strong());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !gap.busy && !runner_busy,
                    egui::Button::new("Import roster CSV…"),
                )
                .on_hover_text("UTF-8 CSV with header 'custodian' (optional notes, alias)")
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .pick_file()
                {
                    gap.import_roster_csv(root, &path);
                }
            }
            if ui
                .add_enabled(!gap.busy, egui::Button::new("Refresh roster"))
                .clicked()
            {
                gap.request_reload(root);
            }
        });
        if gap.busy {
            ui.spinner();
            ui.label("Working…");
        }
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .show(ui, |ui| {
                if gap.roster.is_empty() {
                    ui.label(egui::RichText::new("(no expected custodians)").weak());
                } else {
                    for c in &gap.roster {
                        ui.label(format!("• {} ({})", c.display_name, c.name_norm));
                    }
                }
            });
    });

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(egui::RichText::new("Collection gap").strong());
        ui.horizontal(|ui| {
            ui.label("Window start (UTC):");
            ui.add(
                egui::TextEdit::singleline(&mut gap.window_start)
                    .desired_width(180.0)
                    .hint_text("YYYY-MM-DD or RFC3339"),
            );
            ui.label("end:");
            ui.add(
                egui::TextEdit::singleline(&mut gap.window_end)
                    .desired_width(180.0)
                    .hint_text("optional"),
            );
        });
        ui.checkbox(
            &mut gap.flag_unexpected,
            "Flag unexpected custodians (warn)",
        );
        if ui
            .add_enabled(
                !runner_busy && !gap.busy,
                egui::Button::new("Run collection gap"),
            )
            .on_hover_text("Job kind gap — missing_custodian is warn severity")
            .clicked()
        {
            gap.pending_start = Some(PendingGapStart::Collection);
        }
    });

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(egui::RichText::new("Opposing production").strong());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !gap.busy && !runner_busy,
                    egui::Button::new("Import opposing DAT…"),
                )
                .on_hover_text("0040 Concordance DAT or mapped CSV (metadata only)")
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("DAT / CSV", &["dat", "csv", "txt"])
                    .pick_file()
                {
                    gap.import_opposing(root, &path);
                }
            }
            if ui
                .add_enabled(
                    !runner_busy && !gap.busy && gap.last_import_id.is_some(),
                    egui::Button::new("Run opposing compare"),
                )
                .clicked()
            {
                gap.pending_start = Some(PendingGapStart::Opposing);
            }
        });
        if let Some(id) = &gap.last_import_id {
            ui.label(format!("Last import: {id}"));
        }
        egui::ScrollArea::vertical()
            .max_height(100.0)
            .show(ui, |ui| {
                for (id, path, rows) in gap.imports.iter().take(8) {
                    ui.label(format!("{id}  rows={rows}  {path}"));
                }
            });
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Last report:").strong());
        match &gap.last_report_path {
            Some(p) => {
                ui.label(p);
                if ui.button("Open folder").clicked() {
                    open_parent_folder(p);
                }
            }
            None => {
                ui.label(egui::RichText::new("none").weak());
            }
        }
    });
}

fn open_parent_folder(path: &str) {
    let p = Path::new(path);
    let dir = if p.is_dir() {
        p
    } else {
        p.parent().unwrap_or(p)
    };
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
    }
}

/// Build collection gap job params from panel fields.
pub fn collection_params_from_state(gap: &GapState) -> String {
    crate::params::gap_collection_params(
        gap.window_start.trim(),
        gap.window_end.trim(),
        gap.flag_unexpected,
    )
}

/// Build opposing gap job params.
pub fn opposing_params_from_state(gap: &GapState) -> Option<String> {
    gap.last_import_id
        .as_ref()
        .map(|id| crate::params::gap_opposing_params(id, "inventory"))
}

/// True when the panel requested a collection run (consumes the marker).
pub fn take_start_collection(gap: &mut GapState) -> bool {
    if gap.pending_start == Some(PendingGapStart::Collection) {
        gap.pending_start = None;
        true
    } else {
        false
    }
}

/// True when the panel requested an opposing run (consumes the marker).
pub fn take_start_opposing(gap: &mut GapState) -> bool {
    if gap.pending_start == Some(PendingGapStart::Opposing) {
        gap.pending_start = None;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state() {
        let g = GapState::new();
        assert!(g.flag_unexpected);
        assert!(g.roster.is_empty());
        assert!(g.pending_start.is_none());
    }

    #[test]
    fn pending_start_collection_consumed() {
        let mut g = GapState::new();
        g.pending_start = Some(PendingGapStart::Collection);
        assert!(take_start_collection(&mut g));
        assert!(!take_start_collection(&mut g));
        assert!(g.pending_start.is_none());
    }

    #[test]
    fn pending_start_opposing_consumed() {
        let mut g = GapState::new();
        g.pending_start = Some(PendingGapStart::Opposing);
        assert!(take_start_opposing(&mut g));
        assert!(!take_start_opposing(&mut g));
    }
}
