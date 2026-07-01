use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use vela_config::{ResolvedConfig, ResolvedExtensionConfigEntry};

/// Enumerates the supported first-pass extension kinds Vela can discover.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionKind {
    Tool,
    Skill,
    Workflow,
    Service,
}

impl ExtensionKind {
    /// Returns the stable string label used for persistence and display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Skill => "skill",
            Self::Workflow => "workflow",
            Self::Service => "service",
        }
    }
}

/// Captures one extension manifest discovered on disk before enable/disable policy is applied.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtensionManifest {
    #[serde(default = "default_manifest_version")]
    pub manifest_version: u32,
    pub id: String,
    pub title: String,
    pub kind: ExtensionKind,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub entry: Option<String>,
}

/// Represents the load state assigned to one discovered extension manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionState {
    Loaded,
    DisabledByConfig,
    InvalidManifest,
}

impl ExtensionState {
    /// Returns the stable string label used for persistence and display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Loaded => "loaded",
            Self::DisabledByConfig => "disabled-by-config",
            Self::InvalidManifest => "invalid-manifest",
        }
    }
}

/// Describes one discovered extension entry and the metadata retained for status output.
#[derive(Debug, Clone)]
pub struct ExtensionRecord {
    pub manifest_path: PathBuf,
    pub state: ExtensionState,
    pub id: Option<String>,
    pub title: Option<String>,
    pub kind: Option<ExtensionKind>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub capabilities: Vec<String>,
    pub entry: Option<String>,
    pub detail: Option<String>,
}

/// Summarizes the currently discovered extension registry for one bootstrap or reload pass.
#[derive(Debug, Clone)]
pub struct ExtensionsReport {
    pub manifests_dir: PathBuf,
    pub manifests_dir_existed_before: bool,
    pub discovered_manifest_count: usize,
    pub loaded_count: usize,
    pub disabled_count: usize,
    pub invalid_count: usize,
    pub entries: Vec<ExtensionRecord>,
}

impl ExtensionsReport {
    /// Renders a compact extension-registry summary for CLI status output.
    pub fn summary_line(&self) -> String {
        format!(
            "extensions: dir={} existed_before={} discovered={} loaded={} disabled={} invalid={}",
            self.manifests_dir.display(),
            self.manifests_dir_existed_before,
            self.discovered_manifest_count,
            self.loaded_count,
            self.disabled_count,
            self.invalid_count,
        )
    }
}

/// Initializes the extension manifest directory and loads the current registry snapshot.
pub fn initialize_extensions(vela_home: &Path, resolved: &ResolvedConfig) -> Result<ExtensionsReport> {
    let manifests_dir = resolve_manifests_dir(vela_home, resolved);
    let existed_before = manifests_dir.is_dir();
    std::fs::create_dir_all(&manifests_dir)
        .with_context(|| format!("failed to create {}", manifests_dir.display()))?;
    load_registry_from_dir(&manifests_dir, existed_before, &resolved.extension_entries)
}

fn default_manifest_version() -> u32 {
    1
}

fn resolve_manifests_dir(vela_home: &Path, resolved: &ResolvedConfig) -> PathBuf {
    match resolved.extension_manifests_dir.as_deref() {
        Some(value) if !value.trim().is_empty() => expand_manifest_dir(vela_home, value.trim()),
        _ => vela_home.join("extensions"),
    }
}

fn expand_manifest_dir(vela_home: &Path, raw: &str) -> PathBuf {
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        candidate
    } else {
        vela_home.join(candidate)
    }
}

fn load_registry_from_dir(
    manifests_dir: &Path,
    existed_before: bool,
    overrides: &[ResolvedExtensionConfigEntry],
) -> Result<ExtensionsReport> {
    let override_map: BTreeMap<_, _> = overrides
        .iter()
        .map(|entry| (entry.id.trim().to_string(), entry.enabled))
        .collect();

    let mut entries = Vec::new();
    let mut seen_ids = BTreeSet::new();
    let mut manifest_paths = Vec::new();
    for dir_entry in std::fs::read_dir(manifests_dir)
        .with_context(|| format!("failed to read {}", manifests_dir.display()))?
    {
        let dir_entry = dir_entry?;
        let path = dir_entry.path();
        if !dir_entry.file_type()?.is_file() {
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if matches!(extension, "yaml" | "yml") {
            manifest_paths.push(path);
        }
    }
    manifest_paths.sort();

    for path in &manifest_paths {
        entries.push(load_record(path, &override_map, &mut seen_ids)?);
    }

    let discovered_manifest_count = manifest_paths.len();
    let loaded_count = entries
        .iter()
        .filter(|entry| matches!(entry.state, ExtensionState::Loaded))
        .count();
    let disabled_count = entries
        .iter()
        .filter(|entry| matches!(entry.state, ExtensionState::DisabledByConfig))
        .count();
    let invalid_count = entries
        .iter()
        .filter(|entry| matches!(entry.state, ExtensionState::InvalidManifest))
        .count();

    Ok(ExtensionsReport {
        manifests_dir: manifests_dir.to_path_buf(),
        manifests_dir_existed_before: existed_before,
        discovered_manifest_count,
        loaded_count,
        disabled_count,
        invalid_count,
        entries,
    })
}

fn load_record(
    path: &Path,
    overrides: &BTreeMap<String, bool>,
    seen_ids: &mut BTreeSet<String>,
) -> Result<ExtensionRecord> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: ExtensionManifest = match serde_yaml::from_str(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Ok(ExtensionRecord {
                manifest_path: path.to_path_buf(),
                state: ExtensionState::InvalidManifest,
                id: None,
                title: None,
                kind: None,
                version: None,
                description: None,
                capabilities: vec![],
                entry: None,
                detail: Some(error.to_string()),
            });
        }
    };

    let trimmed_id = parsed.id.trim().to_string();
    if trimmed_id.is_empty() {
        return Ok(ExtensionRecord {
            manifest_path: path.to_path_buf(),
            state: ExtensionState::InvalidManifest,
            id: None,
            title: Some(parsed.title),
            kind: Some(parsed.kind),
            version: parsed.version,
            description: parsed.description,
            capabilities: parsed.capabilities,
            entry: parsed.entry,
            detail: Some("manifest id cannot be empty".to_string()),
        });
    }
    if parsed.manifest_version != 1 {
        return Ok(ExtensionRecord {
            manifest_path: path.to_path_buf(),
            state: ExtensionState::InvalidManifest,
            id: Some(trimmed_id),
            title: Some(parsed.title),
            kind: Some(parsed.kind),
            version: parsed.version,
            description: parsed.description,
            capabilities: parsed.capabilities,
            entry: parsed.entry,
            detail: Some(format!(
                "unsupported manifest_version {}; expected 1",
                parsed.manifest_version
            )),
        });
    }
    if !seen_ids.insert(trimmed_id.clone()) {
        return Ok(ExtensionRecord {
            manifest_path: path.to_path_buf(),
            state: ExtensionState::InvalidManifest,
            id: Some(trimmed_id),
            title: Some(parsed.title),
            kind: Some(parsed.kind),
            version: parsed.version,
            description: parsed.description,
            capabilities: parsed.capabilities,
            entry: parsed.entry,
            detail: Some("duplicate extension id discovered".to_string()),
        });
    }

    let enabled = overrides.get(&trimmed_id).copied().unwrap_or(true);
    Ok(ExtensionRecord {
        manifest_path: path.to_path_buf(),
        state: if enabled {
            ExtensionState::Loaded
        } else {
            ExtensionState::DisabledByConfig
        },
        id: Some(trimmed_id),
        title: Some(parsed.title),
        kind: Some(parsed.kind),
        version: parsed.version,
        description: parsed.description,
        capabilities: parsed.capabilities,
        entry: parsed.entry,
        detail: if enabled {
            None
        } else {
            Some("disabled by config override".to_string())
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a resolved config with extension-specific overrides for unit tests.
    fn resolved_with(entries: Vec<ResolvedExtensionConfigEntry>) -> ResolvedConfig {
        ResolvedConfig {
            extension_manifests_dir: Some("extensions-manifests".to_string()),
            extension_entries: entries,
            ..ResolvedConfig::default()
        }
    }

    #[test]
    /// Verifies that manifest discovery loads valid entries and applies config disables.
    fn initialize_extensions_discovers_manifests_and_applies_config_disables() {
        let vela_home = std::env::temp_dir().join(format!("vela-ext-test-{}", std::process::id()));
        let manifests_dir = vela_home.join("extensions-manifests");
        let _ = std::fs::remove_dir_all(&vela_home);
        std::fs::create_dir_all(&manifests_dir).unwrap();
        std::fs::write(
            manifests_dir.join("demo.yaml"),
            "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\ncapabilities:\n  - chat\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("ops.yaml"),
            "manifest_version: 1\nid: ops\ntitle: Ops\nkind: workflow\n",
        )
        .unwrap();

        let report = initialize_extensions(
            &vela_home,
            &resolved_with(vec![ResolvedExtensionConfigEntry {
                id: "ops".to_string(),
                enabled: false,
            }]),
        )
        .unwrap();

        assert_eq!(report.discovered_manifest_count, 2);
        assert_eq!(report.loaded_count, 1);
        assert_eq!(report.disabled_count, 1);
        assert_eq!(report.invalid_count, 0);
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("demo") && matches!(entry.state, ExtensionState::Loaded)
        }));
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("ops")
                && matches!(entry.state, ExtensionState::DisabledByConfig)
        }));

        let _ = std::fs::remove_dir_all(&vela_home);
    }

    #[test]
    /// Verifies that duplicate ids or unsupported manifests are surfaced as invalid entries.
    fn initialize_extensions_marks_invalid_manifests() {
        let vela_home = std::env::temp_dir().join(format!("vela-ext-invalid-{}", std::process::id()));
        let manifests_dir = vela_home.join("extensions-manifests");
        let _ = std::fs::remove_dir_all(&vela_home);
        std::fs::create_dir_all(&manifests_dir).unwrap();
        std::fs::write(
            manifests_dir.join("duplicate-a.yaml"),
            "manifest_version: 1\nid: duplicate\ntitle: First\nkind: tool\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("duplicate-b.yaml"),
            "manifest_version: 1\nid: duplicate\ntitle: Second\nkind: tool\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("unsupported.yaml"),
            "manifest_version: 2\nid: unsupported\ntitle: Unsupported\nkind: skill\n",
        )
        .unwrap();

        let report = initialize_extensions(&vela_home, &resolved_with(vec![])).unwrap();
        assert_eq!(report.discovered_manifest_count, 3);
        assert_eq!(report.loaded_count, 1);
        assert_eq!(report.invalid_count, 2);
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("duplicate")
                && matches!(entry.state, ExtensionState::InvalidManifest)
                && entry.detail.as_deref() == Some("duplicate extension id discovered")
        }));
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("unsupported")
                && matches!(entry.state, ExtensionState::InvalidManifest)
        }));

        let _ = std::fs::remove_dir_all(&vela_home);
    }
}
