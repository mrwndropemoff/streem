//! Streem desktop app. It draws what `streem-core` publishes and sends it commands.
//! It does no network or disk work itself.

mod screens;

use std::path::PathBuf;

use eframe::egui;
use streem_core::Core;

struct App {
    core: Core,
    addons: screens::addons::AddonsScreen,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let egui_ctx = cc.egui_ctx.clone();
        let core = Core::start(data_dir(), move || egui_ctx.request_repaint());
        App {
            core,
            addons: Default::default(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = self.core.state();
        screens::notice_banner(ui, &state, &self.core);
        egui::CentralPanel::default().show(ui, |ui| {
            self.addons.ui(ui, &state, &self.core);
        });
    }
}

/// `STREEM_DATA_DIR` if set, so two copies can run side by side; otherwise the user's data folder.
fn data_dir() -> PathBuf {
    match std::env::var_os("STREEM_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::data_dir().unwrap_or_default().join("streem"),
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        // The player (a later ticket) can only draw through OpenGL.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native("Streem", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
