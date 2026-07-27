pub mod record;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use record::DevelopmentRecord;
use vela_extensions::{ExtensionKind, ExtensionRegistry, activate_tool_selection};
use vela_kernel::tool::{
    PermissionDecision, ToolAuthorizer, ToolEffect, ToolId, ToolRegistry, ToolRequest,
};

/// Project Vela's developer-facing command line.
#[derive(Debug, Parser)]
#[command(name = "vela-dev", about = "Developer tooling for Project Vela")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level developer workflows.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Work with Vela development records.
    Record {
        #[command(subcommand)]
        command: Option<RecordCommand>,
    },
    /// Inspect a directory of Vela development records.
    Corpus {
        #[command(subcommand)]
        command: Option<CorpusCommand>,
    },
    /// Work with validated Vela extension packages.
    Extension {
        #[command(subcommand)]
        command: Option<ExtensionCommand>,
    },
}

/// Extension-package workflows.
#[derive(Debug, Subcommand)]
pub enum ExtensionCommand {
    /// Discover and print one validated extension root.
    Inspect { root: PathBuf },
    /// Invoke one exact validated WebAssembly tool with a JSON value.
    Invoke {
        root: PathBuf,
        id: String,
        input_json: String,
    },
}

/// Corpus workflows.
#[derive(Debug, Subcommand)]
pub enum CorpusCommand {
    /// Recursively validate JSON records and summarize the corpus.
    Inspect { path: PathBuf },
}

/// Development-record workflows.
#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    /// Validate one schema-versioned JSON development record.
    Validate { path: PathBuf },
}

impl Cli {
    #[must_use]
    pub fn run(self) -> ExitCode {
        match self.command {
            Some(Command::Record {
                command: Some(RecordCommand::Validate { path }),
            }) => validate_record(&path),
            Some(Command::Corpus {
                command: Some(CorpusCommand::Inspect { path }),
            }) => inspect_corpus(&path),
            Some(Command::Extension {
                command: Some(ExtensionCommand::Inspect { root }),
            }) => inspect_extensions(&root),
            Some(Command::Extension {
                command:
                    Some(ExtensionCommand::Invoke {
                        root,
                        id,
                        input_json,
                    }),
            }) => invoke_extension(&root, &id, &input_json),
            _ => ExitCode::SUCCESS,
        }
    }
}

fn invoke_extension(root: &Path, id: &str, input_json: &str) -> ExitCode {
    let input = match serde_json::from_str(input_json) {
        Ok(input) => input,
        Err(error) => return extension_error("invalid_tool_input", error),
    };
    let extensions = match ExtensionRegistry::discover(root) {
        Ok(extensions) => extensions,
        Err(error) => return extension_error("invalid_extension_root", error),
    };
    let selection = match extensions.select_kind(ExtensionKind::Tool, [id]) {
        Ok(selection) => selection,
        Err(error) => return extension_error("invalid_tool_selection", error),
    };
    let mut tools = ToolRegistry::new();
    if let Err(error) = activate_tool_selection(root, &selection, &mut tools) {
        return extension_error("tool_activation_failed", error);
    }
    let tool_id = match ToolId::new(id) {
        Ok(tool_id) => tool_id,
        Err(error) => return extension_error("tool_activation_failed", error),
    };
    let mut authorizer = OneShotPureAuthorization {
        tool_id: tool_id.clone(),
        available: true,
    };
    let output = match tools.invoke(&tool_id, &mut authorizer, &input) {
        Ok(output) => output,
        Err(error) => return extension_error("tool_invocation_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

struct OneShotPureAuthorization {
    tool_id: ToolId,
    available: bool,
}

impl ToolAuthorizer for OneShotPureAuthorization {
    fn authorize(&mut self, request: ToolRequest<'_>) -> PermissionDecision {
        if self.available
            && request.tool_id() == &self.tool_id
            && request.effect() == ToolEffect::Pure
        {
            self.available = false;
            PermissionDecision::Allow
        } else {
            PermissionDecision::Deny
        }
    }
}

fn extension_error(code: &str, error: impl std::fmt::Display) -> ExitCode {
    let diagnostic = error.to_string();
    eprintln!("$: {code}: {diagnostic:?}");
    ExitCode::from(1)
}

fn inspect_extensions(root: &Path) -> ExitCode {
    let registry = match ExtensionRegistry::discover(root) {
        Ok(registry) => registry,
        Err(error) => return extension_error("invalid_extension_root", error),
    };

    for extension in registry.extensions() {
        let manifest = extension.manifest();
        let kind = match manifest.kind() {
            ExtensionKind::Tool => "tool",
            ExtensionKind::Skill => "skill",
            ExtensionKind::Workflow => "workflow",
        };
        let path = extension
            .path()
            .strip_prefix(root)
            .unwrap_or(extension.path());
        println!(
            "{:?}\t{kind}\t{:?}\t{path:?}",
            manifest.id(),
            manifest.entrypoint()
        );
    }
    println!("inspected {} extensions", registry.extensions().len());
    ExitCode::SUCCESS
}

fn inspect_corpus(root: &Path) -> ExitCode {
    let mut paths = Vec::new();
    if let Err(error) = collect_json(root, &mut paths) {
        eprintln!("$: unreadable_corpus: {error}");
        return ExitCode::from(2);
    }
    paths.sort();

    let mut valid = 0;
    for path in &paths {
        let relative = path.strip_prefix(root).unwrap_or(path).display();
        let input = match fs::read_to_string(path) {
            Ok(input) => input,
            Err(error) => {
                eprintln!("{relative}: unreadable_record: {error}");
                continue;
            }
        };
        let record: DevelopmentRecord = match serde_json::from_str(&input) {
            Ok(record) => record,
            Err(error) => {
                eprintln!("{relative}: malformed_record: {error}");
                continue;
            }
        };
        let issues = record.validate();
        if issues.is_empty() {
            println!("{relative}: valid");
            valid += 1;
        } else {
            for issue in issues {
                eprintln!(
                    "{relative}: {}: {}: {}",
                    issue.path, issue.code, issue.message
                );
            }
        }
    }

    let invalid = paths.len() - valid;
    println!(
        "inspected {} records: {valid} valid, {invalid} invalid",
        paths.len()
    );
    if invalid == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn collect_json(directory: &Path, paths: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_json(&path, paths)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn validate_record(path: &Path) -> ExitCode {
    let input = match fs::read_to_string(path) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("$: unreadable_record: {error}");
            return ExitCode::from(2);
        }
    };
    let record: DevelopmentRecord = match serde_json::from_str(&input) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("$: malformed_record: {error}");
            return ExitCode::from(2);
        }
    };
    let issues = record.validate();
    if issues.is_empty() {
        println!("valid development record: {}", path.display());
        ExitCode::SUCCESS
    } else {
        for issue in issues {
            eprintln!("{}: {}: {}", issue.path, issue.code, issue.message);
        }
        ExitCode::from(1)
    }
}
