pub mod record;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use record::DevelopmentRecord;

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
            _ => ExitCode::SUCCESS,
        }
    }
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
