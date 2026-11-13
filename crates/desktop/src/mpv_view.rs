//! The libmpv video surface. It owns the one mpv instance and draws its video into egui's own
//! OpenGL context through libmpv's render API.
//!
//! It is the only place that talks to mpv, and it takes its orders from `State.playback`: every
//! frame [`MpvView::sync`] compares mpv with what `Core` says and corrects mpv, then tells `Core`
//! what mpv is doing with `Command::ReportPlayback`. The controls never come here, with one
//! exception: the audio and subtitle menus read mpv's track list with [`MpvView::tracks`] and
//! pick a track directly, because which track is shown is local to the player.

use std::ffi::{CStr, CString, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use eframe::{egui, egui_glow, glow, glow::HasContext};
use libmpv2::Mpv;
use libmpv2::events::Event;
use libmpv2::mpv_end_file_reason;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use streem_core::{Command, Core, Playback, PlaybackReport, Settings, SubtitleTrack};

use crate::screens::settings::mpv_languages;

/// While playing, `Core` hears about the position this often.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// One audio or subtitle track of the loaded file, as a menu shows it.
pub struct Track {
    /// mpv's id for it, which [`MpvView::select_audio`] and [`MpvView::select_subtitle`] take.
    pub id: i64,
    pub label: String,
    pub selected: bool,
}

/// The tracks of the loaded file, in menu order.
#[derive(Default)]
pub struct Tracks {
    pub audio: Vec<Track>,
    /// The tracks inside the file, then the ones supplied by **addons**.
    pub subtitles: Vec<Track>,
}

/// A subtitle from an **addon** to add to the file with this generation.
struct SubtitleJob {
    file: u64,
    url: String,
    lang: String,
}

type GlLoader = Arc<dyn Fn(&CStr) -> *const c_void + Send + Sync>;

/// `RenderContext` holds a raw pointer, so it is neither `Send` nor `Sync`, and egui's paint
/// callback needs both. It is only ever used inside that callback, which runs on the UI thread
/// that owns the OpenGL context and created it.
struct GlRender(RenderContext<'static>);
unsafe impl Send for GlRender {}
unsafe impl Sync for GlRender {}

fn get_proc_address(loader: &GlLoader, name: &str) -> *mut c_void {
    CString::new(name).map_or(std::ptr::null_mut(), |name| loader(&name).cast_mut())
}

pub struct MpvView {
    mpv: &'static Mpv,
    render: Arc<GlRender>,
    /// The link mpv was told to load, until `stop`.
    loaded: Option<String>,
    /// mpv has opened the file, so it can seek and knows the duration.
    file_loaded: bool,
    /// The newest `SeekRequest::id` done for the loaded file.
    applied_seek: u64,
    /// What was last sent to `Core`.
    last_report: Option<Instant>,
    last_buffering: bool,
    /// How many of `Playback::subtitles` were handed to the subtitle worker for this file.
    subtitles_added: usize,
    /// Grows with every load and stop, so the subtitle worker can drop what was meant for an
    /// earlier file.
    file_generation: Arc<AtomicU64>,
    subtitle_worker: mpsc::Sender<SubtitleJob>,
}

impl MpvView {
    /// Starts mpv on the window's OpenGL context. Call it from `eframe`'s creation callback,
    /// where that context is current. Fails when libmpv cannot start.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let mut mpv = Mpv::with_initializer(|init| {
            // Only the render API draws; mpv must not open a window of its own.
            init.set_property("vo", "libmpv")?;
            // A bad link is reported as an event, and then the player is idle again.
            init.set_property("idle", "yes")?;
            init.set_property("keep-open", "no")?;
            Ok(())
        })
        .map_err(|error| format!("libmpv could not start: {error}"))?;

        // An mpv event, such as a file that cannot be loaded, is handled on the next frame,
        // and there may be no frame coming.
        let wake = cc.egui_ctx.clone();
        mpv.set_wakeup_callback(move || wake.request_repaint());

        // One mpv lives as long as the process, so it is never freed. The render context
        // borrows it, and freeing a render context needs the OpenGL context current, which
        // is not guaranteed while the app shuts down.
        let mpv: &'static Mpv = Box::leak(Box::new(mpv));

        let loader = cc
            .get_proc_address
            .clone()
            .ok_or("the window has no OpenGL context")?;
        let loader: GlLoader = Arc::new(move |name: &CStr| loader(name));
        let mut render = mpv
            .create_render_context([
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address,
                    ctx: loader,
                }),
            ])
            .map_err(|error| format!("libmpv could not use OpenGL: {error}"))?;
        // A new video frame is ready.
        let repaint = cc.egui_ctx.clone();
        render.set_update_callback(move || repaint.request_repaint());

        let file_generation = Arc::new(AtomicU64::new(0));
        let subtitle_worker = spawn_subtitle_worker(mpv, file_generation.clone())?;

        Ok(MpvView {
            mpv,
            render: Arc::new(GlRender(render)),
            loaded: None,
            file_loaded: false,
            applied_seek: 0,
            last_report: None,
            last_buffering: false,
            subtitles_added: 0,
            file_generation,
            subtitle_worker,
        })
    }

    /// The audio and subtitle tracks mpv has for the loaded file. A subtitle track whose file
    /// is one of `addon_subtitles` is labelled "language · addon name" and listed last.
    pub fn tracks(&self, addon_subtitles: &[SubtitleTrack]) -> Tracks {
        let mut tracks = Tracks::default();
        let mut from_addons = Vec::new();
        let count = self
            .mpv
            .get_property::<i64>("track-list/count")
            .unwrap_or(0);
        for index in 0..count {
            let key = |name: &str| format!("track-list/{index}/{name}");
            let text = |name: &str| {
                self.mpv
                    .get_property::<String>(&key(name))
                    .ok()
                    .filter(|text| !text.is_empty())
            };
            let (Some(kind), Ok(id)) = (text("type"), self.mpv.get_property::<i64>(&key("id")))
            else {
                continue;
            };
            let selected = self
                .mpv
                .get_property::<bool>(&key("selected"))
                .unwrap_or(false);
            let label = track_label(id, text("lang"), text("title"));
            match kind.as_str() {
                "audio" => tracks.audio.push(Track {
                    id,
                    label,
                    selected,
                }),
                "sub" => {
                    let file = text("external-filename");
                    match addon_subtitles
                        .iter()
                        .find(|track| file.as_deref() == Some(track.url.as_str()))
                    {
                        Some(track) => from_addons.push(Track {
                            id,
                            label: addon_label(track),
                            selected,
                        }),
                        None => tracks.subtitles.push(Track {
                            id,
                            label,
                            selected,
                        }),
                    }
                }
                _ => {}
            }
        }
        tracks.subtitles.extend(from_addons);
        tracks
    }

    pub fn select_audio(&self, id: i64) {
        let _ = self.mpv.set_property("aid", id);
    }

    /// `None` hides the subtitles.
    pub fn select_subtitle(&self, id: Option<i64>) {
        let _ = match id {
            Some(id) => self.mpv.set_property("sid", id),
            None => self.mpv.set_property("sid", "no"),
        };
    }

    /// Draws the current video frame into `rect`, which must be the whole window: mpv draws
    /// into the framebuffer egui has bound, using the viewport egui set up.
    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        let render = self.render.clone();
        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                let gl = painter.gl();
                let viewport = info.viewport_in_pixels();
                // egui has already bound its target framebuffer; mpv draws into whichever it is.
                let fbo = unsafe { gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING) };
                // mpv wants its update flags read before every render call.
                let _ = render.0.update();
                let _ = render
                    .0
                    .render::<()>(fbo, viewport.width_px, viewport.height_px, true);
            })),
        });
    }

    /// Makes mpv match `playback`, and reports back to `core`. Called every frame while
    /// something is playing. A new file starts with the languages in `settings`.
    pub fn sync(
        &mut self,
        ctx: &egui::Context,
        playback: &Playback,
        settings: &Settings,
        core: &Core,
    ) {
        if self.loaded.as_deref() != Some(playback.url.as_str())
            && let Err(error) = self.load(playback, settings)
        {
            // mpv refused the command itself, so no event will ever say so.
            self.send_report(core, false, false, Some(format!("mpv: {error}")));
            return;
        }
        let mut failed = None;
        let mut ended = false;
        let mut just_loaded = false;
        while let Some(event) = self.mpv.wait_event(0.0) {
            match event {
                Ok(Event::FileLoaded) => {
                    self.file_loaded = true;
                    just_loaded = true;
                }
                Ok(Event::EndFile(reason)) if reason == mpv_end_file_reason::Eof => ended = true,
                // A stop or a replacing load, done by `load` or `stop` below.
                Ok(_) => {}
                // The only event that fails during playback is the end of a file that could
                // not be played.
                Err(error) => failed = Some(format!("mpv: {error}")),
            }
        }
        if failed.is_some() || ended {
            self.send_report(core, false, ended, failed);
            return;
        }

        if self.paused() != playback.paused {
            let _ = self.mpv.set_property("pause", playback.paused);
        }
        if self.file_loaded
            && let Some(seek) = &playback.seek
            && seek.id > self.applied_seek
        {
            self.applied_seek = seek.id;
            let _ = self
                .mpv
                .command("seek", &[&seek.position_secs.to_string(), "absolute"]);
        }
        if self.file_loaded {
            self.add_subtitles(playback);
        }

        let buffering = !self.file_loaded || self.flag("paused-for-cache");
        let due = self
            .last_report
            .is_none_or(|at| !playback.paused && at.elapsed() >= REPORT_INTERVAL);
        if just_loaded || buffering != self.last_buffering || due {
            self.last_buffering = buffering;
            self.send_report(core, buffering, false, None);
        }
        if !playback.paused {
            // Audio alone draws no frames, but the position still has to be reported.
            ctx.request_repaint_after(REPORT_INTERVAL);
        }
    }

    /// Unloads the file. Called when `State.playback` is `None`. Does nothing when nothing is
    /// loaded.
    pub fn stop(&mut self) {
        if self.loaded.take().is_some() {
            let _ = self.mpv.command("stop", &[]);
            self.drain_events();
            self.forget_file();
        }
    }

    /// Starts over for the next file.
    fn forget_file(&mut self) {
        self.file_loaded = false;
        self.applied_seek = 0;
        self.last_report = None;
        self.last_buffering = false;
        self.subtitles_added = 0;
        self.file_generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Hands the **addon** subtitles mpv does not have yet to the subtitle worker. Only once
    /// the file is loaded, since mpv cannot add a subtitle to a file it has not opened.
    fn add_subtitles(&mut self, playback: &Playback) {
        let Some(new) = playback.subtitles.get(self.subtitles_added..) else {
            return;
        };
        let file = self.file_generation.load(Ordering::SeqCst);
        for track in new {
            let _ = self.subtitle_worker.send(SubtitleJob {
                file,
                url: track.url.clone(),
                lang: track.lang.clone(),
            });
        }
        self.subtitles_added = playback.subtitles.len();
    }

    /// Tells `Core` what mpv is doing now.
    fn send_report(&mut self, core: &Core, buffering: bool, ended: bool, failed: Option<String>) {
        self.last_report = Some(Instant::now());
        core.send(Command::ReportPlayback(PlaybackReport {
            position_secs: self.number("time-pos"),
            duration_secs: self.number("duration"),
            buffering,
            ended,
            failed,
        }));
    }

    /// Forgets events that are still queued, so that ones about the old file are not taken
    /// for ones about the next.
    fn drain_events(&self) {
        while self.mpv.wait_event(0.0).is_some() {}
    }

    /// Loads the link. Fails only when mpv refuses the command; a link it cannot open comes
    /// back later as an event. The error never holds the link.
    fn load(&mut self, playback: &Playback, settings: &Settings) -> libmpv2::Result<()> {
        self.drain_events();
        self.forget_file();
        let _ = self.mpv.set_property("pause", playback.paused);
        self.prefer_languages(settings);
        // Remembered even on failure, so it is not retried every frame.
        self.loaded = Some(playback.url.clone());
        self.mpv.command("loadfile", &[&playback.url, "replace"])
    }

    /// Sets the tracks mpv picks for the next file. Done before every load, because a track
    /// picked by hand from a menu would otherwise carry over to the next file. With no
    /// subtitle language, subtitles start off.
    fn prefer_languages(&self, settings: &Settings) {
        let audio = settings.audio_lang.as_deref().map(mpv_languages);
        let _ = self.mpv.set_property("alang", audio.unwrap_or_default());
        let _ = self.mpv.set_property("aid", "auto");
        match settings.subtitle_lang.as_deref() {
            Some(code) => {
                let _ = self.mpv.set_property("slang", mpv_languages(code));
                let _ = self.mpv.set_property("sid", "auto");
            }
            None => {
                let _ = self.mpv.set_property("slang", "");
                let _ = self.mpv.set_property("sid", "no");
            }
        }
    }

    fn paused(&self) -> bool {
        self.flag("pause")
    }

    fn flag(&self, name: &str) -> bool {
        self.mpv.get_property::<bool>(name).unwrap_or(false)
    }

    /// A number property, or zero while mpv has none (before a file is loaded).
    fn number(&self, name: &str) -> f64 {
        self.mpv
            .get_property::<f64>(name)
            .ok()
            .filter(|value| value.is_finite())
            .unwrap_or(0.0)
    }
}

/// Adds **addon** subtitles to mpv on a thread of its own. `sub-add` downloads the file before
/// it returns, and a subtitles **addon** can offer dozens, so doing it on the UI thread would
/// freeze the window. A subtitle meant for a file that is no longer loaded is dropped.
fn spawn_subtitle_worker(
    mpv: &'static Mpv,
    file_generation: Arc<AtomicU64>,
) -> Result<mpsc::Sender<SubtitleJob>, String> {
    let (sender, jobs) = mpsc::channel::<SubtitleJob>();
    std::thread::Builder::new()
        .name("mpv-subtitles".into())
        .spawn(move || {
            for job in jobs {
                if job.file != file_generation.load(Ordering::SeqCst) {
                    continue;
                }
                // `auto` adds it without selecting it. Its title and language are both the
                // language the **addon** gave. A file mpv cannot read is simply not added.
                let _ = mpv.command("sub-add", &[&job.url, "auto", &job.lang, &job.lang]);
            }
        })
        .map_err(|error| format!("the subtitle thread could not start: {error}"))?;
    Ok(sender)
}

/// A track inside the file: "language · title", either one alone, or its number.
fn track_label(id: i64, lang: Option<String>, title: Option<String>) -> String {
    match (lang, title) {
        (Some(lang), Some(title)) => format!("{lang} · {title}"),
        (Some(text), None) | (None, Some(text)) => text,
        (None, None) => format!("Track {id}"),
    }
}

/// A track supplied by an **addon**: "language · addon name".
fn addon_label(track: &SubtitleTrack) -> String {
    let lang = if track.lang.is_empty() {
        "Unknown language"
    } else {
        &track.lang
    };
    format!("{lang} · {}", track.addon_name)
}
