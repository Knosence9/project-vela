mod interactive;
mod memory_skills_review;
mod runtime_ops;

use crate::cli::{Cli, Commands};
use anyhow::Result;

pub(crate) fn run_command(bootstrap: &vela_runtime::BootstrapReport, cli: &Cli) -> Result<()> {
    match &cli.command {
        Some(Commands::Chat(args)) => interactive::run_chat(bootstrap, args),
        None => interactive::run_default_chat(bootstrap, cli),
        Some(Commands::Status) => interactive::run_status(bootstrap),
        Some(Commands::Extensions(args)) => interactive::run_extensions(bootstrap, args),
        Some(Commands::Memory(args)) => memory_skills_review::run_memory(bootstrap, args),
        Some(Commands::Skills(args)) => memory_skills_review::run_skills(bootstrap, args),
        Some(Commands::Review(args)) => memory_skills_review::run_review(bootstrap, args),
        Some(Commands::Plan) => {
            println!("docs/vela-rust-agentic-os-plan.md");
            Ok(())
        }
        Some(Commands::Gateway(args)) => runtime_ops::run_gateway(bootstrap, args),
        Some(Commands::Sessions(args)) => runtime_ops::run_sessions(bootstrap, args),
        Some(Commands::Cron(args)) => runtime_ops::run_cron(bootstrap, args),
        Some(Commands::Logs(args)) => {
            println!(
                "logs placeholder: follow={} since={:?}",
                args.follow, args.since
            );
            Ok(())
        }
        Some(Commands::Dashboard(args)) => {
            println!(
                "dashboard placeholder: stop={} status={}",
                args.stop, args.status
            );
            Ok(())
        }
        Some(Commands::Setup) => {
            println!("placeholder command: Setup");
            Ok(())
        }
        Some(Commands::Model) => {
            println!("placeholder command: Model");
            Ok(())
        }
        Some(Commands::Config) => {
            println!("placeholder command: Config");
            Ok(())
        }
        Some(Commands::Tools) => {
            println!("placeholder command: Tools");
            Ok(())
        }
        Some(Commands::Mcp) => {
            println!("placeholder command: Mcp");
            Ok(())
        }
        Some(Commands::Update) => {
            println!("placeholder command: Update");
            Ok(())
        }
        Some(Commands::Auth) => {
            println!("placeholder command: Auth");
            Ok(())
        }
        Some(Commands::Pairing) => {
            println!("placeholder command: Pairing");
            Ok(())
        }
        Some(Commands::Version) => {
            println!("placeholder command: Version");
            Ok(())
        }
    }
}
