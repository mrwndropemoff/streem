//! Settings: the Real-Debrid key that **addons** are called with, and the languages the player
//! prefers. Everything here is drawing; saving is a `Command`.

use eframe::egui;
use streem_core::{Command, Core, State};

/// The languages offered: ISO 639-2 code as saved, other codes a file may tag the same
/// language with, and the name shown.
const LANGUAGES: &[(&str, &str, &str)] = &[
    ("eng", "en", "English"),
    ("fre", "fra,fr", "French"),
    ("ger", "deu,de", "German"),
    ("spa", "es", "Spanish"),
    ("ita", "it", "Italian"),
    ("por", "pt", "Portuguese"),
    ("dut", "nld,nl", "Dutch"),
    ("pol", "pl", "Polish"),
    ("rus", "ru", "Russian"),
    ("tur", "tr", "Turkish"),
    ("ara", "ar", "Arabic"),
    ("hin", "hi", "Hindi"),
    ("jpn", "ja", "Japanese"),
    ("kor", "ko", "Korean"),
    ("chi", "zho,zh", "Chinese"),
];

/// What mpv's `alang` and `slang` take for a saved code: the code and the other codes of the
/// same language, so a track tagged `en` matches `eng`.
pub fn mpv_languages(code: &str) -> String {
    match LANGUAGES.iter().find(|(saved, _, _)| *saved == code) {
        Some((code, others, _)) => format!("{code},{others}"),
        None => code.to_string(),
    }
}

fn language_name(code: Option<&str>) -> &str {
    match code {
        None => "None",
        Some(code) => LANGUAGES
            .iter()
            .find(|(saved, _, _)| *saved == code)
            .map_or(code, |(_, _, name)| name),
    }
}

#[derive(Default)]
pub struct SettingsScreen {
    key_input: String,
    show_key: bool,
    /// The saved key the field was last filled from. When the saved key changes, the field
    /// shows the new one.
    filled_from: Option<Option<String>>,
}

impl SettingsScreen {
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, core: &Core) {
        let saved_key = state.settings.key().map(str::to_string);
        if self.filled_from.as_ref() != Some(&saved_key) {
            self.key_input = saved_key.clone().unwrap_or_default();
            self.filled_from = Some(saved_key.clone());
        }

        ui.heading("Settings");
        ui.add_space(8.0);

        ui.label(egui::RichText::new("Real-Debrid key").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.key_input)
                    .password(!self.show_key)
                    .hint_text("Paste your Real-Debrid API key")
                    .desired_width(360.0),
            );
            let toggle = if self.show_key { "Hide" } else { "Show" };
            if ui.button(toggle).clicked() {
                self.show_key = !self.show_key;
            }
            let typed = self.key_input.trim();
            let save = egui::Button::new("Save");
            if ui
                .add_enabled(
                    !typed.is_empty() && Some(typed) != saved_key.as_deref(),
                    save,
                )
                .clicked()
            {
                core.send(Command::SetRealDebridKey(Some(typed.to_string())));
            }
            let clear = egui::Button::new("Clear");
            if ui
                .add_enabled(saved_key.is_some() || !typed.is_empty(), clear)
                .clicked()
            {
                self.key_input.clear();
                core.send(Command::SetRealDebridKey(None));
            }
        });
        ui.weak(if saved_key.is_some() {
            "A key is saved."
        } else {
            "No key saved."
        });
        ui.label("Put {real_debrid_key} in an addon's URL where its Real-Debrid key goes.");
        ui.add_space(16.0);

        ui.label(egui::RichText::new("Playback languages").strong());
        let mut audio = state.settings.audio_lang.clone();
        let mut subtitle = state.settings.subtitle_lang.clone();
        egui::Grid::new("languages")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Audio");
                language_menu(ui, "audio-language", &mut audio);
                ui.end_row();
                ui.label("Subtitles");
                language_menu(ui, "subtitle-language", &mut subtitle);
                ui.end_row();
            });
        ui.weak("With subtitles set to None, they start off.");
        if audio != state.settings.audio_lang || subtitle != state.settings.subtitle_lang {
            core.send(Command::SetPlaybackLanguages { audio, subtitle });
        }
    }
}

/// A dropdown of `LANGUAGES` with "None" first.
fn language_menu(ui: &mut egui::Ui, id_salt: &str, selected: &mut Option<String>) {
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(language_name(selected.as_deref()))
        .width(180.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, None, "None");
            for (code, _, name) in LANGUAGES {
                ui.selectable_value(selected, Some(code.to_string()), *name);
            }
        });
}
