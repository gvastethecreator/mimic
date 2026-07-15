#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Hide console window in release build

mod compositor;
mod config;
mod decoder;
mod gui;
mod setup;
mod webcam;

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mimic - Virtual Camera Studio")
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([980.0, 640.0])
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "mimic_app",
        options,
        Box::new(|cc| Box::new(gui::MimicApp::new(cc))),
    )
}
