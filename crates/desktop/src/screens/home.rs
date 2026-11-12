//! The home screen: one horizontal row of posters for each catalog the **addons** offer.
//!
//! Everything here is drawing. Rows come from `State.home` and are drawn with
//! [`super::catalog_row`].

use eframe::egui;
use streem_core::{Core, MetaPreview, State};

use super::Textures;

#[derive(Default)]
pub struct HomeScreen {
    textures: Textures,
}

impl HomeScreen {
    /// Draws the rows. Returns the title whose poster was clicked this frame, if any.
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) -> Option<MetaPreview> {
        let mut opened = None;
        // The scroll position lives under this id, so it is still there when the person comes
        // back from the details screen.
        egui::ScrollArea::vertical()
            .id_salt("home")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if state.home.is_empty() {
                    ui.label("No catalogs yet. Install an addon on the Addons screen.");
                }
                for (index, row) in state.home.iter().enumerate() {
                    let id_salt = ("home-row", index);
                    if let Some(item) =
                        super::catalog_row(ui, id_salt, row, &mut self.textures, core)
                    {
                        opened = Some(item);
                    }
                    ui.add_space(12.0);
                }
            });
        opened
    }
}
