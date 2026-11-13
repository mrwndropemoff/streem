//! The library screen: every title in the library as a wrapping grid of posters, most recently
//! added first.
//!
//! Everything here is drawing. Titles come from `State.library`; posters are drawn with the
//! same cell as the home screen, so only those on screen are asked for.

use eframe::egui;
use streem_core::{Core, LibraryItem, State};

use super::Textures;

#[derive(Default)]
pub struct LibraryScreen {
    textures: Textures,
}

impl LibraryScreen {
    /// Draws the grid. Returns the title whose poster was clicked this frame, if any.
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) -> Option<LibraryItem> {
        if state.library.is_empty() {
            ui.label("Your library is empty");
            return None;
        }
        let mut opened = None;
        // The scroll position lives under this id, so it is still there when the person comes
        // back from the details screen.
        egui::ScrollArea::vertical()
            .id_salt("library")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for item in &state.library {
                        let poster = item.poster.as_deref();
                        if super::poster(ui, &item.name, poster, &mut self.textures, core) {
                            opened = Some(item.clone());
                        }
                    }
                });
            });
        opened
    }
}
