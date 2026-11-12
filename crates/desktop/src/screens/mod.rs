pub mod addons;
pub mod details;
pub mod home;
pub mod player;

use std::collections::HashMap;

use eframe::egui;
use streem_core::{Command, Core, Poster, State};

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

/// Pictures turned into textures. A texture is made from a `Poster` once; this keeps it for
/// every later frame.
#[derive(Default)]
pub struct Textures {
    by_url: HashMap<String, egui::TextureHandle>,
}

impl Textures {
    /// The texture of the picture at `url`, made the first time the core has it ready.
    /// Asks the core for the picture, which starts loading it if it was not asked for before.
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        core: &Core,
        url: &str,
    ) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.by_url.get(url) {
            return Some(texture.clone());
        }
        let poster = core.poster(url)?;
        let texture = ctx.load_texture(url, color_image(&poster), egui::TextureOptions::LINEAR);
        self.by_url.insert(url.to_string(), texture.clone());
        Some(texture)
    }
}

fn color_image(poster: &Poster) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [poster.width as usize, poster.height as usize],
        &poster.rgba,
    )
}
