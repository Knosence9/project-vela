mod cli;
mod commands;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() -> Result<()> {
    let (argv, preparse_profile) = vela_runtime::preparse_profile_override(std::env::args())?;
    tracing_subscriber::fmt::init();
    let cli = Cli::parse_from(argv);

    if cli.version {
        println!("vela-rs 0.1.0-kernel");
        vela_runtime::bootstrap_banner();
        return Ok(());
    }

    let bootstrap = vela_runtime::initialize_bootstrap(
        preparse_profile.or_else(|| cli.profile.clone()),
        cli.ignore_user_config,
    )?;

    commands::run_command(&bootstrap, &cli)?;

    vela_runtime::bootstrap_banner();
    Ok(())
}
