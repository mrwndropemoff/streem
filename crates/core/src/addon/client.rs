//! Fetching an **addon**'s manifest over HTTP.

use std::time::Duration;

use super::types::Manifest;

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
