use std::{
    error::Error,
    fmt,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use tempfile::NamedTempFile;

/// One explicit execution request for a selected live Jupyter notebook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonExecutionRequest {
    port: u16,
    notebook_path: String,
    source: String,
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

        let output = Command::new(&self.program)
            .arg("execute")
            .arg("--port")
            .arg(request.port().to_string())
            .arg("--path")
            .arg(request.notebook_path())
            .arg("--code-file")
            .arg(source_file.path())
            .arg("--compact")
            .output()
            .map_err(PythonExecutionError::Launch)?;

        if !output.status.success() {
            return Err(PythonExecutionError::AdapterExited {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(PythonExecutionError::InvalidResponse)?;
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

#[derive(Debug)]
#[non_exhaustive]
pub enum PythonExecutionError {
    SourceFile(std::io::Error),
    Launch(std::io::Error),
    AdapterExited { code: Option<i32>, stderr: String },
    InvalidResponse(serde_json::Error),
    MissingStatus,
    ExecutionFailed { status: String, response: Value },
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
            Self::SourceFile(error) => Some(error),
            Self::Launch(error) => Some(error),
            Self::InvalidResponse(error) => Some(error),
            Self::AdapterExited { .. } | Self::MissingStatus | Self::ExecutionFailed { .. } => None,
        }
    }
}
