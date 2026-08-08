use std::{
    error::Error,
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use serde_json::Value;
use tempfile::NamedTempFile;

pub const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const POST_EXIT_DRAIN_GRACE: Duration = Duration::from_millis(20);

/// Runtime and per-stream capture budgets for one adapter process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PythonExecutionLimits {
    timeout: Duration,
    max_output_bytes: usize,
}

impl PythonExecutionLimits {
    pub fn new(
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Result<Self, PythonExecutionLimitsError> {
        if timeout.is_zero() {
            return Err(PythonExecutionLimitsError::ZeroTimeout);
        }
        if max_output_bytes == 0 {
            return Err(PythonExecutionLimitsError::ZeroOutputLimit);
        }
        Ok(Self {
            timeout,
            max_output_bytes,
        })
    }

    pub fn timeout(self) -> Duration {
        self.timeout
    }

    pub fn max_output_bytes(self) -> usize {
        self.max_output_bytes
    }
}

impl Default for PythonExecutionLimits {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_EXECUTION_TIMEOUT,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PythonExecutionLimitsError {
    ZeroTimeout,
    ZeroOutputLimit,
}

impl fmt::Display for PythonExecutionLimitsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroTimeout => formatter.write_str("Python execution timeout must not be zero"),
            Self::ZeroOutputLimit => {
                formatter.write_str("Python output byte limit must not be zero")
            }
        }
    }
}

impl Error for PythonExecutionLimitsError {}

/// One explicit execution request for a selected live Jupyter notebook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonExecutionRequest {
    port: u16,
    notebook_path: String,
    source: String,
    limits: PythonExecutionLimits,
}

impl PythonExecutionRequest {
    pub fn new(
        port: u16,
        notebook_path: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<Self, PythonExecutionRequestError> {
        let notebook_path = notebook_path.into();
        if port == 0 {
            return Err(PythonExecutionRequestError::InvalidPort);
        }
        if notebook_path.trim().is_empty() {
            return Err(PythonExecutionRequestError::BlankNotebookPath);
        }

        Ok(Self {
            port,
            notebook_path,
            source: source.into(),
            limits: PythonExecutionLimits::default(),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn notebook_path(&self) -> &str {
        &self.notebook_path
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn limits(&self) -> PythonExecutionLimits {
        self.limits
    }

    #[must_use]
    pub fn with_limits(mut self, limits: PythonExecutionLimits) -> Self {
        self.limits = limits;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PythonExecutionRequestError {
    InvalidPort,
    BlankNotebookPath,
}

impl fmt::Display for PythonExecutionRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPort => formatter.write_str("Python workbench port must not be zero"),
            Self::BlankNotebookPath => {
                formatter.write_str("Python workbench notebook path must not be blank")
            }
        }
    }
}

impl Error for PythonExecutionRequestError {}

/// A typed view over one successful compact hamelnb response.
#[derive(Clone, Debug, PartialEq)]
pub struct PythonExecutionResult {
    value: Value,
}

impl PythonExecutionResult {
    pub fn status(&self) -> &str {
        self.value["status"]
            .as_str()
            .expect("successful responses have a string status")
    }

    pub fn transport(&self) -> Option<&str> {
        self.value.get("transport").and_then(Value::as_str)
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
    }
}

/// A synchronous Rust control-plane adapter for the Python hamelnb helper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HamelnbProcessAdapter {
    program: PathBuf,
}

impl HamelnbProcessAdapter {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn execute(
        &self,
        request: &PythonExecutionRequest,
    ) -> Result<PythonExecutionResult, PythonExecutionError> {
        let mut source_file = NamedTempFile::new().map_err(PythonExecutionError::SourceFile)?;
        source_file
            .write_all(request.source().as_bytes())
            .and_then(|()| source_file.flush())
            .map_err(PythonExecutionError::SourceFile)?;

        let mut child = Command::new(&self.program)
            .arg("execute")
            .arg("--port")
            .arg(request.port().to_string())
            .arg("--path")
            .arg(request.notebook_path())
            .arg("--code-file")
            .arg(source_file.path())
            .arg("--compact")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(PythonExecutionError::Launch)?;

        let mut stdout = child.stdout.take().expect("piped stdout must be available");
        let mut stderr = child.stderr.take().expect("piped stderr must be available");
        if let Err(error) =
            set_nonblocking(&stdout, "stdout").and_then(|()| set_nonblocking(&stderr, "stderr"))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let limit = request.limits().max_output_bytes();
        let mut stdout_capture = StreamCapture::new("stdout", limit);
        let mut stderr_capture = StreamCapture::new("stderr", limit);

        let started = Instant::now();
        let mut status = None;
        let mut exited_at = None;
        loop {
            if let Err(error) = stdout_capture.drain(&mut stdout) {
                kill_and_reap_if_running(&mut child, status)?;
                return Err(error);
            }
            if let Err(error) = stderr_capture.drain(&mut stderr) {
                kill_and_reap_if_running(&mut child, status)?;
                return Err(error);
            }
            if status.is_none() {
                status = child.try_wait().map_err(PythonExecutionError::Wait)?;
                if status.is_some() {
                    if started.elapsed() >= request.limits().timeout() {
                        return Err(PythonExecutionError::TimedOut {
                            timeout: request.limits().timeout(),
                        });
                    }
                    exited_at = Some(Instant::now());
                }
            }
            if status.is_some() && stdout_capture.is_eof() && stderr_capture.is_eof() {
                break;
            }
            if exited_at.is_some_and(|exited_at| exited_at.elapsed() >= POST_EXIT_DRAIN_GRACE) {
                break;
            }
            if status.is_none() && started.elapsed() >= request.limits().timeout() {
                kill_and_reap_if_running(&mut child, status)?;
                return Err(PythonExecutionError::TimedOut {
                    timeout: request.limits().timeout(),
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = status.expect("completed capture requires exited child");
        let stdout = stdout_capture.into_bytes();
        let stderr = stderr_capture.into_bytes();

        if !status.success() {
            return Err(PythonExecutionError::AdapterExited {
                code: status.code(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            });
        }

        let value: Value =
            serde_json::from_slice(&stdout).map_err(PythonExecutionError::InvalidResponse)?;
        let status = value
            .get("status")
            .and_then(Value::as_str)
            .ok_or(PythonExecutionError::MissingStatus)?;
        if status != "ok" {
            return Err(PythonExecutionError::ExecutionFailed {
                status: status.to_owned(),
                response: value,
            });
        }

        Ok(PythonExecutionResult { value })
    }
}

fn set_nonblocking(
    stream: &impl std::os::fd::AsFd,
    stream_name: &'static str,
) -> Result<(), PythonExecutionError> {
    let flags = fcntl_getfl(stream).map_err(|error| PythonExecutionError::Capture {
        stream: stream_name,
        source: error.into(),
    })?;
    fcntl_setfl(stream, flags | OFlags::NONBLOCK).map_err(|error| PythonExecutionError::Capture {
        stream: stream_name,
        source: error.into(),
    })
}

struct StreamCapture {
    stream: &'static str,
    limit: usize,
    bytes: Vec<u8>,
    eof: bool,
}

impl StreamCapture {
    fn new(stream: &'static str, limit: usize) -> Self {
        Self {
            stream,
            limit,
            bytes: Vec::with_capacity(limit.min(8192)),
            eof: false,
        }
    }

    fn drain(&mut self, reader: &mut impl Read) -> Result<(), PythonExecutionError> {
        let mut buffer = [0_u8; 8192];
        let count = match reader.read(&mut buffer) {
            Ok(0) => {
                self.eof = true;
                return Ok(());
            }
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(source) => {
                return Err(PythonExecutionError::Capture {
                    stream: self.stream,
                    source,
                });
            }
        };
        if self.bytes.len().saturating_add(count) > self.limit {
            return Err(PythonExecutionError::OutputLimitExceeded {
                stream: self.stream,
                limit: self.limit,
            });
        }
        self.bytes.extend_from_slice(&buffer[..count]);
        Ok(())
    }

    fn is_eof(&self) -> bool {
        self.eof
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

fn kill_and_reap_if_running(
    child: &mut std::process::Child,
    status: Option<std::process::ExitStatus>,
) -> Result<(), PythonExecutionError> {
    if status.is_some() {
        return Ok(());
    }
    if child
        .try_wait()
        .map_err(PythonExecutionError::Wait)?
        .is_some()
    {
        return Ok(());
    }
    match child.kill() {
        Ok(()) => {
            child.wait().map_err(PythonExecutionError::Wait)?;
            Ok(())
        }
        Err(error) => {
            if child
                .try_wait()
                .map_err(PythonExecutionError::Wait)?
                .is_some()
            {
                Ok(())
            } else {
                Err(PythonExecutionError::Kill(error))
            }
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum PythonExecutionError {
    SourceFile(std::io::Error),
    Launch(std::io::Error),
    Kill(std::io::Error),
    Wait(std::io::Error),
    Capture {
        stream: &'static str,
        source: std::io::Error,
    },

    TimedOut {
        timeout: Duration,
    },
    OutputLimitExceeded {
        stream: &'static str,
        limit: usize,
    },
    AdapterExited {
        code: Option<i32>,
        stderr: String,
    },
    InvalidResponse(serde_json::Error),
    MissingStatus,
    ExecutionFailed {
        status: String,
        response: Value,
    },
}

impl fmt::Display for PythonExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceFile(error) => {
                write!(
                    formatter,
                    "could not prepare temporary Python source: {error}"
                )
            }
            Self::Launch(error) => write!(formatter, "could not launch hamelnb adapter: {error}"),
            Self::Kill(error) => write!(formatter, "could not kill hamelnb adapter: {error}"),
            Self::Wait(error) => write!(formatter, "could not wait for hamelnb adapter: {error}"),
            Self::Capture { stream, source } => {
                write!(formatter, "could not capture hamelnb {stream}: {source}")
            }

            Self::TimedOut { timeout } => {
                write!(
                    formatter,
                    "hamelnb adapter exceeded its {timeout:?} timeout"
                )
            }
            Self::OutputLimitExceeded { stream, limit } => write!(
                formatter,
                "hamelnb {stream} exceeded its {limit}-byte capture limit"
            ),
            Self::AdapterExited { code, stderr } => write!(
                formatter,
                "hamelnb adapter exited with code {}: {}",
                code.map_or_else(|| "signal".to_owned(), |code| code.to_string()),
                stderr.trim_end()
            ),
            Self::InvalidResponse(error) => {
                write!(formatter, "hamelnb adapter returned invalid JSON: {error}")
            }
            Self::MissingStatus => {
                formatter.write_str("hamelnb adapter response is missing a string status")
            }
            Self::ExecutionFailed { status, .. } => {
                write!(formatter, "Python execution failed with status {status}")
            }
        }
    }
}

impl Error for PythonExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SourceFile(error)
            | Self::Launch(error)
            | Self::Kill(error)
            | Self::Wait(error) => Some(error),
            Self::Capture { source, .. } => Some(source),
            Self::InvalidResponse(error) => Some(error),
            Self::TimedOut { .. }
            | Self::OutputLimitExceeded { .. }
            | Self::AdapterExited { .. }
            | Self::MissingStatus
            | Self::ExecutionFailed { .. } => None,
        }
    }
}
