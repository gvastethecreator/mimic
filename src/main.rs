#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Hide console window in release build

mod setup;
mod decoder;
mod webcam;
mod compositor;
mod gui;

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mimic - Minimalist Virtual Webcam Simulator")
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([900.0, 600.0])
            .with_active(true),
        ..Default::default()
    };
    
    eframe::run_native(
        "mimic_app",
        options,
        Box::new(|cc| Box::new(gui::MimicApp::new(cc))),
    )
}
