use super::*;

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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
/// Enumerates the bounded lifecycle hooks an extension may declare in this slice.
pub enum ExtensionLifecycleHook {
    OnActivate,
    OnReload,
}

impl ExtensionLifecycleHook {
    /// Returns the stable string label used for persistence and display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::OnActivate => "on-activate",
            Self::OnReload => "on-reload",
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
    #[serde(default)]
    pub hooks: Vec<ExtensionLifecycleHook>,
}

impl ExtensionManifest {
    pub(crate) fn resolved_activation(&self) -> ExtensionActivation {
        self.activation.clone().unwrap_or_else(|| match self.kind {
            ExtensionKind::Tool | ExtensionKind::Skill | ExtensionKind::Workflow => {
                ExtensionActivation::OnBoot
            }
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
    pub hooks: Vec<ExtensionLifecycleHook>,
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
pub fn initialize_extensions(
    vela_home: &Path,
    resolved: &ResolvedConfig,
) -> Result<ExtensionsReport> {
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
