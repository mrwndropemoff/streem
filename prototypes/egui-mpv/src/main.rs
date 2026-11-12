//! Throwaway: can egui host libmpv? mpv draws into egui's own GL context via the render API.
use std::{sync::Arc, time::Duration};

use eframe::{egui, egui_glow, glow, glow::HasContext};
use libmpv2::{
    Mpv,
    render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType},
};

/// RenderContext holds a raw pointer, so it is not Send/Sync. egui's paint callback demands both,
/// but the callback only ever runs on the UI/GL thread, which is where this is created and used.
struct GlRender(RenderContext<'static>);
unsafe impl Send for GlRender {}
unsafe impl Sync for GlRender {}

struct Track {
    id: i64,
    label: String,
    selected: bool,
}

struct App {
    mpv: &'static Mpv,
    render: Arc<GlRender>,
}

fn get_proc_address(f: &Arc<dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void + Send + Sync>, name: &str) -> *mut std::ffi::c_void {
    std::ffi::CString::new(name).map_or(std::ptr::null_mut(), |c| f(&c).cast_mut())
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, path: &str) -> Self {
        // shortcut: leaked so the render context can borrow it for 'static; process exit frees it. Fix when this ships.
        let mpv: &'static Mpv = Box::leak(Box::new(
            Mpv::with_initializer(|i| {
                i.set_property("vo", "libmpv")?;
                i.set_property("terminal", "yes")?; // libmpv is silent otherwise; we want error lines visible
                i.set_property("msg-level", "all=warn")?;
                i.set_property("keep-open", "yes")?;
                Ok(())
            })
            .expect("mpv init"),
        ));
        let gpa = cc.get_proc_address.clone().expect("not running on glow");
        let mut ctx = mpv
            .create_render_context([
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams { get_proc_address, ctx: Arc::new(move |n: &std::ffi::CStr| gpa(n)) as Arc<dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void + Send + Sync> }),
            ])
            .expect("mpv render context");
        let egui_ctx = cc.egui_ctx.clone();
        ctx.set_update_callback(move || egui_ctx.request_repaint());
        mpv.command("loadfile", &[path, "replace"]).expect("loadfile");

        // Own thread so the line still prints while paused (no repaints then). Mpv is Sync.
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let t = mpv.get_property::<f64>("time-pos").unwrap_or(f64::NAN);
            let s = |p: &str| mpv.get_property::<String>(p).unwrap_or_else(|_| "n/a".into());
            println!("time-pos={t:.1} paused={} aid={} sid={}", s("pause") == "yes", s("aid"), s("sid"));
        });
        Self { mpv, render: Arc::new(GlRender(ctx)) }
    }

    /// shortcut: re-read the track list every frame; fine for a handful of tracks, cache on file-loaded if it ever shows up in a profile.
    fn tracks(&self, kind: &str) -> Vec<Track> {
        let n = self.mpv.get_property::<i64>("track-list/count").unwrap_or(0);
        (0..n)
            .filter_map(|i| {
                let p = |k: &str| self.mpv.get_property::<String>(&format!("track-list/{i}/{k}")).ok();
                (p("type")? == kind).then(|| {
                    let id = p("id")?.parse().ok()?;
                    let name = [p("lang"), p("title")].into_iter().flatten().collect::<Vec<_>>().join(" ");
                    Some(Track { id, label: format!("{id}: {name}"), selected: p("selected")? == "yes" })
                })?
            })
            .collect()
    }

    fn track_combo(&self, ui: &mut egui::Ui, prop: &str, kind: &str, off_label: Option<&str>) {
        let tracks = self.tracks(kind);
        let current = tracks.iter().find(|t| t.selected).map_or(off_label.unwrap_or("-").to_owned(), |t| t.label.clone());
        egui::ComboBox::from_id_salt(prop).selected_text(current).show_ui(ui, |ui| {
            if off_label.is_some() && ui.selectable_label(!tracks.iter().any(|t| t.selected), "Off").clicked() {
                let _ = self.mpv.set_property(prop, "no");
            }
            for t in &tracks {
                if ui.selectable_label(t.selected, &t.label).clicked() {
                    let _ = self.mpv.set_property(prop, t.id);
                }
            }
        });
    }
}

fn mmss(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Video fills the window: a frameless panel with one paint callback.
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let rect = ui.max_rect();
            let render = self.render.clone();
            ui.painter().add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                    let gl = painter.gl();
                    let vp = info.viewport_in_pixels();
                    // egui has already bound its target framebuffer; draw into whichever it is.
                    let fbo = unsafe { gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING) };
                    // Core must re-read mpv's update flags before each render call.
                    let _ = render.0.update();
                    let _ = render.0.render::<()>(fbo, vp.width_px, vp.height_px, true);
                })),
            });
        });

        // Control bar: an Area anchored to the bottom, drawn over the video.
        egui::Area::new(egui::Id::unique("bar")).anchor(egui::Align2::LEFT_BOTTOM, [0.0, 0.0]).show(&ctx, |ui| {
            let w = ctx.content_rect().width();
            egui::Frame::NONE.fill(egui::Color32::from_black_alpha(170)).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(w - 16.0);
                ui.horizontal(|ui| {
                    let paused = self.mpv.get_property::<String>("pause").is_ok_and(|p| p == "yes");
                    if ui.button(if paused { "Play" } else { "Pause" }).clicked() {
                        let _ = self.mpv.command("cycle", &["pause"]); // atomic, unlike read-then-set
                    }
                    let dur = self.mpv.get_property::<f64>("duration").unwrap_or(0.0);
                    let mut pos = self.mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
                    ui.spacing_mut().slider_width = (w - 520.0).max(80.0);
                    if ui.add(egui::Slider::new(&mut pos, 0.0..=dur.max(0.1)).show_value(false)).changed() {
                        let _ = self.mpv.command("seek", &[&pos.to_string(), "absolute"]);
                    }
                    ui.label(format!("{} / {}", mmss(pos), mmss(dur)));
                    self.track_combo(ui, "aid", "audio", None);
                    self.track_combo(ui, "sid", "sub", Some("Off"));
                });
            });
        });

        // Only when no widget has focus, or a focused button would toggle twice.
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) && ctx.memory(|m| m.focused().is_none()) {
            let _ = self.mpv.command("cycle", &["pause"]);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F)) {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)); // real state, so a WM exit can't desync it
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
        }
    }
}

fn main() -> eframe::Result {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: egui-mpv-prototype <file or http(s) url>");
        std::process::exit(2);
    };
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow, // libmpv can only draw through OpenGL
        ..Default::default()
    };
    eframe::run_native("egui-mpv prototype", options, Box::new(move |cc| Ok(Box::new(App::new(cc, &path)))))
}
