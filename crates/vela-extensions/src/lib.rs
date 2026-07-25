//! Versioned extension manifest parsing and validation.
//!
//! Manifests describe capabilities but do not activate or authorize them. Callers retain
//! ownership of path selection, discovery, lifecycle, execution, and policy.

use std::{error::Error, fmt, fs, io, path::Path};

use serde::Deserialize;

const SUPPORTED_MANIFEST_VERSION: u64 = 1;

/// A capability class understood by version-one manifests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionKind {
    Tool,
    Skill,
    Workflow,
}

/// One validated version-one extension manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManifest {
    manifest_version: u64,
    id: String,
    kind: ExtensionKind,
    entrypoint: String,
    description: Option<String>,
}

impl ExtensionManifest {
    /// Loads and validates one caller-selected YAML manifest.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ExtensionManifestError> {
        let path = path.as_ref();
        let contents =
            fs::read_to_string(path).map_err(|source| ExtensionManifestError::Read { source })?;
        let raw: RawExtensionManifest = serde_norway::from_str(&contents)
            .map_err(|source| ExtensionManifestError::Parse { source })?;

        if raw.manifest_version != SUPPORTED_MANIFEST_VERSION {
            return Err(ExtensionManifestError::UnsupportedVersion {
                version: raw.manifest_version,
            });
        }
        validate_non_blank("id", &raw.id)?;
        validate_non_blank("entrypoint", &raw.entrypoint)?;
        let kind = match raw.kind.as_str() {
            "tool" => ExtensionKind::Tool,
            "skill" => ExtensionKind::Skill,
            "workflow" => ExtensionKind::Workflow,
            _ => return Err(ExtensionManifestError::UnsupportedKind { kind: raw.kind }),
        };

        Ok(Self {
            manifest_version: raw.manifest_version,
            id: raw.id,
            kind,
            entrypoint: raw.entrypoint,
            description: raw.description,
        })
    }

    pub fn manifest_version(&self) -> u64 {
        self.manifest_version
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> ExtensionKind {
        self.kind
    }

    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

fn validate_non_blank(field: &'static str, value: &str) -> Result<(), ExtensionManifestError> {
    if value.trim().is_empty() {
        Err(ExtensionManifestError::BlankField { field })
    } else {
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtensionManifest {
    manifest_version: u64,
    id: String,
    kind: String,
    entrypoint: String,
    description: Option<String>,
}

/// A deterministic manifest read, parse, or validation failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExtensionManifestError {
    Read { source: io::Error },
    Parse { source: serde_norway::Error },
    UnsupportedVersion { version: u64 },
    UnsupportedKind { kind: String },
    BlankField { field: &'static str },
}

impl fmt::Display for ExtensionManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { source } => {
                write!(formatter, "could not read extension manifest: {source}")
            }
            Self::Parse { source } => {
                write!(formatter, "invalid extension manifest YAML: {source}")
            }
            Self::UnsupportedVersion { version } => {
                write!(
                    formatter,
                    "unsupported extension manifest version {version}"
                )
            }
            Self::UnsupportedKind { kind } => {
                write!(formatter, "unsupported extension kind {kind}")
            }
            Self::BlankField { field } => {
                write!(
                    formatter,
                    "extension manifest field {field} must not be blank"
                )
            }
        }
    }
}

impl Error for ExtensionManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source } => Some(source),
            Self::Parse { source } => Some(source),
            Self::UnsupportedVersion { .. }
            | Self::UnsupportedKind { .. }
            | Self::BlankField { .. } => None,
        }
    }
}
