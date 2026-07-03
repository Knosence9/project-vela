use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vela_config::{ResolvedConfig, ResolvedExtensionConfigEntry};

mod surface;

#[cfg(test)]
mod tests;

pub use surface::*;

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
                    detail: Some(metadata_only_detail(base.kind.as_ref())),
                    ..base
                },
                ExtensionActivation::OnBoot => {
                    match activation_failure(base.kind.as_ref(), base.entry.as_deref()) {
                        Some(detail) => ExtensionRecord {
                            lifecycle: ExtensionLifecycle::Failed,
                            detail: Some(detail),
                            ..base
                        },
                        None => ExtensionRecord {
                            lifecycle: ExtensionLifecycle::Activated,
                            detail: Some(activation_success_detail(base.kind.as_ref())),
                            ..base
                        },
                    }
                }
            }
        }
    }
}

fn activation_failure(kind: Option<&ExtensionKind>, entry: Option<&str>) -> Option<String> {
    match kind {
        Some(ExtensionKind::Service) => Some(
            "service extensions cannot request on-boot activation in this slice".to_string(),
        ),
        Some(ExtensionKind::Tool | ExtensionKind::Skill | ExtensionKind::Workflow) => {
            match entry.map(str::trim).filter(|value| !value.is_empty()) {
                Some(_) => None,
                None => Some("activation requires a non-empty entry path".to_string()),
            }
        }
        None => Some("activation requires a known extension kind".to_string()),
    }
}

fn metadata_only_detail(kind: Option<&ExtensionKind>) -> String {
    match kind {
        Some(ExtensionKind::Service) => {
            "service extensions remain metadata-only in this slice".to_string()
        }
        Some(ExtensionKind::Tool) => {
            "tool extension validated as metadata-only by manifest policy".to_string()
        }
        Some(ExtensionKind::Skill) => {
            "skill extension validated as metadata-only by manifest policy".to_string()
        }
        Some(ExtensionKind::Workflow) => {
            "workflow extension validated as metadata-only by manifest policy".to_string()
        }
        None => "metadata-only activation requires a known extension kind".to_string(),
    }
}

fn activation_success_detail(kind: Option<&ExtensionKind>) -> String {
    match kind {
        Some(ExtensionKind::Tool) => {
            "tool extension activated during bootstrap".to_string()
        }
        Some(ExtensionKind::Skill) => {
            "skill extension activated during bootstrap".to_string()
        }
        Some(ExtensionKind::Workflow) => {
            "workflow extension activated during bootstrap".to_string()
        }
        Some(ExtensionKind::Service) => {
            "service extensions cannot activate during bootstrap in this slice".to_string()
        }
        None => "extension activated during bootstrap".to_string(),
    }
}
