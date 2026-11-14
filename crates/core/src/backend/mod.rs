//! The hosted sync backend (Supabase): where it is, and signing in to it.

pub(crate) mod auth;

/// Where the sync backend is, and the public key every request to it carries.
#[derive(Clone)]
pub struct BackendConfig {
    /// The project URL, such as `https://<project>.supabase.co`.
    pub url: String,
    /// The public ("anon" or "publishable") key. Never a service-role key.
    pub key: String,
}

impl BackendConfig {
    /// From STREEM_SUPABASE_URL and STREEM_SUPABASE_KEY, read at compile time with option_env!.
    /// `None` when either is missing or blank.
    pub fn from_build() -> Option<BackendConfig> {
        BackendConfig::from_values(
            option_env!("STREEM_SUPABASE_URL"),
            option_env!("STREEM_SUPABASE_KEY"),
        )
    }

    fn from_values(url: Option<&str>, key: Option<&str>) -> Option<BackendConfig> {
        let url = url.map(str::trim).filter(|url| !url.is_empty())?;
        let key = key.map(str::trim).filter(|key| !key.is_empty())?;
        Some(BackendConfig {
            url: url.trim_end_matches('/').to_string(),
            key: key.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_values_are_needed() {
        assert!(BackendConfig::from_values(None, Some("k")).is_none());
        assert!(BackendConfig::from_values(Some("https://p.supabase.co"), None).is_none());
        assert!(BackendConfig::from_values(Some(" "), Some("k")).is_none());
    }

    #[test]
    fn a_trailing_slash_is_dropped() {
        let config = BackendConfig::from_values(Some("https://p.supabase.co/"), Some("k")).unwrap();
        assert_eq!(config.url, "https://p.supabase.co");
    }
}
