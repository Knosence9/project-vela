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
    /// Inspect a directory of Vela development records.
    Corpus {
        #[command(subcommand)]
        command: Option<CorpusCommand>,
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
            _ => ExitCode::SUCCESS,
        }
    }
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
