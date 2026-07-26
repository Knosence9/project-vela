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
    sync::{Arc, OnceLock, mpsc},
    thread,
    time::Duration,
};

#[cfg(unix)]
use std::{
    ffi::{OsStr, OsString},
    os::fd::AsFd,
    os::unix::ffi::OsStrExt,
};

#[cfg(unix)]
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, fstat, open, openat, statat};
use serde::Deserialize;
use vela_kernel::tool::{
    Tool, ToolEffect, ToolError, ToolId, ToolIdError, ToolRegistry, ToolRegistryError,
    ToolRegistryRemovalError, ToolRegistryReplacementError,
};
use wasmtime::component::{
    Component, Linker,
    types::{ComponentItem, Type},
};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

const SUPPORTED_MANIFEST_VERSION: u64 = 1;

/// Maximum accepted encoded manifest size.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Maximum accepted encoded WebAssembly component size during artifact preparation.
pub const MAX_ENTRYPOINT_BYTES: u64 = 16 * 1024 * 1024;

/// Default maximum bytes available to each guest linear memory during one invocation.
pub const DEFAULT_TOOL_MEMORY_BYTES: usize = 16 * 1024 * 1024;
/// Default maximum elements available to each guest table during one invocation.
pub const DEFAULT_TOOL_TABLE_ELEMENTS: usize = 10_000;
/// Default maximum core instances created in one invocation store.
pub const DEFAULT_TOOL_INSTANCES: usize = 100;
/// Default maximum linear memories created in one invocation store.
pub const DEFAULT_TOOL_MEMORIES: usize = 10;
/// Default maximum tables created in one invocation store.
pub const DEFAULT_TOOL_TABLES: usize = 10;
/// Default Wasmtime fuel available to one invocation.
pub const DEFAULT_TOOL_FUEL: u64 = 10_000_000;
/// Default wall-clock epoch deadline for one invocation.
pub const DEFAULT_TOOL_EPOCH_DEADLINE: Duration = Duration::from_secs(1);

const TOOL_EPOCH_TICK: Duration = Duration::from_millis(10);

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
#[derive(Clone, Eq, PartialEq)]
pub struct PreparedToolArtifact {
    id: String,
    bytes: Vec<u8>,
}

impl fmt::Debug for PreparedToolArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedToolArtifact")
            .field("id", &self.id)
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

impl PreparedToolArtifact {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// One inert WebAssembly tool component compiled against Vela's version 0.1.0 ABI.
#[derive(Clone)]
pub struct CompiledToolComponent {
    id: String,
    component: Component,
    epoch_ticker: Arc<OnceLock<EpochTicker>>,
}

impl fmt::Debug for CompiledToolComponent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledToolComponent")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl CompiledToolComponent {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the inert compiled component for later controlled activation.
    pub fn component(&self) -> &Component {
        &self.component
    }
}

/// Per-invocation Wasmtime resource policy for a component-backed tool.
///
/// Limits are implementation policy rather than part of the guest ABI. A fresh store receives
/// this complete policy before each instantiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolExecutionLimits {
    pub max_memory_bytes: usize,
    pub max_table_elements: usize,
    pub max_instances: usize,
    pub max_memories: usize,
    pub max_tables: usize,
    pub fuel: u64,
    pub epoch_deadline: Duration,
}

impl Default for ToolExecutionLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: DEFAULT_TOOL_MEMORY_BYTES,
            max_table_elements: DEFAULT_TOOL_TABLE_ELEMENTS,
            max_instances: DEFAULT_TOOL_INSTANCES,
            max_memories: DEFAULT_TOOL_MEMORIES,
            max_tables: DEFAULT_TOOL_TABLES,
            fuel: DEFAULT_TOOL_FUEL,
            epoch_deadline: DEFAULT_TOOL_EPOCH_DEADLINE,
        }
    }
}

/// An inert kernel tool adapter around one compiled no-import component.
pub struct ComponentTool {
    id: ToolId,
    component: Component,
    epoch_ticker: Arc<OnceLock<EpochTicker>>,
    limits: ToolExecutionLimits,
}

impl fmt::Debug for ComponentTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComponentTool")
            .field("id", &self.id)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl ComponentTool {
    /// Adapts an inert compiled component using the default per-invocation resource policy.
    pub fn new(component: CompiledToolComponent) -> Result<Self, ComponentToolConstructionError> {
        Self::with_limits(component, ToolExecutionLimits::default())
    }

    /// Adapts an inert compiled component using an explicit per-invocation resource policy.
    pub fn with_limits(
        component: CompiledToolComponent,
        limits: ToolExecutionLimits,
    ) -> Result<Self, ComponentToolConstructionError> {
        let id = ToolId::new(component.id.clone())
            .map_err(|source| ComponentToolConstructionError { source })?;
        Ok(Self {
            id,
            component: component.component,
            epoch_ticker: component.epoch_ticker,
            limits,
        })
    }

    pub fn limits(&self) -> ToolExecutionLimits {
        self.limits
    }
}

/// A compiled manifest ID that cannot be represented by the kernel tool protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentToolConstructionError {
    source: ToolIdError,
}

impl fmt::Display for ComponentToolConstructionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("compiled component has an invalid tool ID")
    }
}

impl Error for ComponentToolConstructionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// A typed failure while atomically activating one selected tool batch.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolActivationError {
    Preparation {
        source: ExtensionPreparationError,
    },
    Compilation {
        source: ToolComponentCompilationError,
    },
    Construction {
        id: String,
        source: ComponentToolConstructionError,
    },
    Registration {
        source: ToolRegistryError,
    },
}

impl fmt::Display for ToolActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation { .. } => formatter.write_str("failed to prepare selected tools"),
            Self::Compilation { .. } => formatter.write_str("failed to compile selected tools"),
            Self::Construction { id, .. } => {
                write!(formatter, "failed to construct selected tool {id}")
            }
            Self::Registration { .. } => {
                formatter.write_str("failed to register selected tools atomically")
            }
        }
    }
}

impl Error for ToolActivationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Preparation { source } => Some(source),
            Self::Compilation { source } => Some(source),
            Self::Construction { source, .. } => Some(source),
            Self::Registration { source } => Some(source),
        }
    }
}

/// A typed failure while atomically deactivating one selected tool batch.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolDeactivationError {
    WrongKind { id: String, actual: ExtensionKind },
    Registry { source: ToolRegistryRemovalError },
}

impl fmt::Display for ToolDeactivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongKind { id, actual } => {
                write!(
                    formatter,
                    "selected capability {id} is {actual:?}, not Tool"
                )
            }
            Self::Registry { .. } => formatter.write_str("failed to unregister selected tools"),
        }
    }
}

impl Error for ToolDeactivationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WrongKind { .. } => None,
            Self::Registry { source } => Some(source),
        }
    }
}

/// A typed failure while atomically replacing one selected active tool batch.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolReplacementError {
    Preparation {
        source: ExtensionPreparationError,
    },
    Compilation {
        source: ToolComponentCompilationError,
    },
    Construction {
        id: String,
        source: ComponentToolConstructionError,
    },
    Registry {
        source: ToolRegistryReplacementError,
    },
}

impl fmt::Display for ToolReplacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation { .. } => {
                formatter.write_str("failed to prepare selected replacement tools")
            }
            Self::Compilation { .. } => {
                formatter.write_str("failed to compile selected replacement tools")
            }
            Self::Construction { id, .. } => {
                write!(
                    formatter,
                    "failed to construct selected replacement tool {id}"
                )
            }
            Self::Registry { .. } => {
                formatter.write_str("failed to replace selected tools atomically")
            }
        }
    }
}

impl Error for ToolReplacementError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Preparation { source } => Some(source),
            Self::Compilation { source } => Some(source),
            Self::Construction { source, .. } => Some(source),
            Self::Registry { source } => Some(source),
        }
    }
}

#[derive(Debug)]
enum ToolAdapterBatchError {
    Preparation(ExtensionPreparationError),
    Compilation(ToolComponentCompilationError),
    Construction {
        id: String,
        source: ComponentToolConstructionError,
    },
}

impl From<ToolAdapterBatchError> for ToolActivationError {
    fn from(error: ToolAdapterBatchError) -> Self {
        match error {
            ToolAdapterBatchError::Preparation(source) => Self::Preparation { source },
            ToolAdapterBatchError::Compilation(source) => Self::Compilation { source },
            ToolAdapterBatchError::Construction { id, source } => Self::Construction { id, source },
        }
    }
}

impl From<ToolAdapterBatchError> for ToolReplacementError {
    fn from(error: ToolAdapterBatchError) -> Self {
        match error {
            ToolAdapterBatchError::Preparation(source) => Self::Preparation { source },
            ToolAdapterBatchError::Compilation(source) => Self::Compilation { source },
            ToolAdapterBatchError::Construction { id, source } => Self::Construction { id, source },
        }
    }
}

/// A typed failure from one isolated component invocation.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolComponentInvocationError {
    Input { source: serde_json::Error },
    Execution { source: wasmtime::Error },
    Guest { source: GuestToolError },
    Output { source: serde_json::Error },
}

impl fmt::Display for ToolComponentInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input { .. } => formatter.write_str("failed to serialize tool input as JSON"),
            Self::Execution { .. } => formatter.write_str("component tool execution failed"),
            Self::Guest { source } => {
                write!(formatter, "component tool returned an error: {source}")
            }
            Self::Output { .. } => formatter.write_str("component tool returned invalid JSON"),
        }
    }
}

impl Error for ToolComponentInvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input { source } | Self::Output { source } => Some(source),
            Self::Execution { source } => Some(source.as_ref()),
            Self::Guest { source } => Some(source),
        }
    }
}

/// Untrusted diagnostic text returned through the guest ABI's error case.
#[derive(Debug)]
pub struct GuestToolError {
    diagnostic: String,
}

impl GuestToolError {
    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl fmt::Display for GuestToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl Error for GuestToolError {}

impl Tool for ComponentTool {
    fn id(&self) -> &ToolId {
        &self.id
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }

    fn invoke(&mut self, input: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        self.invoke_component(input).map_err(ToolError::new)
    }
}

impl ComponentTool {
    fn invoke_component(
        &self,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolComponentInvocationError> {
        let input = serde_json::to_string(input)
            .map_err(|source| ToolComponentInvocationError::Input { source })?;
        let engine = self.component.engine();
        let mut store = Store::new(engine, self.store_limits());
        store.limiter(|limits| limits);
        store
            .set_fuel(self.limits.fuel)
            .map_err(|source| ToolComponentInvocationError::Execution { source })?;
        self.epoch_ticker
            .get_or_init(|| EpochTicker::start(engine.clone()));
        store.set_epoch_deadline(epoch_ticks(self.limits.epoch_deadline));

        let linker = Linker::new(engine);
        let instance = linker
            .instantiate(&mut store, &self.component)
            .map_err(|source| ToolComponentInvocationError::Execution { source })?;
        let invoke = instance
            .get_typed_func::<(String,), (Result<String, String>,)>(&mut store, "invoke")
            .map_err(|source| ToolComponentInvocationError::Execution { source })?;
        let (result,) = invoke
            .call(&mut store, (input,))
            .map_err(|source| ToolComponentInvocationError::Execution { source })?;
        let output = result.map_err(|diagnostic| ToolComponentInvocationError::Guest {
            source: GuestToolError { diagnostic },
        })?;
        serde_json::from_str(&output)
            .map_err(|source| ToolComponentInvocationError::Output { source })
    }

    fn store_limits(&self) -> StoreLimits {
        StoreLimitsBuilder::new()
            .memory_size(self.limits.max_memory_bytes)
            .table_elements(self.limits.max_table_elements)
            .instances(self.limits.max_instances)
            .memories(self.limits.max_memories)
            .tables(self.limits.max_tables)
            .trap_on_grow_failure(true)
            .build()
    }
}

fn epoch_ticks(deadline: Duration) -> u64 {
    deadline
        .as_nanos()
        .div_ceil(TOOL_EPOCH_TICK.as_nanos())
        .clamp(1, u64::MAX.into()) as u64
}

#[derive(Debug)]
struct EpochTicker {
    cancel: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl EpochTicker {
    fn start(engine: Engine) -> Self {
        let (cancel, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            while let Err(mpsc::RecvTimeoutError::Timeout) = receiver.recv_timeout(TOOL_EPOCH_TICK)
            {
                engine.increment_epoch();
            }
        });
        Self {
            cancel,
            worker: Some(worker),
        }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// A structural mismatch with `vela:extension/tool@0.1.0`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ToolComponentAbiError {
    Imports,
    Exports,
    InvokeType,
}

/// A fail-closed tool component engine, compilation, or ABI error.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolComponentCompilationError {
    Engine {
        source: wasmtime::Error,
    },
    Artifact {
        id: String,
        source: wasmtime::Error,
    },
    Abi {
        id: String,
        source: ToolComponentAbiError,
    },
}

impl ToolComponentCompilationError {
    /// Identifies the failing artifact, or `None` when engine creation itself failed.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Engine { .. } => None,
            Self::Artifact { id, .. } | Self::Abi { id, .. } => Some(id),
        }
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
///
/// # Platform support
///
/// Only Unix targets are supported. Other targets return [`ExtensionPreparationError::ReadRoot`]
/// with [`io::ErrorKind::Unsupported`].
pub fn prepare_tool_artifacts(
    root: impl AsRef<Path>,
    selection: &ExtensionSelection<'_>,
) -> Result<Vec<PreparedToolArtifact>, ExtensionPreparationError> {
    prepare_tool_artifacts_platform(root.as_ref(), selection)
}

/// Compiles prepared tools against the exact no-import `vela:extension/tool@0.1.0` ABI.
///
/// Compilation is inert and all-or-nothing: it creates no store or instance, calls no guest code,
/// and performs no registration, authorization, or persistence.
pub fn compile_tool_components(
    artifacts: &[PreparedToolArtifact],
) -> Result<Vec<CompiledToolComponent>, ToolComponentCompilationError> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    config.epoch_interruption(true);
    let engine =
        Engine::new(&config).map_err(|source| ToolComponentCompilationError::Engine { source })?;
    let epoch_ticker = Arc::new(OnceLock::new());
    let mut compiled = Vec::with_capacity(artifacts.len());

    for artifact in artifacts {
        let id = artifact.id().to_owned();
        let component = Component::new(&engine, artifact.bytes()).map_err(|source| {
            ToolComponentCompilationError::Artifact {
                id: id.clone(),
                source,
            }
        })?;
        validate_tool_component_abi(&engine, &component).map_err(|source| {
            ToolComponentCompilationError::Abi {
                id: id.clone(),
                source,
            }
        })?;
        compiled.push(CompiledToolComponent {
            id,
            component,
            epoch_ticker: Arc::clone(&epoch_ticker),
        });
    }

    Ok(compiled)
}

/// Revalidates, compiles, adapts with default limits, and atomically registers one selected tool
/// batch.
///
/// Every stage completes for the full selection before the caller-owned registry is mutated.
/// Compilation and adapter construction are inert; guest code can run only through a later
/// registry invocation and its existing authorization boundary.
pub fn activate_tool_selection(
    root: impl AsRef<Path>,
    selection: &ExtensionSelection<'_>,
    registry: &mut ToolRegistry,
) -> Result<(), ToolActivationError> {
    activate_tool_selection_with_limits(root, selection, registry, ToolExecutionLimits::default())
}

/// Revalidates, compiles, adapts with uniform caller-selected limits, and atomically registers one
/// selected tool batch.
///
/// Restrictive limits do not instantiate or invoke guests during activation. They are installed in
/// a fresh store only after a later registry invocation passes the existing authorization boundary.
pub fn activate_tool_selection_with_limits(
    root: impl AsRef<Path>,
    selection: &ExtensionSelection<'_>,
    registry: &mut ToolRegistry,
    limits: ToolExecutionLimits,
) -> Result<(), ToolActivationError> {
    let tools =
        build_tool_adapter_batch(root, selection, limits).map_err(ToolActivationError::from)?;
    registry
        .register_all(tools)
        .map_err(|source| ToolActivationError::Registration { source })
}

/// Atomically removes the exact adapters named by one selected tool batch.
///
/// Deactivation performs no filesystem access, guest execution, or authorization. The immutable
/// selection remains metadata and may be activated again only through the existing revalidation
/// boundary.
pub fn deactivate_tool_selection(
    selection: &ExtensionSelection<'_>,
    registry: &mut ToolRegistry,
) -> Result<(), ToolDeactivationError> {
    if let Some(extension) = selection
        .extensions()
        .find(|extension| extension.manifest().kind() != ExtensionKind::Tool)
    {
        return Err(ToolDeactivationError::WrongKind {
            id: extension.manifest().id().to_owned(),
            actual: extension.manifest().kind(),
        });
    }
    registry
        .unregister_all(selection.extensions().map(|extension| {
            ToolId::new(extension.manifest().id()).expect("validated non-blank ID")
        }))
        .map_err(|source| ToolDeactivationError::Registry { source })
}

/// Revalidates, compiles, adapts with default limits, and atomically replaces one selected active
/// tool batch.
///
/// Every stage completes for the full selection before the caller-owned registry is mutated.
pub fn replace_tool_selection(
    root: impl AsRef<Path>,
    selection: &ExtensionSelection<'_>,
    registry: &mut ToolRegistry,
) -> Result<(), ToolReplacementError> {
    replace_tool_selection_with_limits(root, selection, registry, ToolExecutionLimits::default())
}

/// Revalidates, compiles, adapts with uniform caller-selected limits, and atomically replaces one
/// selected active tool batch.
///
/// Restrictive limits remain inert until a later authorized invocation. Any preparation,
/// compilation, construction, or registry failure preserves every previously active adapter.
pub fn replace_tool_selection_with_limits(
    root: impl AsRef<Path>,
    selection: &ExtensionSelection<'_>,
    registry: &mut ToolRegistry,
    limits: ToolExecutionLimits,
) -> Result<(), ToolReplacementError> {
    let tools =
        build_tool_adapter_batch(root, selection, limits).map_err(ToolReplacementError::from)?;
    registry
        .replace_all(tools)
        .map_err(|source| ToolReplacementError::Registry { source })
}

fn build_tool_adapter_batch(
    root: impl AsRef<Path>,
    selection: &ExtensionSelection<'_>,
    limits: ToolExecutionLimits,
) -> Result<Vec<ComponentTool>, ToolAdapterBatchError> {
    if selection.is_empty() {
        return Ok(Vec::new());
    }
    let artifacts =
        prepare_tool_artifacts(root, selection).map_err(ToolAdapterBatchError::Preparation)?;
    let components =
        compile_tool_components(&artifacts).map_err(ToolAdapterBatchError::Compilation)?;
    components
        .into_iter()
        .map(|component| {
            let id = component.id().to_owned();
            ComponentTool::with_limits(component, limits)
                .map_err(|source| ToolAdapterBatchError::Construction { id, source })
        })
        .collect()
}

fn validate_tool_component_abi(
    engine: &Engine,
    component: &Component,
) -> Result<(), ToolComponentAbiError> {
    let component_type = component.component_type();
    if component_type.imports(engine).len() != 0 {
        return Err(ToolComponentAbiError::Imports);
    }

    let mut exports = component_type.exports(engine);
    let Some(("invoke", export)) = exports.next() else {
        return Err(ToolComponentAbiError::Exports);
    };
    if exports.next().is_some() {
        return Err(ToolComponentAbiError::Exports);
    }
    let ComponentItem::ComponentFunc(function) = export.ty else {
        return Err(ToolComponentAbiError::InvokeType);
    };
    if function.async_() {
        return Err(ToolComponentAbiError::InvokeType);
    }

    let mut parameters = function.params();
    if !matches!(parameters.next(), Some(("input", Type::String))) || parameters.next().is_some() {
        return Err(ToolComponentAbiError::InvokeType);
    }
    let mut results = function.results();
    let Some(Type::Result(result)) = results.next() else {
        return Err(ToolComponentAbiError::InvokeType);
    };
    if results.next().is_some()
        || result.ok() != Some(Type::String)
        || result.err() != Some(Type::String)
    {
        return Err(ToolComponentAbiError::InvokeType);
    }

    Ok(())
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

    let mut artifacts = Vec::with_capacity(selection.len());
    for extension in selection.extensions() {
        let id = extension.manifest().id().to_owned();
        if root_identity != extension.root_identity {
            return Err(ExtensionPreparationError::SourceMismatch {
                id,
                path: extension.path().to_path_buf(),
            });
        }
        if extension.manifest().kind() != ExtensionKind::Tool {
            return Err(ExtensionPreparationError::KindMismatch {
                id,
                actual: extension.manifest().kind(),
            });
        }
        let package_name = selected_package_name(extension).ok_or_else(|| {
            ExtensionPreparationError::SourceMismatch {
                id: id.clone(),
                path: extension.path().to_path_buf(),
            }
        })?;
        let package_path = root.join(&package_name);
        let package_fd = openat(
            &root_fd,
            &package_name,
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
fn selected_package_name(extension: &DiscoveredExtension) -> Option<OsString> {
    let manifest_path = extension.path();
    if manifest_path.file_name()? != "extension.yaml" {
        return None;
    }
    manifest_path.parent()?.file_name().map(OsStr::to_owned)
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

impl fmt::Display for ToolComponentAbiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Imports => "tool component must not import host capabilities",
            Self::Exports => "tool component must export only invoke",
            Self::InvokeType => {
                "tool component invoke must be synchronous (input: string) -> result<string, string>"
            }
        })
    }
}

impl Error for ToolComponentAbiError {}

impl fmt::Display for ToolComponentCompilationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine { source } => write!(
                formatter,
                "could not create tool component engine: {source}"
            ),
            Self::Artifact { id, source } => {
                write!(
                    formatter,
                    "could not compile tool component {id:?}: {source}"
                )
            }
            Self::Abi { id, source } => {
                write!(
                    formatter,
                    "tool component {id:?} has an incompatible ABI: {source}"
                )
            }
        }
    }
}

impl Error for ToolComponentCompilationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Engine { source } | Self::Artifact { source, .. } => Some(source.as_ref()),
            Self::Abi { source, .. } => Some(source),
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
