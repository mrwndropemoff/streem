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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestCatalog {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
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
    fn rejects_a_manifest_without_a_name() {
        let json = r#"{"id":"a","version":"0.1.0","types":[],"resources":[]}"#;
        assert!(serde_json::from_str::<Manifest>(json).is_err());
    }
}
