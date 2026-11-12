//! The details screen of one title: artwork, facts, and for a series a season picker with the
//! episodes of the chosen season.
//!
//! Everything here is drawing. The title comes from `State.details`; pictures come from
//! `Core::poster`.

use eframe::egui;
use streem_core::{Core, Details, Load, Meta, State, Video};

use super::Textures;

const BACKGROUND_HEIGHT: f32 = 220.0;
const POSTER_WIDTH: f32 = 200.0;
const POSTER_HEIGHT: f32 = 300.0;

#[derive(Default)]
pub struct DetailsScreen {
    textures: Textures,
    /// The title the chosen season belongs to, as (type, id).
    shown: Option<(String, String)>,
    season: Option<u32>,
}

impl DetailsScreen {
    /// Draws the screen. Returns true when Back was clicked.
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) -> bool {
        let back = ui.button("← Back").clicked();
        ui.add_space(6.0);

        let Some(details) = &state.details else {
            return back;
        };
        self.forget_season_of_another_title(details);

        match &details.meta {
            Load::Loading => {
                ui.spinner();
            }
            Load::Failed(message) => {
                ui.colored_label(egui::Color32::LIGHT_RED, message);
            }
            Load::Ready(meta) => {
                egui::ScrollArea::vertical()
                    .id_salt("details")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.title(ui, meta, core));
            }
        }
        back
    }

    fn forget_season_of_another_title(&mut self, details: &Details) {
        let key = (details.meta_type.clone(), details.meta_id.clone());
        if self.shown.as_ref() != Some(&key) {
            self.shown = Some(key);
            self.season = None;
        }
    }

    fn title(&mut self, ui: &mut egui::Ui, meta: &Meta, core: &Core) {
        self.background(ui, meta, core);
        ui.add_space(12.0);

        ui.horizontal_top(|ui| {
            self.poster(ui, meta, core);
            ui.vertical(|ui| {
                ui.heading(&meta.name);
                let facts: Vec<&str> = [meta.release_info.as_deref(), meta.rating.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect();
                if !facts.is_empty() {
                    ui.label(facts.join(" · "));
                }
                if !meta.genres.is_empty() {
                    ui.weak(meta.genres.join(", "));
                }
                if let Some(description) = &meta.description {
                    ui.add_space(8.0);
                    ui.label(description);
                }
            });
        });

        let seasons = seasons(&meta.videos);
        if !seasons.is_empty() {
            ui.add_space(16.0);
            self.episodes(ui, &meta.videos, &seasons);
        }
    }

    /// A wide picture across the top, cropped to fit and dimmed.
    fn background(&mut self, ui: &mut egui::Ui, meta: &Meta, core: &Core) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), BACKGROUND_HEIGHT),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(40));
        let texture = meta
            .background
            .as_deref()
            .and_then(|url| self.textures.get(ui.ctx(), core, url));
        if let Some(texture) = texture {
            painter.image(
                texture.id(),
                rect,
                cover_uv(texture.size_vec2(), rect.size()),
                egui::Color32::from_gray(150),
            );
        }
    }

    fn poster(&mut self, ui: &mut egui::Ui, meta: &Meta, core: &Core) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(POSTER_WIDTH, POSTER_HEIGHT),
            egui::Sense::hover(),
        );
        let texture = meta
            .poster
            .as_deref()
            .and_then(|url| self.textures.get(ui.ctx(), core, url));
        match texture {
            Some(texture) => {
                egui::Image::new(&texture)
                    .fit_to_exact_size(rect.size())
                    .corner_radius(4.0)
                    .paint_at(ui, rect);
            }
            None => {
                ui.painter()
                    .rect_filled(rect, 4.0, egui::Color32::from_gray(60));
            }
        }
        ui.add_space(8.0);
    }

    /// The season dropdown and the episodes of the chosen season.
    fn episodes(&mut self, ui: &mut egui::Ui, videos: &[Video], seasons: &[u32]) {
        // The first season in the list, unless the person chose one that is still there.
        let chosen = match self.season {
            Some(season) if seasons.contains(&season) => season,
            _ => seasons[0],
        };
        self.season = Some(chosen);

        egui::ComboBox::from_id_salt("season")
            .selected_text(season_label(chosen))
            .show_ui(ui, |ui| {
                for &season in seasons {
                    ui.selectable_value(&mut self.season, Some(season), season_label(season));
                }
            });
        ui.add_space(8.0);

        let mut episodes: Vec<&Video> = videos
            .iter()
            .filter(|video| video.season == Some(chosen))
            .collect();
        episodes.sort_by_key(|video| video.episode);
        for video in episodes {
            ui.label(episode_label(video));
        }
    }
}

/// The seasons the videos belong to, ascending, with season 0 (specials) last.
fn seasons(videos: &[Video]) -> Vec<u32> {
    let mut seasons: Vec<u32> = videos.iter().filter_map(|video| video.season).collect();
    seasons.sort_unstable_by_key(|&season| (season == 0, season));
    seasons.dedup();
    seasons
}

fn season_label(season: u32) -> String {
    match season {
        0 => "Specials".to_string(),
        number => format!("Season {number}"),
    }
}

/// "E3 · Title"; just the title when the episode has no number.
fn episode_label(video: &Video) -> String {
    match video.episode {
        Some(number) => format!("E{number} · {}", video.title),
        None => video.title.clone(),
    }
}

/// The part of a picture that fills `target` without stretching: the middle, cropped.
fn cover_uv(picture: egui::Vec2, target: egui::Vec2) -> egui::Rect {
    let picture_ratio = picture.x / picture.y;
    let target_ratio = target.x / target.y;
    let (width, height) = if picture_ratio > target_ratio {
        (target_ratio / picture_ratio, 1.0)
    } else {
        (1.0, picture_ratio / target_ratio)
    };
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(season: Option<u32>, episode: Option<u32>, title: &str) -> Video {
        Video {
            id: format!("{season:?}:{episode:?}"),
            title: title.to_string(),
            season,
            episode,
            released: None,
        }
    }

    #[test]
    fn seasons_ascend_with_specials_last() {
        let videos = [
            video(Some(2), Some(1), "a"),
            video(Some(0), Some(1), "b"),
            video(Some(1), Some(1), "c"),
            video(Some(1), Some(2), "d"),
            video(None, None, "e"),
        ];
        assert_eq!(seasons(&videos), [1, 2, 0]);
        assert_eq!(season_label(0), "Specials");
        assert_eq!(season_label(2), "Season 2");
    }

    #[test]
    fn a_movie_has_no_seasons() {
        assert!(seasons(&[video(None, None, "A Film")]).is_empty());
    }

    #[test]
    fn episodes_read_as_number_then_title() {
        assert_eq!(
            episode_label(&video(Some(1), Some(3), "Pilot")),
            "E3 · Pilot"
        );
    }
}
