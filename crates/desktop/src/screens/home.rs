//! The home screen: one horizontal row of posters for each catalog the **addons** offer.
//!
//! Everything here is drawing. Rows come from `State.home`; poster pictures come from
//! `Core::poster`, which is asked only for posters that are on screen.

use eframe::egui;
use streem_core::{CatalogRow, Core, Load, MetaPreview, State};

use super::Textures;

const POSTER_WIDTH: f32 = 140.0;
const POSTER_HEIGHT: f32 = 210.0;
const NAME_HEIGHT: f32 = 22.0;
const GAP: f32 = 10.0;
/// Placeholders shown while a row is still loading.
const LOADING_PLACEHOLDERS: usize = 12;

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
                    if let Some(item) = self.row(ui, index, row, core) {
                        opened = Some(item);
                    }
                    ui.add_space(12.0);
                }
            });
        opened
    }

    fn row(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        row: &CatalogRow,
        core: &Core,
    ) -> Option<MetaPreview> {
        let mut opened = None;
        ui.horizontal(|ui| {
            ui.heading(&row.catalog_name);
            ui.weak(format!("{} · {}", row.addon_name, row.meta_type));
        });
        match &row.items {
            Load::Failed(message) => {
                ui.colored_label(egui::Color32::LIGHT_RED, message);
            }
            Load::Loading => {
                egui::ScrollArea::horizontal()
                    .id_salt(("home-row", index))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for _ in 0..LOADING_PLACEHOLDERS {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(POSTER_WIDTH, POSTER_HEIGHT + NAME_HEIGHT),
                                    egui::Sense::hover(),
                                );
                                if ui.is_rect_visible(rect) {
                                    paint_placeholder(ui, poster_rect(rect));
                                }
                            }
                        });
                    });
            }
            Load::Ready(items) => {
                egui::ScrollArea::horizontal()
                    .id_salt(("home-row", index))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for item in items {
                                if self.poster(ui, item, core) {
                                    opened = Some(item.clone());
                                }
                            }
                        });
                    });
            }
        }
        opened
    }

    /// Draws one poster. Returns true when it was clicked.
    fn poster(&mut self, ui: &mut egui::Ui, item: &MetaPreview, core: &Core) -> bool {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(POSTER_WIDTH, POSTER_HEIGHT + NAME_HEIGHT),
            egui::Sense::click(),
        );
        // Off screen: no texture, no text and no request to the core.
        if !ui.is_rect_visible(rect) {
            return false;
        }
        let picture_rect = poster_rect(rect);

        let texture = item
            .poster
            .as_deref()
            .and_then(|url| self.textures.get(ui.ctx(), core, url));
        match texture {
            Some(texture) => {
                egui::Image::new(&texture)
                    .fit_to_exact_size(picture_rect.size())
                    .corner_radius(4.0)
                    .paint_at(ui, picture_rect);
            }
            None => paint_placeholder(ui, picture_rect),
        }

        let text_color = ui.visuals().text_color();
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut job = egui::text::LayoutJob::simple_singleline(item.name.clone(), font, text_color);
        job.wrap = egui::text::TextWrapping {
            max_width: POSTER_WIDTH,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(rect.left(), picture_rect.bottom() + 4.0),
            galley,
            text_color,
        );

        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    }
}

/// The picture's part of a poster's cell; the name goes below it.
fn poster_rect(cell: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(cell.min, egui::vec2(POSTER_WIDTH - GAP, POSTER_HEIGHT))
}

fn paint_placeholder(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter()
        .rect_filled(rect, 4.0, egui::Color32::from_gray(60));
}
