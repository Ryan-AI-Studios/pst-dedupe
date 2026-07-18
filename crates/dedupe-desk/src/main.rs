//! Dedupe Desk — single-exe matter / sources / process shell.
//!
//! Track **0020**. Heavy work runs only on the process-runner matter worker.
//! This binary owns the UI thread: start/cancel/resume/watch only.

// Hide console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialogs;
mod html_strip;
mod matter_ops;
mod matter_ui;
mod nav;
mod params;
mod progress_ui;
mod review_body;
mod review_nav;
mod review_notes;
mod review_privilege;
mod review_ui;
mod settings;
mod workspace;

fn main() -> eframe::Result<()> {
    #[cfg(debug_assertions)]
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Dedupe Desk")
            .with_inner_size([980.0, 700.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Dedupe Desk",
        options,
        Box::new(|cc| Ok(Box::new(app::DeskApp::new(cc)))),
    )
}
