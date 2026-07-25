//! Versioned extension manifest parsing and validation.
//!
//! Manifests describe capabilities but do not activate or authorize them. Callers retain
//! ownership of path selection, discovery, lifecycle, execution, and policy.

use std::{
    error::Error,
    fmt, fs, io,
    io::Read,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

#[cfg(unix)]
use rustix::fs::{Dir, Mode, OFlags, open, openat};
use serde::Deserialize;

const SUPPORTED_MANIFEST_VERSION: u64 = 1;

/// Maximum accepted encoded manifest size.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

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
        let file =
            fs::File::open(path).map_err(|source| ExtensionManifestError::Read { source })?;
        Self::load_reader(file)
    }

    fn load_reader(mut reader: impl Read) -> Result<Self, ExtensionManifestError> {
        let mut contents = Vec::new();
        reader
            .by_ref()
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut contents)
            .map_err(|source| ExtensionManifestError::Read { source })?;
        if contents.len() > MAX_MANIFEST_BYTES as usize {
            return Err(ExtensionManifestError::TooLarge {
                max_bytes: MAX_MANIFEST_BYTES,
            });
        }
        let contents =
            std::str::from_utf8(&contents).map_err(|source| ExtensionManifestError::Read {
                source: io::Error::new(io::ErrorKind::InvalidData, source),
            })?;
        let raw: RawExtensionManifest = serde_norway::from_str(contents)
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

/// One validated manifest and the path it was loaded from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredExtension {
    path: PathBuf,
    manifest: ExtensionManifest,
}

impl DiscoveredExtension {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }
}

/// Discovers validated manifests at `root/*/extension.yaml` without activating them.
pub fn discover_extensions(
    root: impl AsRef<Path>,
) -> Result<Vec<DiscoveredExtension>, ExtensionDiscoveryError> {
    discover_extensions_platform(root.as_ref())
}

#[cfg(unix)]
fn discover_extensions_platform(
    root: &Path,
) -> Result<Vec<DiscoveredExtension>, ExtensionDiscoveryError> {
    let root_fd = open(
        root,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|source| ExtensionDiscoveryError::ReadRoot {
        path: root.to_path_buf(),
        source: io::Error::from(source),
    })?;
    let mut directory =
        Dir::read_from(&root_fd).map_err(|source| ExtensionDiscoveryError::ReadRoot {
            path: root.to_path_buf(),
            source: io::Error::from(source),
        })?;
    let mut children = Vec::new();

    while let Some(entry) = directory.read() {
        let entry = entry.map_err(|source| ExtensionDiscoveryError::ReadRoot {
            path: root.to_path_buf(),
            source: io::Error::from(source),
        })?;
        if !matches!(entry.file_name().to_bytes(), b"." | b"..") {
            children.push(entry.file_name().to_owned());
        }
    }
    children.sort_by(|left, right| left.to_bytes().cmp(right.to_bytes()));

    let mut discovered = Vec::new();
    for child_name in children {
        let child_fd = match openat(
            &root_fd,
            &child_name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(child_fd) => child_fd,
            Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
                continue;
            }
            Err(source) => {
                return Err(ExtensionDiscoveryError::ReadRoot {
                    path: root.to_path_buf(),
                    source: io::Error::from(source),
                });
            }
        };
        let path = root
            .join(OsStr::from_bytes(child_name.to_bytes()))
            .join("extension.yaml");
        let manifest_fd = match openat(
            &child_fd,
            c"extension.yaml",
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(manifest_fd) => manifest_fd,
            Err(rustix::io::Errno::NOENT) => continue,
            Err(source) => {
                return Err(ExtensionDiscoveryError::Manifest {
                    path,
                    source: ExtensionManifestError::Read {
                        source: io::Error::from(source),
                    },
                });
            }
        };
        let file = fs::File::from(manifest_fd);
        let metadata = file
            .metadata()
            .map_err(|source| ExtensionDiscoveryError::Manifest {
                path: path.clone(),
                source: ExtensionManifestError::Read { source },
            })?;
        if !metadata.is_file() {
            continue;
        }
        let manifest = ExtensionManifest::load_reader(file).map_err(|source| {
            ExtensionDiscoveryError::Manifest {
                path: path.clone(),
                source,
            }
        })?;
        discovered.push(DiscoveredExtension { path, manifest });
    }
    Ok(discovered)
}

#[cfg(not(unix))]
fn discover_extensions_platform(
    root: &Path,
) -> Result<Vec<DiscoveredExtension>, ExtensionDiscoveryError> {
    Err(ExtensionDiscoveryError::ReadRoot {
        path: root.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::Unsupported,
            "secure extension discovery is unsupported on this platform",
        ),
    })
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
    TooLarge { max_bytes: u64 },
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
            Self::TooLarge { max_bytes } => {
                write!(formatter, "extension manifest exceeds {max_bytes} bytes")
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
            Self::TooLarge { .. }
            | Self::UnsupportedVersion { .. }
            | Self::UnsupportedKind { .. }
            | Self::BlankField { .. } => None,
        }
    }
}

/// A deterministic extension-root enumeration or candidate validation failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExtensionDiscoveryError {
    ReadRoot {
        path: PathBuf,
        source: io::Error,
    },
    Manifest {
        path: PathBuf,
        source: ExtensionManifestError,
    },
}

impl fmt::Display for ExtensionDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadRoot { path, source } => {
                write!(
                    formatter,
                    "could not read extension root {}: {source}",
                    path.display()
                )
            }
            Self::Manifest { path, source } => {
                write!(
                    formatter,
                    "invalid discovered extension manifest {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ExtensionDiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadRoot { source, .. } => Some(source),
            Self::Manifest { source, .. } => Some(source),
        }
    }
}
