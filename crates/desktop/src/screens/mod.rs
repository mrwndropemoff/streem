pub mod addons;
pub mod home;

use eframe::egui;
use streem_core::{Command, Core, State};

/// A dismissible banner at the top of the window, shown while `State.notice` is set.
pub fn notice_banner(ui: &mut egui::Ui, state: &State, core: &Core) {
    let Some(notice) = &state.notice else { return };
    egui::Panel::top("notice").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(egui::Color32::LIGHT_RED, &notice.text);
            if ui.button("Dismiss").clicked() {
                core.send(Command::DismissNotice);
            }
        });
    });
}
