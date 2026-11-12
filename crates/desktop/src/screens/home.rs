//! The home screen: one horizontal row of posters for each catalog the **addons** offer.
//!
//! Everything here is drawing. Rows come from `State.home`; poster pictures come from
//! `Core::poster`, which is asked only for posters that are on screen.

use std::collections::HashMap;

use eframe::egui;
use streem_core::{CatalogRow, Core, Load, MetaPreview, Poster, State};

const POSTER_WIDTH: f32 = 140.0;
const POSTER_HEIGHT: f32 = 210.0;
const NAME_HEIGHT: f32 = 22.0;
const GAP: f32 = 10.0;
/// Placeholders shown while a row is still loading.
const LOADING_PLACEHOLDERS: usize = 12;

#[derive(Default)]
pub struct HomeScreen {
    /// A texture is made from a `Poster` once; this keeps it for every later frame.
    textures: HashMap<String, egui::TextureHandle>,
}

impl HomeScreen {
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if state.home.is_empty() {
                    ui.label("No catalogs yet. Install an addon on the Addons screen.");
                }
                for (index, row) in state.home.iter().enumerate() {
                    self.row(ui, index, row, core);
                    ui.add_space(12.0);
                }
            });
    }

    fn row(&mut self, ui: &mut egui::Ui, index: usize, row: &CatalogRow, core: &Core) {
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
                                self.poster(ui, item, core);
                            }
                        });
                    });
            }
        }
    }

    fn poster(&mut self, ui: &mut egui::Ui, item: &MetaPreview, core: &Core) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(POSTER_WIDTH, POSTER_HEIGHT + NAME_HEIGHT),
            egui::Sense::hover(),
        );
        // Off screen: no texture, no text and no request to the core.
        if !ui.is_rect_visible(rect) {
            return;
        }
        let picture_rect = poster_rect(rect);

        match self.texture(ui.ctx(), item, core) {
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
    }

    /// The texture of an item's poster, made the first time the poster is ready.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        item: &MetaPreview,
        core: &Core,
    ) -> Option<egui::TextureHandle> {
        let url = item.poster.as_deref()?;
        if let Some(texture) = self.textures.get(url) {
            return Some(texture.clone());
        }
        let poster = core.poster(url)?;
        let texture = ctx.load_texture(url, color_image(&poster), egui::TextureOptions::LINEAR);
        self.textures.insert(url.to_string(), texture.clone());
        Some(texture)
    }
}

fn color_image(poster: &Poster) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [poster.width as usize, poster.height as usize],
        &poster.rgba,
    )
}

/// The picture's part of a poster's cell; the name goes below it.
fn poster_rect(cell: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(cell.min, egui::vec2(POSTER_WIDTH - GAP, POSTER_HEIGHT))
}

fn paint_placeholder(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter()
        .rect_filled(rect, 4.0, egui::Color32::from_gray(60));
}
