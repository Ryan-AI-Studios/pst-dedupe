//! People–comms graph panel (track 0047) — tables first, no force-directed canvas.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::Utf8Path;
use eframe::egui;
use matter_core::{
    DomainRollupRow, Matter, PeopleEdge, PeopleGraphStatus, PeopleTimelineBucket, Person,
};

/// Desk-side people panel state.
#[derive(Default)]
pub struct PeopleState {
    pub status: PeopleGraphStatus,
    pub people: Vec<Person>,
    pub edges: Vec<PeopleEdge>,
    pub domains: Vec<DomainRollupRow>,
    pub timeline: Vec<PeopleTimelineBucket>,
    pub error: Option<String>,
    pub last_status: Option<String>,
    pub busy: bool,
    /// When set, app applies person_id FilterSpec and navigates to Review.
    pub pending_filter_person_id: Option<String>,
    /// Request start of people_graph job (consumed by app).
    pub pending_start: bool,
    op_rx: Option<Receiver<PeopleOpResult>>,
}

enum PeopleOpResult {
    Loaded {
        status: PeopleGraphStatus,
        people: Vec<Person>,
        edges: Vec<PeopleEdge>,
        domains: Vec<DomainRollupRow>,
        timeline: Vec<PeopleTimelineBucket>,
    },
    Error(String),
}

impl PeopleState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_reload(&mut self, matter_root: &Utf8Path) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let root = matter_root.to_path_buf();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<PeopleOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let status = matter.people_graph_status().map_err(|e| e.to_string())?;
                let people = matter.list_people(50).map_err(|e| e.to_string())?;
                let edges = matter.list_people_edges(50).map_err(|e| e.to_string())?;
                let domains = matter.list_domain_rollup(40).map_err(|e| e.to_string())?;
                let grain = "day";
                let timeline = matter
                    .list_people_timeline(grain, None, 90)
                    .map_err(|e| e.to_string())?;
                Ok(PeopleOpResult::Loaded {
                    status,
                    people,
                    edges,
                    domains,
                    timeline,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(PeopleOpResult::Error));
        });
    }

    pub fn poll(&mut self) {
        let Some(rx) = self.op_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(PeopleOpResult::Loaded {
                status,
                people,
                edges,
                domains,
                timeline,
            }) => {
                self.status = status;
                self.people = people;
                self.edges = edges;
                self.domains = domains;
                self.timeline = timeline;
                self.busy = false;
                self.op_rx = None;
                self.error = None;
            }
            Ok(PeopleOpResult::Error(e)) => {
                self.error = Some(e);
                self.busy = false;
                self.op_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.op_rx = None;
                self.error = Some("People load thread ended unexpectedly.".into());
            }
        }
    }

    pub fn take_start(&mut self) -> bool {
        if self.pending_start {
            self.pending_start = false;
            true
        } else {
            false
        }
    }

    pub fn take_filter_person(&mut self) -> Option<String> {
        self.pending_filter_person_id.take()
    }
}

/// Default Top Pairs column headers (visible_count only — no Bcc column).
pub fn top_pairs_column_labels() -> &'static [&'static str] {
    &["From", "To", "Visible", "To/Cc"]
}

/// Edges eligible for default Top Pairs: `visible_count > 0` (excludes BCC-only).
pub fn top_pairs_visible_edges(edges: &[PeopleEdge]) -> Vec<&PeopleEdge> {
    edges.iter().filter(|e| e.visible_count > 0).collect()
}

/// Draw the People screen.
pub fn show(
    ui: &mut egui::Ui,
    people: &mut PeopleState,
    matter_root: Option<&Utf8Path>,
    busy: bool,
) {
    people.poll();

    ui.heading("People / comms graph");
    ui.label(
        "Header participants → person nodes, directed pairs (visible = To+Cc), domain rollup, \
         timeline. Offline SQLite only — not Relativity CA parity.",
    );
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !busy && !people.busy,
                egui::Button::new("Build people graph"),
            )
            .on_hover_text("Run people_graph two-pass job (reset rebuild)")
            .clicked()
        {
            people.pending_start = true;
        }
        if ui
            .add_enabled(
                matter_root.is_some() && !people.busy,
                egui::Button::new("Refresh"),
            )
            .clicked()
        {
            if let Some(root) = matter_root {
                people.request_reload(root);
            }
        }
        if people.busy {
            ui.spinner();
            ui.label("Loading…");
        }
    });

    if let Some(err) = &people.error {
        ui.colored_label(egui::Color32::from_rgb(200, 80, 80), err);
    }
    if let Some(st) = &people.last_status {
        ui.label(st);
    }

    // Incomplete warning
    if !people.status.is_complete {
        let pass = people.status.pass.as_deref().unwrap_or("(never run)");
        ui.colored_label(
            egui::Color32::from_rgb(180, 120, 40),
            format!(
                "Graph incomplete — pass={pass}. Run Build people graph; do not treat Top Pairs as final until pass=complete."
            ),
        );
    } else if let Some(at) = &people.status.built_at {
        ui.label(format!(
            "Complete · built_at={at} · people={} · edges={}",
            people.status.people_count, people.status.edge_count
        ));
    }

    ui.add_space(8.0);
    ui.separator();

    // Top people
    ui.heading("Top people");
    ui.label("message_count includes any role; as_bcc is separate (never mixed into as_to).");
    egui::ScrollArea::vertical()
        .id_salt("people_top")
        .max_height(220.0)
        .show(ui, |ui| {
            egui::Grid::new("people_grid")
                .striped(true)
                .num_columns(9)
                .show(ui, |ui| {
                    ui.strong("Key");
                    ui.strong("Kind");
                    ui.strong("Domain");
                    ui.strong("Msgs");
                    ui.strong("From");
                    ui.strong("To");
                    ui.strong("Cc");
                    ui.strong("Bcc");
                    ui.strong("Self");
                    ui.end_row();
                    for p in &people.people {
                        ui.label(&p.normalized_key);
                        ui.label(&p.identity_kind);
                        ui.label(p.email_domain.as_deref().unwrap_or("—"));
                        ui.label(p.message_count.to_string());
                        ui.label(p.as_from_count.to_string());
                        ui.label(p.as_to_count.to_string());
                        ui.label(p.as_cc_count.to_string());
                        ui.label(p.as_bcc_count.to_string());
                        ui.label(p.self_mail_count.to_string());
                        ui.end_row();
                        ui.horizontal(|ui| {
                            if ui.small_button("Filter to person").clicked() {
                                people.pending_filter_person_id = Some(p.id.clone());
                            }
                            if ui.small_button("Copy key").clicked() {
                                ui.ctx().copy_text(p.normalized_key.clone());
                            }
                        });
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    }
                });
        });

    ui.add_space(8.0);
    ui.heading("Top pairs (visible only)");
    ui.label(
        "Sorted by visible_count = to + cc. BCC-only pairs are hidden; Bcc is not shown in this table by default.",
    );
    // Column labels used by UI + unit test (Bcc intentionally omitted).
    let pair_cols = top_pairs_column_labels();
    egui::ScrollArea::vertical()
        .id_salt("people_edges")
        .max_height(180.0)
        .show(ui, |ui| {
            egui::Grid::new("edges_grid")
                .striped(true)
                .num_columns(pair_cols.len())
                .show(ui, |ui| {
                    for col in pair_cols {
                        ui.strong(*col);
                    }
                    ui.end_row();
                    for e in top_pairs_visible_edges(&people.edges) {
                        let from = e
                            .from_key
                            .as_deref()
                            .or(e.from_label.as_deref())
                            .unwrap_or(&e.from_person_id);
                        let to = e
                            .to_key
                            .as_deref()
                            .or(e.to_label.as_deref())
                            .unwrap_or(&e.to_person_id);
                        ui.label(from);
                        ui.label(to);
                        ui.label(e.visible_count.to_string());
                        ui.label(format!("{}/{}", e.to_count, e.cc_count));
                        ui.end_row();
                    }
                });
        });

    ui.add_space(8.0);
    ui.heading("Domain rollup (SMTP)");
    egui::ScrollArea::vertical()
        .id_salt("people_domains")
        .max_height(140.0)
        .show(ui, |ui| {
            egui::Grid::new("domain_grid")
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    ui.strong("Domain");
                    ui.strong("People");
                    ui.strong("Msgs");
                    ui.end_row();
                    for d in &people.domains {
                        ui.label(&d.email_domain);
                        ui.label(d.person_count.to_string());
                        ui.label(d.message_count.to_string());
                        ui.end_row();
                    }
                });
        });

    ui.add_space(8.0);
    ui.heading("Timeline (day, matter-wide)");
    egui::ScrollArea::vertical()
        .id_salt("people_timeline")
        .max_height(160.0)
        .show(ui, |ui| {
            egui::Grid::new("tl_grid")
                .striped(true)
                .num_columns(2)
                .show(ui, |ui| {
                    ui.strong("Bucket");
                    ui.strong("Messages");
                    ui.end_row();
                    for t in &people.timeline {
                        ui.label(&t.bucket_start);
                        ui.label(t.message_count.to_string());
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::{item_status, ItemInput, Matter};
    use tempfile::tempdir;

    #[test]
    fn people_state_loads_empty_graph() {
        let tmp = tempdir().expect("tmp");
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
        let matter = Matter::create(&root, "PeopleUi").expect("create");
        let _ = matter
            .insert_item(ItemInput {
                path: Some("a.eml".into()),
                status: item_status::EXTRACTED.into(),
                from_addr: Some("a@example.com".into()),
                to_addrs_json: Some(r#"["b@example.com"]"#.into()),
                ..Default::default()
            })
            .expect("item");

        let mut state = PeopleState::new();
        state.request_reload(&root);
        // Poll until done (short).
        for _ in 0..200 {
            state.poll();
            if !state.busy {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!state.busy);
        assert!(state.error.is_none(), "{:?}", state.error);
        assert!(!state.status.is_complete);
    }

    #[test]
    fn take_start_and_filter() {
        let mut s = PeopleState::new();
        s.pending_start = true;
        assert!(s.take_start());
        assert!(!s.take_start());
        s.pending_filter_person_id = Some("pid".into());
        assert_eq!(s.take_filter_person().as_deref(), Some("pid"));
        assert!(s.take_filter_person().is_none());
    }

    #[test]
    fn top_pairs_columns_omit_bcc_and_filter_bcc_only_edges() {
        let labels = top_pairs_column_labels();
        assert!(!labels.iter().any(|l| l.eq_ignore_ascii_case("bcc")));
        assert!(labels.contains(&"Visible"));
        assert!(labels.contains(&"To/Cc"));

        let edges = vec![
            PeopleEdge {
                id: "e1".into(),
                matter_id: "m".into(),
                from_person_id: "a".into(),
                to_person_id: "b".into(),
                to_count: 1,
                cc_count: 0,
                bcc_count: 0,
                visible_count: 1,
                first_at: None,
                last_at: None,
                from_label: None,
                to_label: None,
                from_key: Some("a@example.com".into()),
                to_key: Some("b@example.com".into()),
            },
            PeopleEdge {
                id: "e2".into(),
                matter_id: "m".into(),
                from_person_id: "a".into(),
                to_person_id: "h".into(),
                to_count: 0,
                cc_count: 0,
                bcc_count: 2,
                visible_count: 0,
                first_at: None,
                last_at: None,
                from_label: None,
                to_label: None,
                from_key: Some("a@example.com".into()),
                to_key: Some("hidden@example.com".into()),
            },
        ];
        let visible = top_pairs_visible_edges(&edges);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "e1");
        assert!(visible.iter().all(|e| e.visible_count > 0));
    }
}
