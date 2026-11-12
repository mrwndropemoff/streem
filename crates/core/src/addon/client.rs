//! Fetching an **addon**'s manifest over HTTP.

use std::time::Duration;

use super::types::{Manifest, Meta, MetaPreview, Video};

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

/// The URL of a catalog: `{base}/catalog/{type}/{id}.json`, where `{base}` is the manifest URL
/// without `/manifest.json`. `{type}` and `{id}` are percent-encoded.
pub fn catalog_url(
    manifest_url: &str,
    meta_type: &str,
    catalog_id: &str,
) -> Result<String, AddonError> {
    let mut url = reqwest::Url::parse(manifest_url)
        .map_err(|_| AddonError::InvalidUrl(manifest_url.into()))?;
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| AddonError::InvalidUrl(manifest_url.into()))?
        .pop()
        .push("catalog")
        .push(meta_type)
        .push(&format!("{catalog_id}.json"));
    Ok(url.into())
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

/// The URL of a meta request: `{base}/meta/{type}/{id}.json`. `{type}` and `{id}` are
/// percent-encoded.
pub fn meta_url(manifest_url: &str, meta_type: &str, meta_id: &str) -> Result<String, AddonError> {
    let mut url = reqwest::Url::parse(manifest_url)
        .map_err(|_| AddonError::InvalidUrl(manifest_url.into()))?;
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| AddonError::InvalidUrl(manifest_url.into()))?
        .pop()
        .push("meta")
        .push(meta_type)
        .push(&format!("{meta_id}.json"));
    Ok(url.into())
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
