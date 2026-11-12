//! The player screen: the video fills the window and a control bar is drawn over it.
//!
//! Everything here is drawing and commands. The controls send `SetPaused`, `Seek` and
//! `StopPlayback` to `Core`, and show what `State.playback` says. The video itself is drawn by
//! the function the caller passes in.

use std::time::{Duration, Instant};

use eframe::egui;
use streem_core::{Command, Core, Playback, State};

/// The bar and the pointer disappear after this long without the mouse moving.
const HIDE_AFTER: Duration = Duration::from_secs(3);
/// How far the arrow keys jump.
const KEY_SEEK_SECS: f64 = 10.0;
/// Room beside the slider for the time label.
const TIME_LABEL_WIDTH: f32 = 170.0;

pub struct PlayerScreen {
    last_activity: Instant,
    /// Where the slider is while it is being dragged. Nothing is sent until it is let go.
    scrubbing: Option<f64>,
}

impl Default for PlayerScreen {
    fn default() -> Self {
        PlayerScreen {
            last_activity: Instant::now(),
            scrubbing: None,
        }
    }
}

impl PlayerScreen {
    /// Draws the video with `paint_video`, then the bar and the keyboard shortcuts.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &State,
        core: &Core,
        paint_video: impl FnOnce(&egui::Ui, egui::Rect),
    ) {
        let Some(playback) = &state.playback else {
            return;
        };
        let ctx = ui.ctx().clone();

        // Taken before any widget is drawn, so that a focused button or slider does not act
        // on the same key press a second time.
        self.keys(&ctx, playback, core);
        self.track_activity(&ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let rect = ui.max_rect();
                paint_video(ui, rect);
                if playback.buffering {
                    ui.put(
                        egui::Rect::from_center_size(rect.center(), egui::vec2(48.0, 48.0)),
                        egui::Spinner::new().size(48.0),
                    );
                }
            });

        if self.bar_is_visible() {
            self.bar(&ctx, playback, core);
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::None);
            self.scrubbing = None;
        }
    }

    /// Put the screen away: the next time it is shown, the bar is visible again.
    pub fn on_leave(&mut self) {
        self.last_activity = Instant::now();
        self.scrubbing = None;
    }

    fn keys(&mut self, ctx: &egui::Context, playback: &Playback, core: &Core) {
        let none = egui::Modifiers::NONE;
        let (space, left, right, escape, fullscreen) = ctx.input_mut(|input| {
            (
                input.consume_key(none, egui::Key::Space),
                input.consume_key(none, egui::Key::ArrowLeft),
                input.consume_key(none, egui::Key::ArrowRight),
                input.consume_key(none, egui::Key::Escape),
                input.key_pressed(egui::Key::F),
            )
        });
        if space {
            core.send(Command::SetPaused(!playback.paused));
        }
        if left {
            core.send(Command::Seek {
                position_secs: playback.position_secs - KEY_SEEK_SECS,
            });
        }
        if right {
            core.send(Command::Seek {
                position_secs: playback.position_secs + KEY_SEEK_SECS,
            });
        }
        if escape {
            core.send(Command::StopPlayback);
        }
        if fullscreen {
            // The real state, so that leaving fullscreen through the window manager cannot
            // leave this one wrong.
            let is_fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen));
        }
        if space || left || right || escape || fullscreen {
            self.last_activity = Instant::now();
        }
    }

    fn track_activity(&mut self, ctx: &egui::Context) {
        let moved = ctx.input(|input| {
            input.pointer.delta() != egui::Vec2::ZERO || input.pointer.any_pressed()
        });
        if moved {
            self.last_activity = Instant::now();
        }
        // Nothing else wakes the screen up to hide the bar while the video is paused.
        if let Some(left) = HIDE_AFTER.checked_sub(self.last_activity.elapsed()) {
            ctx.request_repaint_after(left);
        }
    }

    fn bar_is_visible(&self) -> bool {
        self.last_activity.elapsed() < HIDE_AFTER || self.scrubbing.is_some()
    }

    fn bar(&mut self, ctx: &egui::Context, playback: &Playback, core: &Core) {
        egui::Area::new(egui::Id::unique("player-bar"))
            .anchor(egui::Align2::LEFT_BOTTOM, [0.0, 0.0])
            .show(ctx, |ui| {
                let width = ctx.content_rect().width();
                egui::Frame::NONE
                    .fill(egui::Color32::from_black_alpha(170))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        ui.add(
                            egui::Label::new(egui::RichText::new(&playback.title).strong())
                                .truncate(),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("← Back").clicked() {
                                core.send(Command::StopPlayback);
                            }
                            let label = if playback.paused { "Play" } else { "Pause" };
                            if ui.button(label).clicked() {
                                core.send(Command::SetPaused(!playback.paused));
                            }
                            self.seek_slider(ui, playback, core);
                        });
                    });
            });
    }

    fn seek_slider(&mut self, ui: &mut egui::Ui, playback: &Playback, core: &Core) {
        let duration = playback.duration_secs;
        let mut position = self.scrubbing.unwrap_or(playback.position_secs);
        ui.spacing_mut().slider_width = (ui.available_width() - TIME_LABEL_WIDTH).max(80.0);
        let response =
            ui.add(egui::Slider::new(&mut position, 0.0..=duration.max(0.1)).show_value(false));
        if response.dragged() || response.changed() {
            self.scrubbing = Some(position);
        }
        if response.drag_stopped() {
            core.send(Command::Seek {
                position_secs: position,
            });
            self.scrubbing = None;
        }
        ui.label(format!("{} / {}", clock(position), clock(duration)));
    }
}

/// "mm:ss", or "h:mm:ss" from one hour on.
fn clock(secs: f64) -> String {
    let secs = secs.max(0.0) as u64;
    let (hours, minutes, seconds) = (secs / 3600, secs / 60 % 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_shows_hours_only_when_there_are_some() {
        assert_eq!(clock(0.0), "00:00");
        assert_eq!(clock(75.9), "01:15");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-4.0), "00:00");
    }
}
