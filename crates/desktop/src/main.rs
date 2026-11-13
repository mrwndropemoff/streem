//! Streem desktop app. It draws what `streem-core` publishes and sends it commands.
//! It does no network or disk work itself.

mod mpv_view;
mod screens;

use std::path::PathBuf;

use eframe::egui;
use streem_core::{Command, Core, MetaPreview, PlaybackReport, State};

use mpv_view::MpvView;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Addons,
    /// The results in `State.search`.
    Search,
    /// The title in `State.details`. Back goes to `App::details_from`.
    Details,
    Settings,
}

struct App {
    core: Core,
    screen: Screen,
    /// The screen the open title was opened from: Home or Search.
    details_from: Screen,
    /// What is typed in the search box.
    search_text: String,
    home: screens::home::HomeScreen,
    search: screens::search::SearchScreen,
    addons: screens::addons::AddonsScreen,
    details: screens::details::DetailsScreen,
    player: screens::player::PlayerScreen,
    settings: screens::settings::SettingsScreen,
    /// `None` when libmpv could not start. Browsing still works; playing a stream then fails.
    video: Option<MpvView>,
    /// `State.playback` was set in the last frame, so the player screen was showing.
    was_playing: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let egui_ctx = cc.egui_ctx.clone();
        let core = Core::start(data_dir(), move || egui_ctx.request_repaint());
        let video = match MpvView::new(cc) {
            Ok(video) => Some(video),
            Err(error) => {
                eprintln!("{error}");
                None
            }
        };
        App {
            core,
            screen: Screen::Home,
            details_from: Screen::Home,
            search_text: String::new(),
            home: Default::default(),
            search: Default::default(),
            addons: Default::default(),
            details: Default::default(),
            player: Default::default(),
            settings: Default::default(),
            video,
            was_playing: false,
        }
    }

    /// The player screen, shown in place of everything else while `State.playback` is set.
    /// Playing and stopping are `Command`s; this only makes the video match the state.
    fn show_player(&mut self, ui: &mut egui::Ui, state: &State) {
        self.was_playing = true;
        let Some(playback) = &state.playback else {
            return;
        };
        let Some(video) = &mut self.video else {
            self.core.send(Command::ReportPlayback(PlaybackReport {
                position_secs: 0.0,
                duration_secs: 0.0,
                buffering: false,
                ended: false,
                failed: Some("libmpv is not available".into()),
            }));
            return;
        };
        video.sync(ui.ctx(), playback, &state.settings, &self.core);
        self.player.ui(ui, state, &self.core, video);
    }

    /// Called every frame while nothing is playing. The first time after something was, it
    /// unloads the file and leaves fullscreen, so the details screen is not stuck in it.
    fn after_playback(&mut self, ctx: &egui::Context) {
        if !self.was_playing {
            return;
        }
        self.was_playing = false;
        if let Some(video) = &mut self.video {
            video.stop();
        }
        self.player.on_leave();
        if ctx.input(|input| input.viewport().fullscreen.unwrap_or(false)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    /// Leaves the details screen for `screen` and tells the core the title is closed.
    fn leave_details(&mut self, screen: Screen) {
        self.core.send(Command::CloseTitle);
        self.details.on_leave();
        self.screen = screen;
    }

    /// Opens the details of a title whose poster was clicked on the current screen.
    fn open_title(&mut self, item: MetaPreview) {
        self.core.send(Command::OpenTitle {
            meta_type: item.meta_type,
            meta_id: item.id,
        });
        self.details_from = self.screen;
        self.screen = Screen::Details;
    }

    /// Enter was pressed in the search box: searches and shows the results. An empty box
    /// clears the search and leaves the search screen.
    fn submit_search(&mut self) {
        self.core.send(Command::Search {
            query: self.search_text.clone(),
        });
        if !self.search_text.trim().is_empty() {
            self.screen = Screen::Search;
        } else if self.screen == Screen::Search {
            self.screen = Screen::Home;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = self.core.state();
        if state.playback.is_some() {
            self.show_player(ui, &state);
            return;
        }
        self.after_playback(&ui.ctx().clone());
        screens::notice_banner(ui, &state, &self.core);
        let before = self.screen;
        let mut search_submitted = false;
        egui::Panel::top("navigation").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.screen, Screen::Home, "Home");
                ui.selectable_value(&mut self.screen, Screen::Addons, "Addons");
                ui.selectable_value(&mut self.screen, Screen::Settings, "Settings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.search_text)
                            .hint_text("Search")
                            .desired_width(240.0),
                    );
                    search_submitted = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                });
            });
        });
        if search_submitted {
            self.submit_search();
        }
        if before == Screen::Details && self.screen != Screen::Details {
            // A tab was clicked, or a search made, while a title was open.
            self.leave_details(self.screen);
        }

        egui::CentralPanel::default().show(ui, |ui| match self.screen {
            Screen::Home => {
                if let Some(item) = self.home.ui(ui, &state, &self.core) {
                    self.open_title(item);
                }
            }
            Screen::Search => {
                if let Some(item) = self.search.ui(ui, &state, &self.core) {
                    self.open_title(item);
                }
            }
            Screen::Addons => self.addons.ui(ui, &state, &self.core),
            Screen::Settings => self.settings.ui(ui, &state, &self.core),
            Screen::Details => {
                if self.details.ui(ui, &state, &self.core) {
                    self.leave_details(self.details_from);
                }
            }
        });
    }
}

/// `STREEM_DATA_DIR` if set, so two copies can run side by side; otherwise the user's data folder.
fn data_dir() -> PathBuf {
    match std::env::var_os("STREEM_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::data_dir().unwrap_or_default().join("streem"),
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        // The player can only draw through OpenGL.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native("Streem", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
