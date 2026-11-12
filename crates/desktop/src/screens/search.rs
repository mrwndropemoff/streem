//! The search screen: one horizontal row of posters for each catalog that was searched.
//!
//! Everything here is drawing. Rows come from `State.search` and are drawn with
//! [`super::catalog_row`]. A row that came back with no results is not shown.

use eframe::egui;
use streem_core::{Core, Load, MetaPreview, State};

use super::Textures;

#[derive(Default)]
pub struct SearchScreen {
    textures: Textures,
}

impl SearchScreen {
    /// Draws the results. Returns the title whose poster was clicked this frame, if any.
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) -> Option<MetaPreview> {
        let Some(search) = &state.search else {
            ui.label("Type in the search box and press Enter.");
            return None;
        };
        let mut opened = None;
        // Keyed by the query, so a new search starts at the top.
        egui::ScrollArea::vertical()
            .id_salt(("search", &search.query))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let no_results = search
                    .rows
                    .iter()
                    .all(|row| matches!(&row.items, Load::Ready(items) if items.is_empty()));
                if no_results {
                    ui.label("No results");
                }
                for (index, row) in search.rows.iter().enumerate() {
                    if matches!(&row.items, Load::Ready(items) if items.is_empty()) {
                        continue;
                    }
                    let id_salt = ("search-row", &search.query, index);
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
