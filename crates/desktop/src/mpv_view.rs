//! The libmpv video surface. It owns the one mpv instance and draws its video into egui's own
//! OpenGL context through libmpv's render API.
//!
//! It is the only place that talks to mpv, and it takes its orders from `State.playback`: every
//! frame [`MpvView::sync`] compares mpv with what `Core` says and corrects mpv, then tells `Core`
//! what mpv is doing with `Command::ReportPlayback`. The controls never come here.

use std::ffi::{CStr, CString, c_void};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::{egui, egui_glow, glow, glow::HasContext};
use libmpv2::Mpv;
use libmpv2::events::Event;
use libmpv2::mpv_end_file_reason;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use streem_core::{Command, Core, Playback, PlaybackReport};

/// While playing, `Core` hears about the position this often.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

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

        Ok(MpvView {
            mpv,
            render: Arc::new(GlRender(render)),
            loaded: None,
            file_loaded: false,
            applied_seek: 0,
            last_report: None,
            last_buffering: false,
        })
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
    /// something is playing.
    pub fn sync(&mut self, ctx: &egui::Context, playback: &Playback, core: &Core) {
        if self.loaded.as_deref() != Some(playback.url.as_str()) {
            self.load(playback);
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
            self.last_report = Some(Instant::now());
            core.send(Command::ReportPlayback(PlaybackReport {
                position_secs: self.number("time-pos"),
                duration_secs: self.number("duration"),
                buffering: false,
                ended,
                failed,
            }));
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

        let buffering = !self.file_loaded || self.flag("paused-for-cache");
        let due = self
            .last_report
            .is_none_or(|at| !playback.paused && at.elapsed() >= REPORT_INTERVAL);
        if just_loaded || buffering != self.last_buffering || due {
            self.last_buffering = buffering;
            self.last_report = Some(Instant::now());
            core.send(Command::ReportPlayback(PlaybackReport {
                position_secs: self.number("time-pos"),
                duration_secs: self.number("duration"),
                buffering,
                ended: false,
                failed: None,
            }));
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
            self.file_loaded = false;
            self.applied_seek = 0;
            self.last_report = None;
            self.last_buffering = false;
        }
    }

    /// Forgets events that are still queued, so that ones about the old file are not taken
    /// for ones about the next.
    fn drain_events(&self) {
        while self.mpv.wait_event(0.0).is_some() {}
    }

    fn load(&mut self, playback: &Playback) {
        self.drain_events();
        self.file_loaded = false;
        self.applied_seek = 0;
        self.last_report = None;
        self.last_buffering = false;
        let _ = self.mpv.set_property("pause", playback.paused);
        let loading = self.mpv.command("loadfile", &[&playback.url, "replace"]);
        // Remembered even when mpv refused the command, so it is not retried every frame.
        // mpv reports a link it cannot open as an event, not here.
        self.loaded = Some(playback.url.clone());
        if let Err(error) = loading {
            // Never print the link.
            eprintln!("mpv refused to load the stream: {error}");
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
