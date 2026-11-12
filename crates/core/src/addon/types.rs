//! Addon protocol types.
//!
//! The shapes follow `stremio-core` (<https://github.com/Stremio/stremio-core>, MIT licence),
//! `src/types/addon/manifest.rs`: `description` and `idPrefixes` are optional, `catalogs` may be
//! missing, and `types` and `resources` are required. We write our own structs instead of
//! depending on that crate. Only the fields this app uses are read; unknown fields are ignored.
//! The raw manifest text is what gets stored, so nothing is lost.

use serde::{Deserialize, Serialize};

/// The `manifest.json` of an **addon**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Types the **addon** supports as a whole, such as `movie` or `series`.
    pub types: Vec<String>,
    pub resources: Vec<ManifestResource>,
    #[serde(default)]
    pub catalogs: Vec<ManifestCatalog>,
    #[serde(default)]
    pub id_prefixes: Option<Vec<String>>,
}

/// A `resources` entry: either a bare name (`"catalog"`) or an object with its own limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ManifestResource {
    Short(String),
    #[serde(rename_all = "camelCase")]
    Full {
        name: String,
        #[serde(default)]
        types: Option<Vec<String>>,
        #[serde(default)]
        id_prefixes: Option<Vec<String>>,
    },
}

impl ManifestResource {
    pub fn name(&self) -> &str {
        match self {
            ManifestResource::Short(name) | ManifestResource::Full { name, .. } => name,
        }
    }
}

impl Manifest {
    /// True when this **addon** can answer a request for `resource` (such as `meta` or
    /// `stream`) with this type and id: it lists the resource, and the type and the start of
    /// the id are within its limits. A resource given as an object carries its own limits; a
    /// plain name uses the manifest's.
    pub fn offers(&self, resource_name: &str, meta_type: &str, id: &str) -> bool {
        self.resources.iter().any(|resource| {
            if resource.name() != resource_name {
                return false;
            }
            let (types, id_prefixes) = match resource {
                ManifestResource::Short(_) => (None, None),
                ManifestResource::Full {
                    types, id_prefixes, ..
                } => (types.as_ref(), id_prefixes.as_ref()),
            };
            let types = types.unwrap_or(&self.types);
            let id_prefixes = id_prefixes.or(self.id_prefixes.as_ref());
            types.iter().any(|offered| offered == meta_type)
                && id_prefixes
                    .is_none_or(|prefixes| prefixes.iter().any(|prefix| id.starts_with(prefix)))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestCatalog {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub extra: Vec<ManifestExtra>,
}

impl ManifestCatalog {
    /// What the home screen shows: the catalog's name, or its id when it has none.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// The home screen can only show a catalog that can be requested with no extra properties.
    pub fn has_required_extra(&self) -> bool {
        self.extra
            .iter()
            .any(|extra| extra.is_required == Some(true))
    }

    /// A catalog can be searched when it declares the `search` extra and needs no other extra.
    pub fn supports_search(&self) -> bool {
        let mut search = false;
        for extra in &self.extra {
            if extra.name == "search" {
                search = true;
            } else if extra.is_required == Some(true) {
                return false;
            }
        }
        search
    }
}

/// An `extra` entry of a catalog, such as `skip`, `genre` or `search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestExtra {
    /// Lenient on purpose: an odd `extra` entry must not make the whole manifest unreadable.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_required: Option<bool>,
}

/// One entry of a catalog response: enough to draw a poster.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MetaPreview {
    pub id: String,
    #[serde(rename = "type")]
    pub meta_type: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
}

/// The full description of a title, from a meta request.
///
/// Lenient on purpose: an entry that cannot be read is left out of a list, and a missing
/// `id` or `type` is filled in from the request by the client.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "type")]
    pub meta_type: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "releaseInfo")]
    pub release_info: Option<String>,
    /// **Addons** send it as text (`"8.8"`) or as a number.
    #[serde(default, rename = "imdbRating", deserialize_with = "text_or_number")]
    pub rating: Option<String>,
    #[serde(default, deserialize_with = "lenient_list")]
    pub genres: Vec<String>,
    #[serde(default, deserialize_with = "lenient_list")]
    pub videos: Vec<Video>,
}

/// An episode of a series, or the one playable item of a movie.
///
/// Only the `id` is required, because without it the video cannot be told apart or played.
/// Every other field is read leniently, so one odd value (a `season` sent as text, both
/// `title` and `name` present) never costs the episode its place in the list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "RawVideo")]
pub struct Video {
    pub id: String,
    pub title: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub released: Option<String>,
}

/// The wire shape of a [`Video`]: `title` is called `name` by older **addons**.
#[derive(Deserialize)]
struct RawVideo {
    id: String,
    #[serde(default, deserialize_with = "text_or_number")]
    title: Option<String>,
    #[serde(default, deserialize_with = "text_or_number")]
    name: Option<String>,
    #[serde(default, deserialize_with = "number_or_text")]
    season: Option<u32>,
    #[serde(default, deserialize_with = "number_or_text")]
    episode: Option<u32>,
    #[serde(default, deserialize_with = "text_or_number")]
    released: Option<String>,
}

impl From<RawVideo> for Video {
    fn from(raw: RawVideo) -> Self {
        Video {
            id: raw.id,
            title: raw.title.or(raw.name).unwrap_or_default(),
            season: raw.season,
            episode: raw.episode,
            released: raw.released,
        }
    }
}

/// One entry of a stream response: something an **addon** offers to play for one video.
///
/// Read leniently like [`Video`]: an entry is only skipped when it is not a JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "RawStream")]
pub struct Stream {
    pub name: String,
    /// Called `title` by older **addons**.
    pub description: String,
    /// `behaviorHints.filename`, or else the last part of the URL when it looks like a file.
    pub filename: Option<String>,
    pub source: StreamSource,
}

/// One entry of a subtitles response: a subtitle file an **addon** offers for one video.
///
/// Only the `url` is required; an entry without one is skipped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Subtitle {
    pub url: String,
    #[serde(default, deserialize_with = "text_or_number")]
    pub lang: Option<String>,
}

/// Where a [`Stream`] is played from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamSource {
    /// A link, of any scheme.
    Url(String),
    /// A torrent, which the app does not play: it has no torrent engine.
    Torrent,
    /// Anything else, such as `ytId` or `externalUrl`.
    Other,
}

/// The wire shape of a [`Stream`].
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStream {
    #[serde(default, deserialize_with = "text_or_number")]
    name: Option<String>,
    #[serde(default, deserialize_with = "text_or_number")]
    description: Option<String>,
    #[serde(default, deserialize_with = "text_or_number")]
    title: Option<String>,
    #[serde(default, deserialize_with = "text_or_number")]
    url: Option<String>,
    #[serde(default, deserialize_with = "text_or_number")]
    info_hash: Option<String>,
    #[serde(default)]
    behavior_hints: Option<serde_json::Value>,
}

impl From<RawStream> for Stream {
    fn from(raw: RawStream) -> Self {
        let hinted_filename = raw
            .behavior_hints
            .as_ref()
            .and_then(|hints| hints.get("filename"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let filename = hinted_filename.or_else(|| raw.url.as_deref().and_then(filename_in_url));
        // An `infoHash` makes it a torrent, even next to a `magnet:` link, unless the
        // **addon** also gives an `http(s)` link that can be played instead.
        let source = match (raw.url, raw.info_hash) {
            (Some(url), _) if is_http(&url) => StreamSource::Url(url),
            (_, Some(_)) => StreamSource::Torrent,
            (Some(url), None) => StreamSource::Url(url),
            (None, None) => StreamSource::Other,
        };
        Stream {
            name: raw.name.unwrap_or_default(),
            description: raw.description.or(raw.title).unwrap_or_default(),
            filename,
            source,
        }
    }
}

/// The last segment of the URL's path, percent-decoded, when it has a file extension
/// (`My%20Film.mkv` gives `My Film.mkv`). The query string is not part of it.
///
/// The extension is read before decoding, so an encoded `?` or `#` cannot end up in it, and it
/// must be letters and digits with at least one letter, so `v1.2` is not a file.
fn filename_in_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.next_back()?;
    let (stem, extension) = last.rsplit_once('.')?;
    let is_extension = extension.chars().all(|c| c.is_ascii_alphanumeric())
        && extension.chars().any(|c| c.is_ascii_alphabetic());
    if stem.is_empty() || !is_extension {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(last)
        .decode_utf8()
        .ok()?;
    Some(decoded.into_owned())
}

/// Only `http://` and `https://` links can be played.
pub(crate) fn is_http(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn text_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::String(text)) => Some(text),
            Some(serde_json::Value::Number(number)) => Some(number.to_string()),
            _ => None,
        },
    )
}

/// A whole number sent as a number (`3`) or as text (`"3"`); anything else is `None`.
fn number_or_text<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::Number(number)) => {
                number.as_u64().and_then(|n| u32::try_from(n).ok())
            }
            Some(serde_json::Value::String(text)) => text.trim().parse().ok(),
            _ => None,
        },
    )
}

/// A list where an entry that cannot be read is skipped, and `null` is an empty list.
///
/// Entries are only skipped when they are unusable as a whole (for a [`Video`]: no `id`);
/// a single odd field is handled by the entry's own type instead.
fn lenient_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let values = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_resource_forms() {
        let json = r#"{
            "id": "org.example", "version": "1.2.3", "name": "Example",
            "description": "An example addon",
            "types": ["movie"],
            "resources": ["catalog", {"name": "meta", "types": ["movie"], "idPrefixes": ["tt"]}],
            "catalogs": [{"type": "movie", "id": "top"}],
            "idPrefixes": ["tt"],
            "somethingNew": true
        }"#;
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "Example");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.resources.len(), 2);
        assert!(matches!(manifest.resources[0], ManifestResource::Short(_)));
        assert_eq!(manifest.resources[1].name(), "meta");
        assert!(matches!(
            manifest.resources[1],
            ManifestResource::Full { .. }
        ));
        assert_eq!(manifest.catalogs[0].kind, "movie");
    }

    #[test]
    fn optional_fields_may_be_missing() {
        let json = r#"{"id":"a","version":"0.1.0","name":"A","types":[],"resources":["stream"]}"#;
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert!(manifest.description.is_none());
        assert!(manifest.catalogs.is_empty());
        assert!(manifest.id_prefixes.is_none());
    }

    #[test]
    fn a_video_with_odd_optional_fields_is_kept() {
        let json = r#"{"name": "Show", "videos": [
            {"id": "a", "title": "Both", "name": "Ignored", "season": "2", "episode": 4},
            {"id": "b", "name": "Older", "season": null, "episode": "x"},
            {"title": "No id"}
        ]}"#;
        let meta: Meta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.videos.len(), 2);
        assert_eq!(meta.videos[0].title, "Both");
        assert_eq!(meta.videos[0].season, Some(2));
        assert_eq!(meta.videos[0].episode, Some(4));
        assert_eq!(meta.videos[1].title, "Older");
        assert_eq!(meta.videos[1].season, None);
        assert_eq!(meta.videos[1].episode, None);
    }

    #[test]
    fn rejects_a_manifest_without_a_name() {
        let json = r#"{"id":"a","version":"0.1.0","types":[],"resources":[]}"#;
        assert!(serde_json::from_str::<Manifest>(json).is_err());
    }
}
