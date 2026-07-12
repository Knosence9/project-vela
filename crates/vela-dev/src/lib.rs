use clap::{Parser, Subcommand};

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
    Record,
}
