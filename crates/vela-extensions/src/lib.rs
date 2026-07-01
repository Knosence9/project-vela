use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vela_config::{ResolvedConfig, ResolvedExtensionConfigEntry};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
/// Enumerates the supported first-pass extension kinds Vela can discover.
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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
/// Enumerates how a discovered extension should participate in runtime activation.
pub enum ExtensionActivation {
    MetadataOnly,
    OnBoot,
}

impl ExtensionActivation {
    /// Returns the stable string label used for persistence and display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::MetadataOnly => "metadata-only",
            Self::OnBoot => "on-boot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Enumerates the lifecycle states surfaced for discovered extension entries.
pub enum ExtensionLifecycle {
    Discovered,
    Validated,
    Activated,
    Disabled,
    Failed,
}

impl ExtensionLifecycle {
    /// Returns the stable string label used for persistence and display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Validated => "validated",
            Self::Activated => "activated",
            Self::Disabled => "disabled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
/// Captures one extension manifest discovered on disk before activation policy is applied.
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
    #[serde(default)]
    pub activation: Option<ExtensionActivation>,
}

impl ExtensionManifest {
    fn resolved_activation(&self) -> ExtensionActivation {
        self.activation.clone().unwrap_or_else(|| match self.kind {
            ExtensionKind::Tool | ExtensionKind::Skill | ExtensionKind::Workflow => ExtensionActivation::OnBoot,
            ExtensionKind::Service => ExtensionActivation::MetadataOnly,
        })
    }
}

#[derive(Debug, Clone)]
/// Describes one discovered extension entry and the metadata retained for status output.
pub struct ExtensionRecord {
    pub manifest_path: PathBuf,
    pub lifecycle: ExtensionLifecycle,
    pub activation: Option<ExtensionActivation>,
    pub id: Option<String>,
    pub title: Option<String>,
    pub kind: Option<ExtensionKind>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub capabilities: Vec<String>,
    pub entry: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
/// Summarizes the currently discovered extension registry for one bootstrap or reload pass.
pub struct ExtensionsReport {
    pub manifests_dir: PathBuf,
    pub manifests_dir_existed_before: bool,
    pub discovered_manifest_count: usize,
    pub discovered_count: usize,
    pub validated_count: usize,
    pub activated_count: usize,
    pub disabled_count: usize,
    pub failed_count: usize,
    pub entries: Vec<ExtensionRecord>,
}

impl ExtensionsReport {
    /// Renders a compact extension-registry summary for CLI status output.
    pub fn summary_line(&self) -> String {
        format!(
            "extensions: dir={} existed_before={} manifests={} discovered={} validated={} activated={} disabled={} failed={}",
            self.manifests_dir.display(),
            self.manifests_dir_existed_before,
            self.discovered_manifest_count,
            self.discovered_count,
            self.validated_count,
            self.activated_count,
            self.disabled_count,
            self.failed_count,
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

    let discovered_manifest_count = manifest_paths.len();
    let mut pending = Vec::new();
    let mut id_counts = BTreeMap::<String, usize>::new();
    let mut discovered_count = 0usize;
    for path in &manifest_paths {
        let entry = parse_manifest(path)?;
        if let ParsedExtension::Valid { id, .. } = &entry {
            discovered_count += 1;
            *id_counts.entry(id.clone()).or_default() += 1;
        }
        pending.push(entry);
    }

    let mut entries = Vec::new();
    for entry in pending {
        entries.push(finalize_record(entry, &override_map, &id_counts));
    }
    let validated_count = entries
        .iter()
        .filter(|entry| matches!(entry.lifecycle, ExtensionLifecycle::Validated))
        .count();
    let activated_count = entries
        .iter()
        .filter(|entry| matches!(entry.lifecycle, ExtensionLifecycle::Activated))
        .count();
    let disabled_count = entries
        .iter()
        .filter(|entry| matches!(entry.lifecycle, ExtensionLifecycle::Disabled))
        .count();
    let failed_count = entries
        .iter()
        .filter(|entry| matches!(entry.lifecycle, ExtensionLifecycle::Failed))
        .count();

    Ok(ExtensionsReport {
        manifests_dir: manifests_dir.to_path_buf(),
        manifests_dir_existed_before: existed_before,
        discovered_manifest_count,
        discovered_count,
        validated_count,
        activated_count,
        disabled_count,
        failed_count,
        entries,
    })
}

enum ParsedExtension {
    Valid {
        manifest_path: PathBuf,
        id: String,
        title: String,
        kind: ExtensionKind,
        activation: ExtensionActivation,
        version: Option<String>,
        description: Option<String>,
        capabilities: Vec<String>,
        entry: Option<String>,
    },
    Invalid(ExtensionRecord),
}

fn parse_manifest(path: &Path) -> Result<ParsedExtension> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: ExtensionManifest = match serde_yaml::from_str(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Ok(ParsedExtension::Invalid(ExtensionRecord {
                manifest_path: path.to_path_buf(),
                lifecycle: ExtensionLifecycle::Failed,
                activation: None,
                id: None,
                title: None,
                kind: None,
                version: None,
                description: None,
                capabilities: vec![],
                entry: None,
                detail: Some(error.to_string()),
            }));
        }
    };

    let trimmed_id = parsed.id.trim().to_string();
    if trimmed_id.is_empty() {
        return Ok(ParsedExtension::Invalid(ExtensionRecord {
            manifest_path: path.to_path_buf(),
            lifecycle: ExtensionLifecycle::Failed,
            activation: None,
            id: None,
            title: Some(parsed.title),
            kind: Some(parsed.kind),
            version: parsed.version,
            description: parsed.description,
            capabilities: parsed.capabilities,
            entry: parsed.entry,
            detail: Some("manifest id cannot be empty".to_string()),
        }));
    }
    if parsed.manifest_version != 1 {
        return Ok(ParsedExtension::Invalid(ExtensionRecord {
            manifest_path: path.to_path_buf(),
            lifecycle: ExtensionLifecycle::Failed,
            activation: Some(parsed.resolved_activation()),
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
        }));
    }

    let activation = parsed.resolved_activation();
    Ok(ParsedExtension::Valid {
        manifest_path: path.to_path_buf(),
        id: trimmed_id,
        title: parsed.title,
        kind: parsed.kind,
        activation,
        version: parsed.version,
        description: parsed.description,
        capabilities: parsed.capabilities,
        entry: parsed.entry,
    })
}

fn finalize_record(
    parsed: ParsedExtension,
    overrides: &BTreeMap<String, bool>,
    id_counts: &BTreeMap<String, usize>,
) -> ExtensionRecord {
    match parsed {
        ParsedExtension::Invalid(record) => record,
        ParsedExtension::Valid {
            manifest_path,
            id,
            title,
            kind,
            activation,
            version,
            description,
            capabilities,
            entry,
        } => {
            if id_counts.get(&id).copied().unwrap_or_default() > 1 {
                return ExtensionRecord {
                    manifest_path,
                    lifecycle: ExtensionLifecycle::Failed,
                    activation: Some(activation),
                    id: Some(id),
                    title: Some(title),
                    kind: Some(kind),
                    version,
                    description,
                    capabilities,
                    entry,
                    detail: Some("duplicate extension id discovered".to_string()),
                };
            }

            let base = ExtensionRecord {
                manifest_path,
                lifecycle: ExtensionLifecycle::Discovered,
                activation: Some(activation.clone()),
                id: Some(id.clone()),
                title: Some(title),
                kind: Some(kind.clone()),
                version,
                description,
                capabilities,
                entry,
                detail: None,
            };

            if !overrides.get(&id).copied().unwrap_or(true) {
                return ExtensionRecord {
                    lifecycle: ExtensionLifecycle::Disabled,
                    detail: Some("disabled by config override".to_string()),
                    ..base
                };
            }

            match activation {
                ExtensionActivation::MetadataOnly => ExtensionRecord {
                    lifecycle: ExtensionLifecycle::Validated,
                    detail: Some("metadata-only extension in this slice".to_string()),
                    ..base
                },
                ExtensionActivation::OnBoot => match activation_failure(base.kind.as_ref(), base.entry.as_deref()) {
                    Some(detail) => ExtensionRecord {
                        lifecycle: ExtensionLifecycle::Failed,
                        detail: Some(detail),
                        ..base
                    },
                    None => ExtensionRecord {
                        lifecycle: ExtensionLifecycle::Activated,
                        detail: Some("activation completed during bootstrap".to_string()),
                        ..base
                    },
                },
            }
        }
    }
}

fn activation_failure(kind: Option<&ExtensionKind>, entry: Option<&str>) -> Option<String> {
    match kind {
        Some(ExtensionKind::Service) => Some("service extensions remain metadata-only in this slice".to_string()),
        Some(ExtensionKind::Tool | ExtensionKind::Skill | ExtensionKind::Workflow) => {
            match entry.map(str::trim).filter(|value| !value.is_empty()) {
                Some(_) => None,
                None => Some("activation requires a non-empty entry path".to_string()),
            }
        }
        None => Some("activation requires a known extension kind".to_string()),
    }
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
    /// Verifies that manifests can activate, validate as metadata-only, or disable through config.
    fn initialize_extensions_applies_lifecycle_states() {
        let vela_home = std::env::temp_dir().join(format!("vela-ext-test-{}", std::process::id()));
        let manifests_dir = vela_home.join("extensions-manifests");
        let _ = std::fs::remove_dir_all(&vela_home);
        std::fs::create_dir_all(&manifests_dir).unwrap();
        std::fs::write(
            manifests_dir.join("demo.yaml"),
            "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\nentry: extensions/demo-tool.wasm\ncapabilities:\n  - chat\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("service.yaml"),
            "manifest_version: 1\nid: service\ntitle: Service\nkind: service\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("ops.yaml"),
            "manifest_version: 1\nid: ops\ntitle: Ops\nkind: workflow\nentry: extensions/ops.flow\n",
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

        assert_eq!(report.discovered_manifest_count, 3);
        assert_eq!(report.discovered_count, 3);
        assert_eq!(report.activated_count, 1);
        assert_eq!(report.validated_count, 1);
        assert_eq!(report.disabled_count, 1);
        assert_eq!(report.failed_count, 0);
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("demo") && matches!(entry.lifecycle, ExtensionLifecycle::Activated)
        }));
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("service") && matches!(entry.lifecycle, ExtensionLifecycle::Validated)
        }));
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("ops") && matches!(entry.lifecycle, ExtensionLifecycle::Disabled)
        }));

        let _ = std::fs::remove_dir_all(&vela_home);
    }

    #[test]
    /// Verifies that duplicate ids, unsupported manifests, and failed activation are surfaced as failed entries.
    fn initialize_extensions_marks_failed_entries() {
        let vela_home = std::env::temp_dir().join(format!("vela-ext-invalid-{}", std::process::id()));
        let manifests_dir = vela_home.join("extensions-manifests");
        let _ = std::fs::remove_dir_all(&vela_home);
        std::fs::create_dir_all(&manifests_dir).unwrap();
        std::fs::write(
            manifests_dir.join("duplicate-a.yaml"),
            "manifest_version: 1\nid: duplicate\ntitle: First\nkind: tool\nentry: extensions/a.wasm\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("duplicate-b.yaml"),
            "manifest_version: 1\nid: duplicate\ntitle: Second\nkind: tool\nentry: extensions/b.wasm\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("unsupported.yaml"),
            "manifest_version: 2\nid: unsupported\ntitle: Unsupported\nkind: skill\n",
        )
        .unwrap();
        std::fs::write(
            manifests_dir.join("missing-entry.yaml"),
            "manifest_version: 1\nid: missing-entry\ntitle: Missing Entry\nkind: workflow\n",
        )
        .unwrap();

        let report = initialize_extensions(&vela_home, &resolved_with(vec![])).unwrap();
        assert_eq!(report.discovered_manifest_count, 4);
        assert_eq!(report.discovered_count, 3);
        assert_eq!(report.activated_count, 0);
        assert_eq!(report.failed_count, 4);
        let duplicate_failed = report
            .entries
            .iter()
            .filter(|entry| {
                entry.id.as_deref() == Some("duplicate")
                    && matches!(entry.lifecycle, ExtensionLifecycle::Failed)
                    && entry.detail.as_deref() == Some("duplicate extension id discovered")
            })
            .count();
        assert_eq!(duplicate_failed, 2);
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("unsupported")
                && matches!(entry.lifecycle, ExtensionLifecycle::Failed)
        }));
        assert!(report.entries.iter().any(|entry| {
            entry.id.as_deref() == Some("missing-entry")
                && matches!(entry.lifecycle, ExtensionLifecycle::Failed)
                && entry.detail.as_deref() == Some("activation requires a non-empty entry path")
        }));

        let _ = std::fs::remove_dir_all(&vela_home);
    }
}
