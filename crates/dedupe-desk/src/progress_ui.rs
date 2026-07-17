//! Progress panel + repaint throttle policy.

use std::time::Duration;

use eframe::egui;
use process_runner::JobProgressSnapshot;

/// Required repaint interval while a job is active (~10 FPS).
pub const REPAINT_WHILE_JOB_MS: u64 = 100;

/// Whether the snapshot indicates an active (non-idle, non-terminal) job.
pub fn job_is_active(snap: &JobProgressSnapshot) -> bool {
    let s = snap.state.as_str();
    s == "running"
        || s == "pending"
        || (s != "idle" && !snap.is_terminal() && !snap.job_id.is_empty())
}

/// Request a throttled repaint when a job is running.
pub fn request_job_repaint(ctx: &egui::Context, snap: &JobProgressSnapshot) {
    if job_is_active(snap) || snap.state == "running" {
        ctx.request_repaint_after(Duration::from_millis(REPAINT_WHILE_JOB_MS));
    }
}

/// Paint the live progress panel from a watch snapshot.
pub fn show_progress_panel(ui: &mut egui::Ui, snap: &JobProgressSnapshot) {
    ui.group(|ui| {
        ui.heading("Process progress");
        if snap.job_id.is_empty() || snap.state == "idle" {
            ui.label("No active job.");
            return;
        }

        ui.horizontal(|ui| {
            ui.label(format!("Job: {}", short_id(&snap.job_id)));
            ui.separator();
            ui.label(format!("Kind: {}", snap.kind));
            ui.separator();
            ui.strong(format!("State: {}", snap.state));
        });

        if let Some(stage) = &snap.stage {
            ui.label(format!("Stage: {stage}"));
        }

        let fraction = progress_fraction(snap);
        let bar = egui::ProgressBar::new(fraction)
            .show_percentage()
            .text(format!("{} completed", snap.completed_count));
        ui.add(bar);

        if let Some(msg) = &snap.message {
            ui.label(msg);
        }
        if let Some(err) = &snap.error_summary {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                format!("Error: {err}"),
            );
        }

        match snap.state.as_str() {
            "running" => {
                ui.label("Processing… (cancel anytime)");
            }
            "paused" => {
                ui.label("Job paused. Resume when ready.");
            }
            "succeeded" => {
                ui.colored_label(egui::Color32::from_rgb(40, 140, 70), "Succeeded.");
            }
            "failed" => {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), "Failed.");
            }
            _ => {}
        }
    });
}

fn progress_fraction(snap: &JobProgressSnapshot) -> f32 {
    match snap.total_hint {
        Some(t) if t > 0 => (snap.completed_count as f32 / t as f32).clamp(0.0, 1.0),
        _ => {
            // Indeterminate-ish: oscillate lightly from completed_count.
            if snap.state == "running" {
                0.15 + ((snap.completed_count % 20) as f32) * 0.02
            } else if snap.is_terminal() && snap.state == "succeeded" {
                1.0
            } else {
                0.0
            }
        }
    }
}

fn short_id(id: &str) -> &str {
    if id.len() > 12 {
        &id[..12]
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_detection() {
        let mut s = JobProgressSnapshot::idle();
        assert!(!job_is_active(&s));
        s.state = "running".into();
        s.job_id = "j1".into();
        assert!(job_is_active(&s));
        s.state = "succeeded".into();
        assert!(!job_is_active(&s));
    }
}
