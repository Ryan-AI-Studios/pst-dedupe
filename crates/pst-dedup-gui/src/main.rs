//! PST-Dedup GUI entry point.

// Hide console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod worker;
mod views;

fn main() -> eframe::Result<()> {
    // Initialize tracing for debug builds
    #[cfg(debug_assertions)]
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PST-Dedup — Email Deduplication Tool")
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([700.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "PST-Dedup",
        options,
        Box::new(|cc| Ok(Box::new(app::PstDedupApp::new(cc)))),
    )
}
