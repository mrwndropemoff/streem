pub mod addons;
pub mod details;
pub mod home;
pub mod player;
pub mod search;

use std::collections::HashMap;

use eframe::egui;
use streem_core::{CatalogRow, Command, Core, Load, MetaPreview, Poster, State};

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

const POSTER_WIDTH: f32 = 140.0;
const POSTER_HEIGHT: f32 = 210.0;
const NAME_HEIGHT: f32 = 22.0;
const GAP: f32 = 10.0;
/// Placeholders shown while a row is still loading.
const LOADING_PLACEHOLDERS: usize = 12;

/// Draws one catalog as a heading and a horizontal row of posters, used by the home and search
/// screens. `id_salt` keeps the row's scroll position apart from every other row's. Returns the
/// title whose poster was clicked this frame, if any.
///
/// Poster pictures come from `Core::poster`, which is asked only for posters that are on screen.
pub fn catalog_row(
    ui: &mut egui::Ui,
    id_salt: impl egui::AsIdSalt,
    row: &CatalogRow,
    textures: &mut Textures,
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
                .id_salt(id_salt)
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
                .id_salt(id_salt)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for item in items {
                            if poster(ui, item, textures, core) {
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
fn poster(ui: &mut egui::Ui, item: &MetaPreview, textures: &mut Textures, core: &Core) -> bool {
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
        .and_then(|url| textures.get(ui.ctx(), core, url));
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

/// The picture's part of a poster's cell; the name goes below it.
fn poster_rect(cell: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(cell.min, egui::vec2(POSTER_WIDTH - GAP, POSTER_HEIGHT))
}

fn paint_placeholder(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter()
        .rect_filled(rect, 4.0, egui::Color32::from_gray(60));
}
