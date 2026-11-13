//! The **account**'s settings: the **debrid service** key **addons** are called with, and the
//! languages the player prefers.

/// What the person set in Settings. Until sign-in exists there is one set, for the local
/// **account**.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// The Real-Debrid key, put into **addon** URLs in place of `{real_debrid_key}`. A secret:
    /// never log it or show it outside the Settings field.
    pub real_debrid_key: Option<String>,
    /// An ISO 639-2 code such as `eng`.
    pub audio_lang: Option<String>,
    /// An ISO 639-2 code such as `eng`. `None` means subtitles start off.
    pub subtitle_lang: Option<String>,
}

impl Settings {
    /// The Real-Debrid key, or `None` when it is unset or blank.
    pub fn key(&self) -> Option<&str> {
        self.real_debrid_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    }
}

// By hand, so that no `{:?}` of the state, in a log or a failed test, can print the key.
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("real_debrid_key", &self.key().map(|_| "<hidden>"))
            .field("audio_lang", &self.audio_lang)
            .field("subtitle_lang", &self.subtitle_lang)
            .finish()
    }
}

/// What was typed, without surrounding whitespace; blank becomes `None`.
pub(crate) fn non_blank(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_the_key() {
        let settings = Settings {
            real_debrid_key: Some("SECRET".into()),
            audio_lang: Some("eng".into()),
            subtitle_lang: None,
        };
        let printed = format!("{settings:?}");
        assert!(!printed.contains("SECRET"), "{printed}");
        assert!(printed.contains("<hidden>"), "{printed}");
        assert!(printed.contains("eng"), "{printed}");
    }

    #[test]
    fn a_blank_key_counts_as_no_key() {
        let settings = Settings {
            real_debrid_key: Some("  ".into()),
            ..Settings::default()
        };
        assert_eq!(settings.key(), None);
        assert_eq!(non_blank(Some("  abc ".into())), Some("abc".into()));
        assert_eq!(non_blank(Some(" ".into())), None);
    }
}
