//! The `{real_debrid_key}` placeholder in an **addon** URL.
//!
//! The stored **addon** URL is a template: it is what `addons.url` holds and what
//! `InstalledAddon.url` shows. Before any request the placeholder is replaced with the
//! **debrid service** key. The result holds the key, so it is never stored, logged or put in
//! `State`.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::settings::Settings;

/// Where an **addon**'s Real-Debrid key goes in its URL.
pub const PLACEHOLDER: &str = "{real_debrid_key}";

/// Every spelling of the placeholder that is accepted, matched without regard to case. A URL
/// parser turns `{` and `}` into `%7B` and `%7D`, so a template may come back that way.
const SPELLINGS: [&str; 2] = [PLACEHOLDER, "%7Breal_debrid_key%7D"];

/// What stays unencoded in a key: the characters a URL never needs to escape.
const KEY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// The template has a placeholder and no Real-Debrid key is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Needs a Real-Debrid key. Add one in Settings.")]
pub struct MissingKey;

/// The URL to send requests to: the template with every placeholder replaced by the
/// percent-encoded key. A template without a placeholder is returned as it is, whatever the
/// settings are.
pub fn resolve(template: &str, settings: &Settings) -> Result<String, MissingKey> {
    if !has_placeholder(template) {
        return Ok(template.to_string());
    }
    let key = settings.key().ok_or(MissingKey)?;
    let encoded: String = utf8_percent_encode(key, KEY_ENCODE_SET).collect();
    Ok(replace_placeholders(template, &encoded))
}

/// True when the template has a placeholder in any accepted spelling.
pub fn has_placeholder(template: &str) -> bool {
    template
        .char_indices()
        .any(|(index, _)| placeholder_at(&template[index..]).is_some())
}

/// What to store for a pasted URL: with a key saved, every copy of that exact key is replaced
/// by the placeholder, so the stored URL never holds it.
pub fn templatize(url: &str, settings: &Settings) -> String {
    match settings.key() {
        Some(key) => url.replace(key, PLACEHOLDER),
        None => url.to_string(),
    }
}

/// The length of the placeholder that `text` starts with, if it starts with one.
fn placeholder_at(text: &str) -> Option<usize> {
    SPELLINGS
        .iter()
        .find(|spelling| {
            text.get(..spelling.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(spelling))
        })
        .map(|spelling| spelling.len())
}

fn replace_placeholders(template: &str, with: &str) -> String {
    let mut result = String::with_capacity(template.len() + with.len());
    let mut rest = template;
    while let Some(next) = rest.chars().next() {
        match placeholder_at(rest) {
            Some(length) => {
                result.push_str(with);
                rest = &rest[length..];
            }
            None => {
                result.push(next);
                rest = &rest[next.len_utf8()..];
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_key(key: &str) -> Settings {
        Settings {
            real_debrid_key: Some(key.to_string()),
            ..Settings::default()
        }
    }

    #[test]
    fn fills_the_placeholder() {
        let template =
            "https://torrentio.example/sort=size|realdebrid={real_debrid_key}/manifest.json";
        assert_eq!(
            resolve(template, &with_key("ABC123")).unwrap(),
            "https://torrentio.example/sort=size|realdebrid=ABC123/manifest.json"
        );
    }

    #[test]
    fn fills_the_percent_encoded_spelling() {
        for template in [
            "https://a.example/realdebrid=%7Breal_debrid_key%7D/manifest.json",
            "https://a.example/realdebrid=%7breal_debrid_key%7d/manifest.json",
        ] {
            assert_eq!(
                resolve(template, &with_key("ABC123")).unwrap(),
                "https://a.example/realdebrid=ABC123/manifest.json"
            );
        }
    }

    #[test]
    fn percent_encodes_the_key() {
        let template = "https://a.example/realdebrid={real_debrid_key}/manifest.json";
        assert_eq!(
            resolve(template, &with_key("ab/c d?e")).unwrap(),
            "https://a.example/realdebrid=ab%2Fc%20d%3Fe/manifest.json"
        );
    }

    #[test]
    fn a_template_without_a_placeholder_is_returned_unchanged() {
        let template = "https://a.example/manifest.json";
        assert_eq!(resolve(template, &Settings::default()).unwrap(), template);
        assert_eq!(resolve(template, &with_key("ABC123")).unwrap(), template);
    }

    #[test]
    fn a_placeholder_without_a_key_is_missing_key() {
        let template = "https://a.example/{real_debrid_key}/manifest.json";
        assert_eq!(resolve(template, &Settings::default()), Err(MissingKey));
        assert_eq!(resolve(template, &with_key("   ")), Err(MissingKey));
    }

    #[test]
    fn every_placeholder_is_filled() {
        let template = "https://a.example/{real_debrid_key}/%7Breal_debrid_key%7D/manifest.json";
        assert_eq!(
            resolve(template, &with_key("K")).unwrap(),
            "https://a.example/K/K/manifest.json"
        );
    }

    #[test]
    fn templatize_replaces_the_saved_key() {
        let pasted = "https://a.example/realdebrid=ABC123/manifest.json";
        assert_eq!(
            templatize(pasted, &with_key("ABC123")),
            "https://a.example/realdebrid={real_debrid_key}/manifest.json"
        );
        assert_eq!(templatize(pasted, &with_key("OTHER")), pasted);
        assert_eq!(templatize(pasted, &Settings::default()), pasted);
    }
}
