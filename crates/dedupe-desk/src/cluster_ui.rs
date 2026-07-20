//! Concept / theme clustering panel (track 0048) — list actual clusters only.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::Utf8Path;
use eframe::egui;
use matter_core::{ConceptCluster, ConceptClusterStatus, Matter};

/// Desk-side concept cluster panel state.
#[derive(Default)]
pub struct ClusterState {
    pub status: ConceptClusterStatus,
    pub clusters: Vec<ConceptCluster>,
    pub error: Option<String>,
    pub last_status: Option<String>,
    pub busy: bool,
    /// When set, app applies concept_cluster_id FilterSpec and navigates to Review.
    pub pending_filter_cluster_id: Option<String>,
    /// Request start of concept_cluster job (consumed by app).
    pub pending_start: bool,
    /// Requested k draft for thin UI (default 20).
    pub k_draft: String,
    op_rx: Option<Receiver<ClusterOpResult>>,
}

enum ClusterOpResult {
    Loaded {
        status: Box<ConceptClusterStatus>,
        clusters: Vec<ConceptCluster>,
    },
    Error(String),
}

impl ClusterState {
    pub fn new() -> Self {
        Self {
            k_draft: "20".into(),
            ..Default::default()
        }
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
            let result = (|| -> Result<ClusterOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let status = matter
                    .concept_cluster_status("default")
                    .map_err(|e| e.to_string())?;
                let clusters = if let Some(ref sid) = status.set_id {
                    matter
                        .list_concept_clusters(sid)
                        .map_err(|e| e.to_string())?
                } else {
                    Vec::new()
                };
                Ok(ClusterOpResult::Loaded {
                    status: Box::new(status),
                    clusters,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(ClusterOpResult::Error));
        });
    }

    pub fn poll(&mut self) {
        let Some(rx) = self.op_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(ClusterOpResult::Loaded { status, clusters }) => {
                self.status = *status;
                self.clusters = clusters;
                self.busy = false;
                self.op_rx = None;
                self.error = None;
            }
            Ok(ClusterOpResult::Error(e)) => {
                self.error = Some(e);
                self.busy = false;
                self.op_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.op_rx = None;
                self.error = Some("Cluster load thread ended unexpectedly.".into());
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

    pub fn take_filter_cluster(&mut self) -> Option<String> {
        self.pending_filter_cluster_id.take()
    }

    /// Parsed k from draft (fallback 20).
    pub fn requested_k(&self) -> u32 {
        self.k_draft
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|k| *k >= 1)
            .unwrap_or(20)
    }
}

/// Draw the Clusters / Themes screen.
pub fn show(
    ui: &mut egui::Ui,
    clusters: &mut ClusterState,
    matter_root: Option<&Utf8Path>,
    busy: bool,
) {
    clusters.poll();

    ui.heading("Concept clusters / themes");
    ui.label(
        "Offline TF–IDF + k-means themes (tfidf_kmeans_v1). Not near-dup, not embeddings, \
         not Relativity LSI. Requested k is a target — list shows actual non-empty clusters only.",
    );
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label("k (requested):");
        ui.add(
            egui::TextEdit::singleline(&mut clusters.k_draft)
                .desired_width(48.0)
                .hint_text("20"),
        );
        if ui
            .add_enabled(
                !busy && !clusters.busy,
                egui::Button::new("Run concept clustering"),
            )
            .on_hover_text("Run concept_cluster job (reset rebuild, default set)")
            .clicked()
        {
            clusters.pending_start = true;
        }
        if ui
            .add_enabled(
                matter_root.is_some() && !clusters.busy,
                egui::Button::new("Refresh"),
            )
            .clicked()
        {
            if let Some(root) = matter_root {
                clusters.request_reload(root);
            }
        }
        if clusters.busy {
            ui.spinner();
            ui.label("Loading…");
        }
    });

    if let Some(err) = &clusters.error {
        ui.colored_label(egui::Color32::from_rgb(200, 80, 80), err);
    }
    if let Some(st) = &clusters.last_status {
        ui.label(st);
    }

    if !clusters.status.is_complete {
        ui.colored_label(
            egui::Color32::from_rgb(180, 120, 40),
            "No complete concept cluster set — extract text (CAS text_sha256) then Run concept clustering.",
        );
    } else if let Some(at) = &clusters.status.built_at {
        let k = clusters.status.k.unwrap_or(0);
        ui.label(format!(
            "Complete · built_at={at} · method={} · requested k={k} · actual clusters={} · items={}",
            clusters
                .status
                .method
                .as_deref()
                .unwrap_or("?"),
            clusters.status.cluster_count,
            clusters.status.item_count
        ));
        if clusters.status.is_stale {
            ui.colored_label(
                egui::Color32::from_rgb(200, 120, 40),
                "Stale: candidate texts changed after built_at — re-run concept clustering (reset=true).",
            );
        }
        if clusters.status.cluster_count < k {
            ui.label(format!(
                "Note: actual cluster_count ({}) < requested k ({}) — empty centroids were dropped.",
                clusters.status.cluster_count, k
            ));
        }
    }

    ui.add_space(8.0);
    ui.separator();
    ui.heading("Clusters (by size)");
    ui.label("Never assume list length equals k — iterate query results only.");

    if clusters.clusters.is_empty() {
        ui.label("(no clusters)");
    } else {
        egui::Grid::new("concept_clusters_grid")
            .striped(true)
            .num_columns(4)
            .show(ui, |ui| {
                ui.strong("Ordinal");
                ui.strong("Items");
                ui.strong("Label");
                ui.strong("");
                ui.end_row();
                // Iterate actual clusters only — never index by k.
                for c in &clusters.clusters {
                    ui.label(format!("{}", c.ordinal));
                    ui.label(format!("{}", c.item_count));
                    ui.label(&c.label);
                    if ui.small_button("Filter Review").clicked() {
                        clusters.pending_filter_cluster_id = Some(c.id.clone());
                    }
                    ui.end_row();
                }
            });
    }
}
