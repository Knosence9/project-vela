//! Versioned extension manifest parsing and validation.
//!
//! Manifests describe capabilities but do not activate or authorize them. Callers retain
//! ownership of path selection, discovery, lifecycle, execution, and policy.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs, io,
    io::Read,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{ffi::OsStr, os::fd::AsFd, os::unix::ffi::OsStrExt};

#[cfg(unix)]
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, fstat, open, openat, statat};
use serde::Deserialize;

const SUPPORTED_MANIFEST_VERSION: u64 = 1;

/// Maximum accepted encoded manifest size.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Maximum accepted encoded WebAssembly component size during artifact preparation.
pub const MAX_ENTRYPOINT_BYTES: u64 = 16 * 1024 * 1024;

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
        validate_entrypoint(&raw.entrypoint)?;
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
#[derive(Clone, Debug)]
pub struct DiscoveredExtension {
    path: PathBuf,
    manifest: ExtensionManifest,
    root_identity: FileIdentity,
    package_identity: FileIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl PartialEq for DiscoveredExtension {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.manifest == other.manifest
    }
}

impl Eq for DiscoveredExtension {}

impl DiscoveredExtension {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }
}

/// Owned bytes for one revalidated selected tool, ready for a later component compiler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedToolArtifact {
    id: String,
    bytes: Vec<u8>,
}

impl PreparedToolArtifact {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// An immutable, non-activating snapshot of one discovered extension root.
#[derive(Clone, Debug)]
pub struct ExtensionRegistry {
    extensions: Vec<DiscoveredExtension>,
    indices_by_id: BTreeMap<String, usize>,
}

/// One exact-ID difference between two immutable registry snapshots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRegistryChange<'a> {
    /// The extension exists only in the current snapshot.
    Added(&'a DiscoveredExtension),
    /// The extension exists only in the previous snapshot.
    Removed(&'a DiscoveredExtension),
    /// The exact ID exists in both snapshots, but its path or manifest changed.
    Changed {
        previous: &'a DiscoveredExtension,
        current: &'a DiscoveredExtension,
    },
}

/// A fail-closed selected-tool artifact preparation error.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExtensionPreparationError {
    ReadRoot {
        path: PathBuf,
        source: io::Error,
    },
    SourceMismatch {
        id: String,
        path: PathBuf,
    },
    KindMismatch {
        id: String,
        actual: ExtensionKind,
    },
    Package {
        id: String,
        path: PathBuf,
        source: io::Error,
    },
    PackageChanged {
        id: String,
        path: PathBuf,
    },
    Manifest {
        id: String,
        path: PathBuf,
        source: ExtensionManifestError,
    },
    ManifestChanged {
        id: String,
        path: PathBuf,
    },
    Entrypoint {
        id: String,
        path: PathBuf,
        entrypoint: String,
        source: io::Error,
    },
    EntrypointTooLarge {
        id: String,
        path: PathBuf,
        max_bytes: u64,
    },
}

/// A fail-closed exact-ID registry selection error.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionSelectionError {
    /// The caller requested the same exact ID more than once.
    DuplicateId { id: String },
    /// The requested exact ID does not exist in the registry snapshot.
    NotFound { id: String },
    /// The requested exact ID exists, but has a different validated capability kind.
    KindMismatch {
        id: String,
        expected: ExtensionKind,
        actual: ExtensionKind,
    },
}

/// An immutable caller selection borrowed from one registry snapshot.
#[derive(Clone, Debug)]
pub struct ExtensionSelection<'a> {
    extensions: Vec<&'a DiscoveredExtension>,
}

impl<'a> ExtensionSelection<'a> {
    /// Projects this selection to one validated capability kind.
    ///
    /// The projected selection preserves exact-ID order and borrows the same registry records.
    /// Projection performs no filesystem access, activation, authorization, or mutation.
    pub fn of_kind(&self, kind: ExtensionKind) -> Self {
        Self {
            extensions: self
                .extensions
                .iter()
                .copied()
                .filter(|extension| extension.manifest().kind() == kind)
                .collect(),
        }
    }

    /// Resolves one exact ID within this selection.
    pub fn get(&self, id: &str) -> Option<&'a DiscoveredExtension> {
        self.extensions
            .binary_search_by(|extension| extension.manifest().id().cmp(id))
            .ok()
            .map(|index| self.extensions[index])
    }

    /// Enumerates selected records in deterministic exact-ID order.
    pub fn extensions(&self) -> impl ExactSizeIterator<Item = &'a DiscoveredExtension> + '_ {
        self.extensions.iter().copied()
    }

    /// Returns the number of selected records.
    pub fn len(&self) -> usize {
        self.extensions.len()
    }

    /// Returns whether no records are selected.
    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }
}

impl ExtensionRegistry {
    /// Discovers one extension root and owns the complete validated snapshot.
    pub fn discover(root: impl AsRef<Path>) -> Result<Self, ExtensionDiscoveryError> {
        let extensions = discover_extensions(root)?;
        let indices_by_id = extensions
            .iter()
            .enumerate()
            .map(|(index, extension)| (extension.manifest().id().to_owned(), index))
            .collect();
        Ok(Self {
            extensions,
            indices_by_id,
        })
    }

    /// Resolves one exact caller-authored extension ID without activation.
    pub fn get(&self, id: &str) -> Option<&DiscoveredExtension> {
        self.indices_by_id
            .get(id)
            .map(|index| &self.extensions[*index])
    }

    /// Enumerates the snapshot in deterministic manifest-path order.
    pub fn extensions(&self) -> impl ExactSizeIterator<Item = &DiscoveredExtension> {
        self.extensions.iter()
    }

    /// Resolves an all-or-nothing caller selection in deterministic exact-ID order.
    ///
    /// Selection performs no filesystem access, activation, authorization, or mutation.
    pub fn select<I, S>(&self, ids: I) -> Result<ExtensionSelection<'_>, ExtensionSelectionError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.select_with_kind(ids, None)
    }

    /// Resolves an all-or-nothing caller selection of one expected capability kind.
    ///
    /// Kind-constrained selection performs no filesystem access, activation, authorization, or
    /// mutation. An existing ID of another kind is rejected rather than silently omitted.
    pub fn select_kind<I, S>(
        &self,
        kind: ExtensionKind,
        ids: I,
    ) -> Result<ExtensionSelection<'_>, ExtensionSelectionError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.select_with_kind(ids, Some(kind))
    }

    fn select_with_kind<I, S>(
        &self,
        ids: I,
        expected_kind: Option<ExtensionKind>,
    ) -> Result<ExtensionSelection<'_>, ExtensionSelectionError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut selected_ids = BTreeSet::new();
        let mut duplicate_ids = BTreeSet::new();
        for id in ids {
            let id = id.as_ref().to_owned();
            if !selected_ids.insert(id.clone()) {
                duplicate_ids.insert(id);
            }
        }

        if let Some(id) = duplicate_ids.into_iter().next() {
            return Err(ExtensionSelectionError::DuplicateId { id });
        }

        let extensions = selected_ids
            .into_iter()
            .map(|id| {
                let extension = self
                    .get(&id)
                    .ok_or_else(|| ExtensionSelectionError::NotFound { id: id.clone() })?;
                let actual = extension.manifest().kind();
                if let Some(expected) = expected_kind
                    && actual != expected
                {
                    return Err(ExtensionSelectionError::KindMismatch {
                        id,
                        expected,
                        actual,
                    });
                }
                Ok(extension)
            })
            .collect::<Result<_, _>>()?;
        Ok(ExtensionSelection { extensions })
    }

    /// Compares this current snapshot with a previous snapshot in exact-ID order.
    ///
    /// Comparison is pure: it performs no filesystem access and changes neither registry.
    pub fn changes_from<'a>(&'a self, previous: &'a Self) -> Vec<ExtensionRegistryChange<'a>> {
        let ids = self
            .indices_by_id
            .keys()
            .chain(previous.indices_by_id.keys())
            .map(String::as_str)
            .collect::<BTreeSet<_>>();

        ids.into_iter()
            .filter_map(|id| match (previous.get(id), self.get(id)) {
                (None, Some(current)) => Some(ExtensionRegistryChange::Added(current)),
                (Some(previous), None) => Some(ExtensionRegistryChange::Removed(previous)),
                (Some(previous), Some(current)) if previous != current => {
                    Some(ExtensionRegistryChange::Changed { previous, current })
                }
                (Some(_), Some(_)) => None,
                (None, None) => unreachable!("ID originated in one registry"),
            })
            .collect()
    }
}

/// Discovers validated manifests at `root/*/extension.yaml` without activating them.
pub fn discover_extensions(
    root: impl AsRef<Path>,
) -> Result<Vec<DiscoveredExtension>, ExtensionDiscoveryError> {
    discover_extensions_platform(root.as_ref())
}

/// Reopens selected tool packages beneath their original root and returns bounded owned bytes.
///
/// Preparation is descriptor-anchored and all-or-nothing. It does not compile, register, authorize,
/// persist, or execute the returned artifacts.
pub fn prepare_tool_artifacts(
    root: impl AsRef<Path>,
    selection: &ExtensionSelection<'_>,
) -> Result<Vec<PreparedToolArtifact>, ExtensionPreparationError> {
    prepare_tool_artifacts_platform(root.as_ref(), selection)
}

#[cfg(unix)]
fn prepare_tool_artifacts_platform(
    root: &Path,
    selection: &ExtensionSelection<'_>,
) -> Result<Vec<PreparedToolArtifact>, ExtensionPreparationError> {
    let root_fd = open(
        root,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|source| ExtensionPreparationError::ReadRoot {
        path: root.to_path_buf(),
        source: io::Error::from(source),
    })?;
    let root_identity =
        file_identity(&root_fd).map_err(|source| ExtensionPreparationError::ReadRoot {
            path: root.to_path_buf(),
            source,
        })?;

    if let Some(extension) = selection.extensions().next()
        && root_identity != extension.root_identity
    {
        return Err(ExtensionPreparationError::SourceMismatch {
            id: extension.manifest().id().to_owned(),
            path: extension.path().to_path_buf(),
        });
    }

    let mut artifacts = Vec::with_capacity(selection.len());
    for extension in selection.extensions() {
        let id = extension.manifest().id().to_owned();
        if extension.manifest().kind() != ExtensionKind::Tool {
            return Err(ExtensionPreparationError::KindMismatch {
                id,
                actual: extension.manifest().kind(),
            });
        }
        let package_name = selected_package_name(root, extension).ok_or_else(|| {
            ExtensionPreparationError::SourceMismatch {
                id: id.clone(),
                path: extension.path().to_path_buf(),
            }
        })?;
        let package_path = root.join(package_name);
        let package_fd = openat(
            &root_fd,
            package_name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|source| ExtensionPreparationError::Package {
            id: id.clone(),
            path: package_path.clone(),
            source: io::Error::from(source),
        })?;
        let package_identity =
            file_identity(&package_fd).map_err(|source| ExtensionPreparationError::Package {
                id: id.clone(),
                path: package_path.clone(),
                source,
            })?;
        if package_identity != extension.package_identity {
            return Err(ExtensionPreparationError::PackageChanged {
                id,
                path: package_path,
            });
        }

        let manifest_path = package_path.join("extension.yaml");
        let manifest_fd = openat(
            &package_fd,
            c"extension.yaml",
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|source| ExtensionPreparationError::Manifest {
            id: id.clone(),
            path: manifest_path.clone(),
            source: ExtensionManifestError::Read {
                source: io::Error::from(source),
            },
        })?;
        let manifest_file = fs::File::from(manifest_fd);
        let metadata =
            manifest_file
                .metadata()
                .map_err(|source| ExtensionPreparationError::Manifest {
                    id: id.clone(),
                    path: manifest_path.clone(),
                    source: ExtensionManifestError::Read { source },
                })?;
        if !metadata.is_file() {
            return Err(ExtensionPreparationError::Manifest {
                id,
                path: manifest_path,
                source: ExtensionManifestError::Read {
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "extension manifest is not a regular file",
                    ),
                },
            });
        }
        let manifest = ExtensionManifest::load_reader(manifest_file).map_err(|source| {
            ExtensionPreparationError::Manifest {
                id: id.clone(),
                path: manifest_path.clone(),
                source,
            }
        })?;
        if &manifest != extension.manifest() {
            return Err(ExtensionPreparationError::ManifestChanged {
                id,
                path: manifest_path,
            });
        }

        let bytes = read_entrypoint(
            &package_fd,
            &id,
            &package_path,
            extension.manifest().entrypoint(),
        )?;
        artifacts.push(PreparedToolArtifact { id, bytes });
    }
    Ok(artifacts)
}

#[cfg(unix)]
fn selected_package_name<'a>(root: &Path, extension: &'a DiscoveredExtension) -> Option<&'a OsStr> {
    let relative = extension.path().strip_prefix(root).ok()?;
    let mut components = relative.components();
    match (components.next(), components.next(), components.next()) {
        (
            Some(std::path::Component::Normal(package)),
            Some(std::path::Component::Normal(manifest)),
            None,
        ) if manifest == "extension.yaml" => Some(package),
        _ => None,
    }
}

#[cfg(unix)]
fn file_identity(file: &impl AsFd) -> io::Result<FileIdentity> {
    let metadata = fstat(file).map_err(io::Error::from)?;
    Ok(FileIdentity {
        device: metadata.st_dev,
        inode: metadata.st_ino,
    })
}

#[cfg(unix)]
fn read_entrypoint(
    package: &impl AsFd,
    id: &str,
    package_path: &Path,
    entrypoint: &str,
) -> Result<Vec<u8>, ExtensionPreparationError> {
    let mut current_directory = None;
    let mut components = entrypoint.split('/').peekable();
    while let Some(component) = components.next() {
        let parent = current_directory
            .as_ref()
            .map_or_else(|| package.as_fd(), AsFd::as_fd);
        if components.peek().is_some() {
            current_directory = Some(
                openat(
                    parent,
                    component,
                    OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                    Mode::empty(),
                )
                .map_err(|source| ExtensionPreparationError::Entrypoint {
                    id: id.to_owned(),
                    path: package_path.to_path_buf(),
                    entrypoint: entrypoint.to_owned(),
                    source: io::Error::from(source),
                })?,
            );
            continue;
        }

        let target = openat(
            parent,
            component,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|source| ExtensionPreparationError::Entrypoint {
            id: id.to_owned(),
            path: package_path.to_path_buf(),
            entrypoint: entrypoint.to_owned(),
            source: io::Error::from(source),
        })?;
        let mut file = fs::File::from(target);
        let metadata = file
            .metadata()
            .map_err(|source| ExtensionPreparationError::Entrypoint {
                id: id.to_owned(),
                path: package_path.to_path_buf(),
                entrypoint: entrypoint.to_owned(),
                source,
            })?;
        if !metadata.is_file() {
            return Err(ExtensionPreparationError::Entrypoint {
                id: id.to_owned(),
                path: package_path.to_path_buf(),
                entrypoint: entrypoint.to_owned(),
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "extension entrypoint target is not a regular file",
                ),
            });
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_ENTRYPOINT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| ExtensionPreparationError::Entrypoint {
                id: id.to_owned(),
                path: package_path.to_path_buf(),
                entrypoint: entrypoint.to_owned(),
                source,
            })?;
        if bytes.len() > MAX_ENTRYPOINT_BYTES as usize {
            return Err(ExtensionPreparationError::EntrypointTooLarge {
                id: id.to_owned(),
                path: package_path.to_path_buf(),
                max_bytes: MAX_ENTRYPOINT_BYTES,
            });
        }
        return Ok(bytes);
    }
    unreachable!("validated entrypoints contain a component")
}

#[cfg(not(unix))]
fn prepare_tool_artifacts_platform(
    root: &Path,
    _selection: &ExtensionSelection<'_>,
) -> Result<Vec<PreparedToolArtifact>, ExtensionPreparationError> {
    Err(ExtensionPreparationError::ReadRoot {
        path: root.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::Unsupported,
            "secure extension preparation is unsupported on this platform",
        ),
    })
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
    let root_identity =
        file_identity(&root_fd).map_err(|source| ExtensionDiscoveryError::ReadRoot {
            path: root.to_path_buf(),
            source,
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
    let mut paths_by_id: BTreeMap<String, PathBuf> = BTreeMap::new();
    for child_name in children {
        let child_path = root.join(OsStr::from_bytes(child_name.to_bytes()));
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
                    path: child_path,
                    source: io::Error::from(source),
                });
            }
        };
        let package_identity =
            file_identity(&child_fd).map_err(|source| ExtensionDiscoveryError::ReadRoot {
                path: child_path.clone(),
                source,
            })?;
        let path = child_path.join("extension.yaml");
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
        validate_entrypoint_target(&child_fd, manifest.entrypoint()).map_err(|source| {
            ExtensionDiscoveryError::Entrypoint {
                path: path.clone(),
                entrypoint: manifest.entrypoint().to_owned(),
                source,
            }
        })?;
        if let Some(first_path) = paths_by_id.get(manifest.id()) {
            return Err(ExtensionDiscoveryError::DuplicateId {
                id: manifest.id().to_owned(),
                first_path: first_path.clone(),
                duplicate_path: path,
            });
        }
        paths_by_id.insert(manifest.id().to_owned(), path.clone());
        discovered.push(DiscoveredExtension {
            path,
            manifest,
            root_identity,
            package_identity,
        });
    }
    Ok(discovered)
}

#[cfg(unix)]
fn validate_entrypoint_target(directory: &impl AsFd, entrypoint: &str) -> io::Result<()> {
    let mut current_directory = None;
    let mut components = entrypoint.split('/').peekable();

    while let Some(component) = components.next() {
        let parent = current_directory
            .as_ref()
            .map_or_else(|| directory.as_fd(), AsFd::as_fd);
        if components.peek().is_none() {
            let metadata =
                statat(parent, component, AtFlags::SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
            return if FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "extension entrypoint target is not a regular file",
                ))
            };
        }

        current_directory = Some(
            openat(
                parent,
                component,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
    }

    unreachable!("validated entrypoints contain a component")
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

fn validate_entrypoint(entrypoint: &str) -> Result<(), ExtensionManifestError> {
    let has_invalid_component = entrypoint.split('/').any(|component| {
        component.trim().is_empty()
            || matches!(component, "." | "..")
            || component.ends_with([' ', '.'])
            || component.chars().any(|character| {
                character.is_ascii_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\')
            })
            || is_windows_reserved_name(component)
    });

    if entrypoint.starts_with('/') || has_invalid_component {
        Err(ExtensionManifestError::InvalidEntrypoint)
    } else {
        Ok(())
    }
}

fn is_windows_reserved_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches([' ', '.']);
    if ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return true;
    }

    let mut characters = stem.chars();
    let prefix = (
        characters
            .next()
            .map(|character| character.to_ascii_uppercase()),
        characters
            .next()
            .map(|character| character.to_ascii_uppercase()),
        characters
            .next()
            .map(|character| character.to_ascii_uppercase()),
    );
    let suffix = characters.next();
    matches!(
        prefix,
        (Some('C'), Some('O'), Some('M')) | (Some('L'), Some('P'), Some('T'))
    ) && matches!(suffix, Some('1'..='9' | '¹' | '²' | '³'))
        && characters.next().is_none()
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

impl fmt::Display for ExtensionPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadRoot { path, source } => write!(
                formatter,
                "could not reopen extension root {}: {source}",
                path.display()
            ),
            Self::SourceMismatch { id, path } => write!(
                formatter,
                "selected extension {id:?} does not belong to the supplied root at {}",
                path.display()
            ),
            Self::KindMismatch { id, actual } => write!(
                formatter,
                "selected extension {id:?} has kind {actual:?}, expected Tool"
            ),
            Self::Package { id, path, source } => write!(
                formatter,
                "could not reopen selected extension package {id:?} at {}: {source}",
                path.display()
            ),
            Self::PackageChanged { id, path } => write!(
                formatter,
                "selected extension package {id:?} changed at {}",
                path.display()
            ),
            Self::Manifest { id, path, source } => write!(
                formatter,
                "could not revalidate selected extension manifest {id:?} at {}: {source}",
                path.display()
            ),
            Self::ManifestChanged { id, path } => write!(
                formatter,
                "selected extension manifest {id:?} changed at {}",
                path.display()
            ),
            Self::Entrypoint {
                id,
                path,
                entrypoint,
                source,
            } => write!(
                formatter,
                "could not reopen selected tool entrypoint {entrypoint:?} for {id:?} beneath {}: {source}",
                path.display()
            ),
            Self::EntrypointTooLarge {
                id,
                path,
                max_bytes,
            } => write!(
                formatter,
                "selected tool entrypoint for {id:?} beneath {} exceeds {max_bytes} bytes",
                path.display()
            ),
        }
    }
}

impl Error for ExtensionPreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadRoot { source, .. }
            | Self::Package { source, .. }
            | Self::Entrypoint { source, .. } => Some(source),
            Self::Manifest { source, .. } => Some(source),
            Self::SourceMismatch { .. }
            | Self::KindMismatch { .. }
            | Self::PackageChanged { .. }
            | Self::ManifestChanged { .. }
            | Self::EntrypointTooLarge { .. } => None,
        }
    }
}

impl fmt::Display for ExtensionSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { id } => {
                write!(
                    formatter,
                    "extension selection contains duplicate ID {id:?}"
                )
            }
            Self::NotFound { id } => write!(formatter, "extension ID {id:?} was not found"),
            Self::KindMismatch {
                id,
                expected,
                actual,
            } => write!(
                formatter,
                "extension ID {id:?} has kind {actual:?}, expected {expected:?}"
            ),
        }
    }
}

impl Error for ExtensionSelectionError {}

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
    InvalidEntrypoint,
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
            Self::InvalidEntrypoint => write!(
                formatter,
                "extension manifest field entrypoint must be a portable relative path"
            ),
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
            | Self::BlankField { .. }
            | Self::InvalidEntrypoint => None,
        }
    }
}

/// A deterministic extension-root enumeration, candidate validation, or identity failure.
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
    Entrypoint {
        path: PathBuf,
        entrypoint: String,
        source: io::Error,
    },
    DuplicateId {
        id: String,
        first_path: PathBuf,
        duplicate_path: PathBuf,
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
            Self::Entrypoint {
                path,
                entrypoint,
                source,
            } => write!(
                formatter,
                "invalid extension entrypoint {entrypoint:?} declared by {}: {source}",
                path.display()
            ),
            Self::DuplicateId {
                id,
                first_path,
                duplicate_path,
            } => write!(
                formatter,
                "duplicate extension ID {id:?} in {} and {}",
                first_path.display(),
                duplicate_path.display()
            ),
        }
    }
}

impl Error for ExtensionDiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadRoot { source, .. } => Some(source),
            Self::Manifest { source, .. } => Some(source),
            Self::Entrypoint { source, .. } => Some(source),
            Self::DuplicateId { .. } => None,
        }
    }
}
