//! The sign-in screen, shown in place of everything else until someone is signed in to an
//! **account**. Everything here is drawing; signing in is a `Command`.

use eframe::egui;
use streem_core::{Command, Core, Session, State};

const FIELD_WIDTH: f32 = 280.0;

#[derive(Default)]
pub struct SignInScreen {
    email: String,
    password: String,
}

impl SignInScreen {
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.heading("Streem");
            ui.add_space(16.0);
            match &state.session {
                Session::NotConfigured => {
                    ui.label("The sync backend is not configured. See ticket 14.");
                }
                Session::SignedIn { .. } => {}
                Session::Working => {
                    self.form(ui, false);
                    ui.add_space(8.0);
                    ui.spinner();
                }
                Session::SignedOut { error } => {
                    if let Some(command) = self.form(ui, true) {
                        core.send(command);
                    }
                    if let Some(error) = error {
                        ui.add_space(8.0);
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                }
            }
        });
    }

    /// The fields and buttons. Returns the command to send when the person submitted.
    fn form(&mut self, ui: &mut egui::Ui, enabled: bool) -> Option<Command> {
        let mut sign_in = false;
        let mut sign_up = false;
        ui.add_enabled_ui(enabled, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.email)
                    .hint_text("Email")
                    .desired_width(FIELD_WIDTH),
            );
            let password = ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .hint_text("Password")
                    .desired_width(FIELD_WIDTH),
            );
            let ready = !self.email.trim().is_empty() && !self.password.is_empty();
            let enter =
                password.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            sign_in = ready && enter;
            ui.add_space(8.0);
            sign_in |= ui
                .add_enabled(ready, egui::Button::new("Sign in"))
                .clicked();
            sign_up = ui
                .add_enabled(ready, egui::Button::new("Create account"))
                .clicked();
        });
        if !sign_in && !sign_up {
            return None;
        }
        let email = self.email.trim().to_string();
        // Not kept on screen once it is sent.
        let password = std::mem::take(&mut self.password);
        Some(if sign_up {
            Command::SignUp { email, password }
        } else {
            Command::SignIn { email, password }
        })
    }
}
