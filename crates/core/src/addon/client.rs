//! Talking to an **addon** over HTTP: its manifest, catalogs, metas, streams and subtitles.
//!
//! Every request starts from an [`ResolvedAddon`], and the only way to make one is
//! [`ResolvedAddon::resolve`], which fills the `{real_debrid_key}` placeholder. The URLs of the
//! requests are built here and nowhere else. They can hold a **debrid service** key, so no
//! error from this module contains a URL it requested.

use std::time::Duration;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::types::{Manifest, Meta, MetaPreview, Stream, Subtitle, Video};
use super::url::{self, MissingKey};
use crate::settings::Settings;

#[derive(Debug, thiserror::Error)]
pub enum AddonError {
    #[error(
        "\"{0}\" is not an addon URL; it must be an http(s) or stremio:// link ending in /manifest.json"
    )]
    InvalidUrl(String),
    #[error("the addon's URL cannot be used for a request")]
    UnusableUrl,
    #[error("could not reach the addon: {0}")]
    Network(reqwest::Error),
    #[error("the addon answered with HTTP status {0}")]
    Status(u16),
    #[error("the addon's manifest is not valid: {0}")]
    InvalidManifest(#[from] serde_json::Error),
    #[error("the addon's catalog is not valid: {0}")]
    InvalidCatalog(serde_json::Error),
    #[error("the addon's description of the title is not valid: {0}")]
    InvalidMeta(serde_json::Error),
    #[error("the addon's list of streams is not valid: {0}")]
    InvalidStreams(serde_json::Error),
    #[error("the addon's list of subtitles is not valid: {0}")]
    InvalidSubtitles(serde_json::Error),
}

impl From<reqwest::Error> for AddonError {
    /// Drops the URL, which can hold a **debrid service** key, from the error's text.
    fn from(error: reqwest::Error) -> Self {
        AddonError::Network(error.without_url())
    }
}

/// Turns what the user typed into the URL that identifies the **addon**.
/// A `stremio://` link becomes `https://`. Nothing else in the text is changed, so a
/// `{real_debrid_key}` placeholder stays as typed.
pub fn normalize_url(input: &str) -> Result<String, AddonError> {
    let trimmed = input.trim();
    let url = match trimmed.get(..10) {
        Some(scheme) if scheme.eq_ignore_ascii_case("stremio://") => {
            format!("https://{}", &trimmed[10..])
        }
        _ => trimmed.to_string(),
    };
    let parsed = reqwest::Url::parse(&url).map_err(|_| AddonError::InvalidUrl(trimmed.into()))?;
    let is_http = matches!(parsed.scheme(), "http" | "https");
    if !is_http || !parsed.path().ends_with("/manifest.json") {
        return Err(AddonError::InvalidUrl(trimmed.into()));
    }
    Ok(url)
}

/// Parses manifest text; the error says why it was rejected.
pub fn parse_manifest(text: &str) -> Result<Manifest, AddonError> {
    Ok(serde_json::from_str(text)?)
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("streem/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("the HTTP client settings are valid")
}

/// Where the requests to one **addon** go: its stored URL with the Real-Debrid key filled in.
/// It holds the key, so it has no `Debug` and is never stored, logged or shown.
#[derive(Clone)]
pub struct ResolvedAddon {
    manifest_url: String,
}

impl ResolvedAddon {
    /// Fills the placeholder in the stored **addon** URL with [`url::resolve`]. Fails when the
    /// URL has a placeholder and no key is saved; the **addon** is then skipped.
    pub fn resolve(template: &str, settings: &Settings) -> Result<ResolvedAddon, MissingKey> {
        Ok(ResolvedAddon {
            manifest_url: url::resolve(template, settings)?,
        })
    }
}

/// Sends a GET request and returns the body of a successful answer.
async fn get(client: &reqwest::Client, url: &str) -> Result<String, AddonError> {
    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AddonError::Status(status.as_u16()));
    }
    Ok(response.text().await?)
}

/// Downloads and checks a manifest. Returns it with the raw text, which is what gets stored.
pub async fn fetch_manifest(
    client: &reqwest::Client,
    addon: &ResolvedAddon,
) -> Result<(Manifest, String), AddonError> {
    let text = get(client, &addon.manifest_url).await?;
    let manifest = parse_manifest(&text)?;
    Ok((manifest, text))
}

/// The URL of a resource request: `{base}/{resource}/{type}/{id}.json`, where `{base}` is the
/// manifest URL without `/manifest.json`. `{type}` and `{id}` are percent-encoded. An `extra`
/// such as `("search", "the wire")` adds `/search=the%20wire` before `.json`; its value is
/// percent-encoded so that `&`, `=` or `/` in it cannot be read as part of the request.
fn resource_url(
    manifest_url: &str,
    resource: &str,
    meta_type: &str,
    id: &str,
    extra: Option<(&str, &str)>,
) -> Result<String, AddonError> {
    let mut url = reqwest::Url::parse(manifest_url).map_err(|_| AddonError::UnusableUrl)?;
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| AddonError::UnusableUrl)?
        .pop()
        .push(resource)
        .push(meta_type)
        .push(id);
    let mut url = String::from(url);
    if let Some((name, value)) = extra {
        url.push('/');
        url.extend(utf8_percent_encode(name, EXTRA_ENCODE_SET));
        url.push('=');
        url.extend(utf8_percent_encode(value, EXTRA_ENCODE_SET));
    }
    url.push_str(".json");
    Ok(url)
}

/// What stays unencoded in an extra: the characters JavaScript's `encodeURIComponent` leaves
/// alone, which is what Stremio itself sends.
const EXTRA_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// The URL of a catalog: `{base}/catalog/{type}/{id}.json`.
fn catalog_url(
    manifest_url: &str,
    meta_type: &str,
    catalog_id: &str,
) -> Result<String, AddonError> {
    resource_url(manifest_url, "catalog", meta_type, catalog_id, None)
}

/// The URL of a search in a catalog: `{base}/catalog/{type}/{id}/search={query}.json`, with the
/// query percent-encoded.
fn search_url(
    manifest_url: &str,
    meta_type: &str,
    catalog_id: &str,
    query: &str,
) -> Result<String, AddonError> {
    let extra = Some(("search", query));
    resource_url(manifest_url, "catalog", meta_type, catalog_id, extra)
}

/// Downloads the first page of a catalog, or with a `query` the results of a search in it. An
/// entry that lacks an id, type or name is skipped, so one bad entry does not hide the rest.
pub async fn fetch_catalog(
    client: &reqwest::Client,
    addon: &ResolvedAddon,
    meta_type: &str,
    catalog_id: &str,
    query: Option<&str>,
) -> Result<Vec<MetaPreview>, AddonError> {
    // shortcut: first page only, add the "skip" extra when rows need to scroll further
    #[derive(serde::Deserialize)]
    struct Response {
        metas: Vec<serde_json::Value>,
    }

    let url = match query {
        Some(query) => search_url(&addon.manifest_url, meta_type, catalog_id, query)?,
        None => catalog_url(&addon.manifest_url, meta_type, catalog_id)?,
    };
    let text = get(client, &url).await?;
    let parsed: Response = serde_json::from_str(&text).map_err(AddonError::InvalidCatalog)?;
    Ok(parsed
        .metas
        .into_iter()
        .filter_map(|meta| serde_json::from_value(meta).ok())
        .collect())
}

/// The URL of a meta request: `{base}/meta/{type}/{id}.json`.
fn meta_url(manifest_url: &str, meta_type: &str, meta_id: &str) -> Result<String, AddonError> {
    resource_url(manifest_url, "meta", meta_type, meta_id, None)
}

/// The URL of a stream request: `{base}/stream/{type}/{videoId}.json`. For a movie the video
/// id is the meta id; for an episode it is the video's own id, such as `tt0903747:1:2`.
fn stream_url(manifest_url: &str, meta_type: &str, video_id: &str) -> Result<String, AddonError> {
    resource_url(manifest_url, "stream", meta_type, video_id, None)
}

/// Downloads the streams an **addon** offers for one video. An entry that is not an object is
/// skipped, so one bad entry does not hide the rest.
pub async fn fetch_streams(
    client: &reqwest::Client,
    addon: &ResolvedAddon,
    meta_type: &str,
    video_id: &str,
) -> Result<Vec<Stream>, AddonError> {
    #[derive(serde::Deserialize)]
    struct Response {
        #[serde(default)]
        streams: Option<Vec<serde_json::Value>>,
    }

    let url = stream_url(&addon.manifest_url, meta_type, video_id)?;
    let text = get(client, &url).await?;
    let parsed: Response = serde_json::from_str(&text).map_err(AddonError::InvalidStreams)?;
    Ok(parsed
        .streams
        .unwrap_or_default()
        .into_iter()
        .filter_map(|stream| serde_json::from_value(stream).ok())
        .collect())
}

/// The URL of a subtitles request: `{base}/subtitles/{type}/{videoId}.json`, with the same
/// video id as a stream request.
fn subtitles_url(
    manifest_url: &str,
    meta_type: &str,
    video_id: &str,
) -> Result<String, AddonError> {
    resource_url(manifest_url, "subtitles", meta_type, video_id, None)
}

/// Downloads the subtitles an **addon** offers for one video. An entry without a `url` is
/// skipped, so one bad entry does not hide the rest.
pub async fn fetch_subtitles(
    client: &reqwest::Client,
    addon: &ResolvedAddon,
    meta_type: &str,
    video_id: &str,
) -> Result<Vec<Subtitle>, AddonError> {
    #[derive(serde::Deserialize)]
    struct Response {
        #[serde(default)]
        subtitles: Option<Vec<serde_json::Value>>,
    }

    let url = subtitles_url(&addon.manifest_url, meta_type, video_id)?;
    let text = get(client, &url).await?;
    let parsed: Response = serde_json::from_str(&text).map_err(AddonError::InvalidSubtitles)?;
    Ok(parsed
        .subtitles
        .unwrap_or_default()
        .into_iter()
        .filter_map(|subtitle| serde_json::from_value(subtitle).ok())
        .collect())
}

/// Downloads the description of one title. A movie that comes back with no videos gets one
/// whose id is the meta id, so there is always something to play.
pub async fn fetch_meta(
    client: &reqwest::Client,
    addon: &ResolvedAddon,
    meta_type: &str,
    meta_id: &str,
) -> Result<Meta, AddonError> {
    #[derive(serde::Deserialize)]
    struct Response {
        meta: Meta,
    }

    let url = meta_url(&addon.manifest_url, meta_type, meta_id)?;
    let text = get(client, &url).await?;
    let Response { mut meta } = serde_json::from_str(&text).map_err(AddonError::InvalidMeta)?;
    if meta.id.is_empty() {
        meta.id = meta_id.to_string();
    }
    if meta.meta_type.is_empty() {
        meta.meta_type = meta_type.to_string();
    }
    if meta.meta_type == "movie" && meta.videos.is_empty() {
        meta.videos.push(Video {
            id: meta.id.clone(),
            title: meta.name.clone(),
            season: None,
            episode: None,
            released: None,
        });
    }
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_query_is_percent_encoded() {
        let base = "https://example.com/a/manifest.json";
        assert_eq!(
            search_url(base, "movie", "top", "the wire").unwrap(),
            "https://example.com/a/catalog/movie/top/search=the%20wire.json"
        );
        assert_eq!(
            search_url(base, "movie", "top", "fast & furious 50%/+=?#é").unwrap(),
            "https://example.com/a/catalog/movie/top/\
             search=fast%20%26%20furious%2050%25%2F%2B%3D%3F%23%C3%A9.json"
        );
    }

    #[test]
    fn other_requests_keep_their_shape() {
        let base = "https://example.com/a/manifest.json?key=1";
        assert_eq!(
            catalog_url(base, "movie", "top").unwrap(),
            "https://example.com/a/catalog/movie/top.json"
        );
        assert_eq!(
            stream_url(base, "series", "tt1:1:2").unwrap(),
            "https://example.com/a/stream/series/tt1:1:2.json"
        );
        assert_eq!(
            subtitles_url(base, "series", "tt1:1:2").unwrap(),
            "https://example.com/a/subtitles/series/tt1:1:2.json"
        );
    }

    #[test]
    fn a_resolved_base_keeps_the_encoded_key_in_resource_urls() {
        let settings = Settings {
            real_debrid_key: Some("ab/c".into()),
            ..Settings::default()
        };
        let template = "https://example.com/realdebrid={real_debrid_key}/manifest.json";
        let addon = ResolvedAddon::resolve(template, &settings).unwrap();
        assert_eq!(
            stream_url(&addon.manifest_url, "movie", "tt1").unwrap(),
            "https://example.com/realdebrid=ab%2Fc/stream/movie/tt1.json"
        );
    }

    #[test]
    fn stremio_scheme_becomes_https() {
        assert_eq!(
            normalize_url("stremio://example.com/a/manifest.json").unwrap(),
            "https://example.com/a/manifest.json"
        );
        assert_eq!(
            normalize_url("  STREMIO://example.com/manifest.json ").unwrap(),
            "https://example.com/manifest.json"
        );
    }

    #[test]
    fn https_url_is_kept_as_typed() {
        let url = "https://v3-cinemeta.strem.io/manifest.json";
        assert_eq!(normalize_url(url).unwrap(), url);
        let template = "https://example.com/realdebrid={real_debrid_key}/manifest.json";
        assert_eq!(normalize_url(template).unwrap(), template);
    }

    #[test]
    fn rejects_things_that_are_not_manifest_urls() {
        for bad in [
            "",
            "hello",
            "https://example.com/",
            "ftp://example.com/manifest.json",
            "https://example.com/manifest.json.bak",
        ] {
            assert!(
                matches!(normalize_url(bad), Err(AddonError::InvalidUrl(_))),
                "{bad}"
            );
        }
    }
}
