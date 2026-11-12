//! The details screen of one title: artwork, facts, its streams, and for a series a season
//! picker with the episodes of the chosen season.
//!
//! Everything here is drawing. The title comes from `State.details`; pictures come from
//! `Core::poster`. Streams are asked for with `Command::LoadStreams`: for a movie as soon as
//! its details are ready, for a series when an episode is clicked. The library button sends
//! `Command::AddToLibrary` or `Command::RemoveFromLibrary` according to `Details::in_library`.

use eframe::egui;
use streem_core::{Command, Core, Details, Load, Meta, State, StreamItem, StreamList, Video};

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
    /// A movie's `LoadStreams` was sent and its answer is not in `State` yet. Stops it from
    /// being sent again on every frame until `Core` handles it.
    movie_streams_asked: bool,
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
        if details.streams.is_some() {
            self.movie_streams_asked = false;
        }

        match &details.meta {
            Load::Loading => {
                ui.spinner();
            }
            Load::Failed(message) => {
                ui.colored_label(egui::Color32::LIGHT_RED, message);
            }
            Load::Ready(meta) => {
                if details.meta_type == "movie"
                    && details.streams.is_none()
                    && !self.movie_streams_asked
                {
                    core.send(Command::LoadStreams {
                        video_id: details.meta_id.clone(),
                    });
                    self.movie_streams_asked = true;
                }
                egui::ScrollArea::vertical()
                    .id_salt("details")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.title(ui, details, meta, core));
            }
        }
        back
    }

    /// Called when the person leaves this screen, so that opening the same movie again asks
    /// for its streams again.
    pub fn on_leave(&mut self) {
        self.movie_streams_asked = false;
    }

    fn forget_season_of_another_title(&mut self, details: &Details) {
        let key = (details.meta_type.clone(), details.meta_id.clone());
        if self.shown.as_ref() != Some(&key) {
            self.shown = Some(key);
            self.season = None;
        }
    }

    fn title(&mut self, ui: &mut egui::Ui, details: &Details, meta: &Meta, core: &Core) {
        self.background(ui, meta, core);
        ui.add_space(12.0);

        ui.horizontal_top(|ui| {
            self.poster(ui, meta, core);
            ui.vertical(|ui| {
                ui.heading(&meta.name);
                library_button(ui, details, core);
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

        let streams = details.streams.as_ref();
        let seasons = seasons(&meta.videos);
        if details.meta_type == "movie" {
            ui.add_space(16.0);
            // Asked for as soon as the details were ready, so a missing list is on its way.
            if let Some(play) = stream_list(ui, streams, None) {
                core.send(play);
            }
        } else if !seasons.is_empty() {
            ui.add_space(16.0);
            ui.columns(2, |columns| {
                let clicked = self.episodes(
                    &mut columns[0],
                    &meta.videos,
                    &seasons,
                    streams.map(|list| list.video_id.as_str()),
                );
                if let Some(video_id) = clicked {
                    core.send(Command::LoadStreams { video_id });
                }
                let waiting = Some("Choose an episode to see its streams.");
                if let Some(play) = stream_list(&mut columns[1], streams, waiting) {
                    core.send(play);
                }
            });
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

    /// The season dropdown and the episodes of the chosen season. The episode whose streams are
    /// shown is highlighted. Returns the id of the episode the person clicked.
    fn episodes(
        &mut self,
        ui: &mut egui::Ui,
        videos: &[Video],
        seasons: &[u32],
        selected: Option<&str>,
    ) -> Option<String> {
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
        let mut clicked = None;
        for video in episodes {
            let is_selected = selected == Some(video.id.as_str());
            if ui
                .selectable_label(is_selected, episode_label(video))
                .clicked()
            {
                clicked = Some(video.id.clone());
            }
        }
        clicked
    }
}

/// "Add to library", or "Remove from library" once the title is in it.
fn library_button(ui: &mut egui::Ui, details: &Details, core: &Core) {
    let label = if details.in_library {
        "Remove from library"
    } else {
        "Add to library"
    };
    if ui.button(label).clicked() {
        core.send(if details.in_library {
            Command::RemoveFromLibrary {
                meta_id: details.meta_id.clone(),
            }
        } else {
            Command::AddToLibrary
        });
    }
}

/// The streams under each **addon**'s name. `waiting` is shown while no streams were asked
/// for; without it, a spinner is. Returns the `Play` command for the stream the person clicked.
fn stream_list(
    ui: &mut egui::Ui,
    streams: Option<&StreamList>,
    waiting: Option<&str>,
) -> Option<Command> {
    ui.heading("Streams");
    ui.add_space(4.0);
    let Some(list) = streams else {
        match waiting {
            Some(text) => {
                ui.weak(text);
            }
            None => {
                ui.spinner();
            }
        }
        return None;
    };
    if list.groups.is_empty() {
        ui.weak("No installed addon has streams for this.");
        return None;
    }
    let mut play = None;
    for (group_index, group) in list.groups.iter().enumerate() {
        ui.add_space(8.0);
        ui.strong(&group.addon_name);
        match &group.streams {
            Load::Loading => {
                ui.spinner();
            }
            Load::Failed(message) => {
                ui.colored_label(egui::Color32::LIGHT_RED, message);
            }
            Load::Ready(items) => {
                for (index, item) in items.iter().enumerate() {
                    if stream_row(ui, item, (group_index, index)) {
                        play = Some(Command::Play {
                            addon_id: group.addon_id.clone(),
                            stream_index: index,
                        });
                    }
                }
            }
        }
    }
    play
}

/// One stream: its name and description. An unplayable one is greyed out with the reason.
/// Returns true when a playable one was clicked.
fn stream_row(ui: &mut egui::Ui, item: &StreamItem, position: (usize, usize)) -> bool {
    let shown = ui.add_enabled_ui(item.playable, |ui| {
        egui::Frame::group(ui.style())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.strong(&item.name);
                    ui.label(&item.description);
                });
                if let Some(reason) = &item.unplayable_reason {
                    ui.label(reason);
                }
            })
            .response
            .rect
    });
    if !item.playable {
        return false;
    }
    let rect = shown.inner;
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("stream", position)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_stroke(
            rect,
            4.0,
            ui.visuals().widgets.hovered.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    response.clicked()
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
