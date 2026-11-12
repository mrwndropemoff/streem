//! Streem desktop app. It draws what `streem-core` publishes and sends it commands.
//! It does no network or disk work itself.

mod mpv_view;
mod screens;

use std::path::PathBuf;

use eframe::egui;
use streem_core::{Command, Core, PlaybackReport, State};

use mpv_view::MpvView;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Addons,
    /// The title in `State.details`. Back goes to Home, the only place a title is opened from.
    Details,
}

struct App {
    core: Core,
    screen: Screen,
    home: screens::home::HomeScreen,
    addons: screens::addons::AddonsScreen,
    details: screens::details::DetailsScreen,
    player: screens::player::PlayerScreen,
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
            home: Default::default(),
            addons: Default::default(),
            details: Default::default(),
            player: Default::default(),
            video,
            was_playing: false,
        }
    }

    /// The player screen, shown in place of everything else while `State.playback` is set.
    /// Playing and stopping are `Command`s; this only makes the video match the state.
    fn play(&mut self, ui: &mut egui::Ui, state: &State) {
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
        video.sync(ui.ctx(), playback, &self.core);
        let video = &*video;
        self.player.ui(ui, state, &self.core, |ui, rect| {
            video.paint(ui, rect);
        });
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
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = self.core.state();
        if state.playback.is_some() {
            self.play(ui, &state);
            return;
        }
        self.after_playback(&ui.ctx().clone());
        screens::notice_banner(ui, &state, &self.core);
        let before = self.screen;
        egui::Panel::top("navigation").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.screen, Screen::Home, "Home");
                ui.selectable_value(&mut self.screen, Screen::Addons, "Addons");
            });
        });
        if before == Screen::Details && self.screen != Screen::Details {
            // A tab was clicked while a title was open.
            self.leave_details(self.screen);
        }

        egui::CentralPanel::default().show(ui, |ui| match self.screen {
            Screen::Home => {
                if let Some(item) = self.home.ui(ui, &state, &self.core) {
                    self.core.send(Command::OpenTitle {
                        meta_type: item.meta_type,
                        meta_id: item.id,
                    });
                    self.screen = Screen::Details;
                }
            }
            Screen::Addons => self.addons.ui(ui, &state, &self.core),
            Screen::Details => {
                if self.details.ui(ui, &state, &self.core) {
                    self.leave_details(Screen::Home);
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
