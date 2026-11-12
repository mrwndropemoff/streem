//! Talking to an **addon** over HTTP: its manifest, catalogs, metas and streams.

use std::time::Duration;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::types::{Manifest, Meta, MetaPreview, Stream, Video};

#[derive(Debug, thiserror::Error)]
pub enum AddonError {
    #[error(
        "\"{0}\" is not an addon URL; it must be an http(s) or stremio:// link ending in /manifest.json"
    )]
    InvalidUrl(String),
    #[error("could not reach the addon: {0}")]
    Network(#[from] reqwest::Error),
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
}

/// Turns what the user typed into the URL that identifies the **addon**.
/// A `stremio://` link becomes `https://`. Nothing else in the text is changed.
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

/// Downloads and checks a manifest. Returns it with the raw text, which is what gets stored.
pub async fn fetch_manifest(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Manifest, String), AddonError> {
    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AddonError::Status(status.as_u16()));
    }
    let text = response.text().await?;
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
    let mut url = reqwest::Url::parse(manifest_url)
        .map_err(|_| AddonError::InvalidUrl(manifest_url.into()))?;
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| AddonError::InvalidUrl(manifest_url.into()))?
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
pub fn catalog_url(
    manifest_url: &str,
    meta_type: &str,
    catalog_id: &str,
) -> Result<String, AddonError> {
    resource_url(manifest_url, "catalog", meta_type, catalog_id, None)
}

/// The URL of a search in a catalog: `{base}/catalog/{type}/{id}/search={query}.json`, with the
/// query percent-encoded.
pub fn search_url(
    manifest_url: &str,
    meta_type: &str,
    catalog_id: &str,
    query: &str,
) -> Result<String, AddonError> {
    let extra = Some(("search", query));
    resource_url(manifest_url, "catalog", meta_type, catalog_id, extra)
}

/// Downloads the first page of a catalog. An entry that lacks an id, type or name is skipped,
/// so one bad entry does not hide the rest.
pub async fn fetch_catalog(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<MetaPreview>, AddonError> {
    // shortcut: first page only, add the "skip" extra when rows need to scroll further
    #[derive(serde::Deserialize)]
    struct Response {
        metas: Vec<serde_json::Value>,
    }

    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AddonError::Status(status.as_u16()));
    }
    let text = response.text().await?;
    let parsed: Response = serde_json::from_str(&text).map_err(AddonError::InvalidCatalog)?;
    Ok(parsed
        .metas
        .into_iter()
        .filter_map(|meta| serde_json::from_value(meta).ok())
        .collect())
}

/// The URL of a meta request: `{base}/meta/{type}/{id}.json`.
pub fn meta_url(manifest_url: &str, meta_type: &str, meta_id: &str) -> Result<String, AddonError> {
    resource_url(manifest_url, "meta", meta_type, meta_id, None)
}

/// The URL of a stream request: `{base}/stream/{type}/{videoId}.json`. For a movie the video
/// id is the meta id; for an episode it is the video's own id, such as `tt0903747:1:2`.
pub fn stream_url(
    manifest_url: &str,
    meta_type: &str,
    video_id: &str,
) -> Result<String, AddonError> {
    resource_url(manifest_url, "stream", meta_type, video_id, None)
}

/// Downloads the streams an **addon** offers for one video. An entry that is not an object is
/// skipped, so one bad entry does not hide the rest.
pub async fn fetch_streams(client: &reqwest::Client, url: &str) -> Result<Vec<Stream>, AddonError> {
    #[derive(serde::Deserialize)]
    struct Response {
        #[serde(default)]
        streams: Option<Vec<serde_json::Value>>,
    }

    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AddonError::Status(status.as_u16()));
    }
    let text = response.text().await?;
    let parsed: Response = serde_json::from_str(&text).map_err(AddonError::InvalidStreams)?;
    Ok(parsed
        .streams
        .unwrap_or_default()
        .into_iter()
        .filter_map(|stream| serde_json::from_value(stream).ok())
        .collect())
}

/// Downloads the description of one title. A movie that comes back with no videos gets one
/// whose id is the meta id, so there is always something to play.
pub async fn fetch_meta(
    client: &reqwest::Client,
    url: &str,
    meta_type: &str,
    meta_id: &str,
) -> Result<Meta, AddonError> {
    #[derive(serde::Deserialize)]
    struct Response {
        meta: Meta,
    }

    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AddonError::Status(status.as_u16()));
    }
    let text = response.text().await?;
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
