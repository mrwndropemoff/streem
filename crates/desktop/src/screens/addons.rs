//! The list of installed **addons**, with a field to install another and a button to remove one.

use eframe::egui;
use streem_core::{Command, Core, State};

#[derive(Default)]
pub struct AddonsScreen {
    url_input: String,
}

impl AddonsScreen {
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) {
        ui.heading("Addons");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.url_input)
                    .hint_text("Paste an addon URL (…/manifest.json)")
                    .desired_width(ui.available_width() - 90.0),
            );
            let enter_pressed =
                field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let install_clicked = ui.button("Install").clicked();
            if (install_clicked || enter_pressed) && !self.url_input.trim().is_empty() {
                core.send(Command::InstallAddon {
                    url: std::mem::take(&mut self.url_input),
                });
            }
        });
        ui.add_space(8.0);
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if state.addons.is_empty() {
                ui.label("No addons installed.");
            }
            for addon in &state.addons {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&addon.name).strong());
                        ui.weak(format!("version {}", addon.version));
                        if !addon.description.is_empty() {
                            ui.label(&addon.description);
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Remove").clicked() {
                            core.send(Command::RemoveAddon {
                                url: addon.url.clone(),
                            });
                        }
                    });
                });
                ui.separator();
            }
        });
    }
}
