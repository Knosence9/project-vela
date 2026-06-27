use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "vela")]
#[command(about = "Behavior-first Rust shell for Vela parity work")]
struct Cli {
    #[arg(short = 'z', long = "oneshot")]
    oneshot: Option<String>,
    #[arg(short = 'm', long = "model")]
    model: Option<String>,
    #[arg(long = "provider")]
    provider: Option<String>,
    #[arg(short = 't', long = "toolsets")]
    toolsets: Option<String>,
    #[arg(short = 'r', long = "resume")]
    resume: Option<String>,
    #[arg(short = 's', long = "skills")]
    skills: Vec<String>,
    #[arg(short = 'c', long = "continue")]
    continue_last: Option<String>,
    #[arg(short = 'w', long = "worktree", default_value_t = false)]
    worktree: bool,
    #[arg(long = "accept-hooks", default_value_t = false)]
    accept_hooks: bool,
    #[arg(long = "yolo", default_value_t = false)]
    yolo: bool,
    #[arg(long = "pass-session-id", default_value_t = false)]
    pass_session_id: bool,
    #[arg(long = "ignore-user-config", default_value_t = false)]
    ignore_user_config: bool,
    #[arg(long = "ignore-rules", default_value_t = false)]
    ignore_rules: bool,
    #[arg(long = "safe-mode", default_value_t = false)]
    safe_mode: bool,
    #[arg(short = 'p', long = "profile")]
    profile: Option<String>,
    #[arg(long = "cli", default_value_t = false)]
    cli_mode: bool,
    #[arg(long = "tui", default_value_t = false)]
    tui_mode: bool,
    #[arg(long = "version", default_value_t = false)]
    version: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Default, Args)]
struct ChatArgs {
    #[arg(short = 'q', long = "query")]
    query: Option<String>,
    #[arg(long = "image")]
    image: Option<String>,
    #[arg(short = 'm', long = "model")]
    model: Option<String>,
    #[arg(short = 't', long = "toolsets")]
    toolsets: Option<String>,
    #[arg(short = 's', long = "skills")]
    skills: Vec<String>,
    #[arg(long = "provider")]
    provider: Option<String>,
    #[arg(short = 'v', long = "verbose", default_value_t = false)]
    verbose: bool,
    #[arg(short = 'r', long = "resume")]
    resume: Option<String>,
    #[arg(short = 'c', long = "continue")]
    continue_last: Option<String>,
    #[arg(short = 'w', long = "worktree", default_value_t = false)]
    worktree: bool,
    #[arg(long = "accept-hooks", default_value_t = false)]
    accept_hooks: bool,
    #[arg(long = "checkpoints", default_value_t = false)]
    checkpoints: bool,
    #[arg(long = "max-turns")]
    max_turns: Option<u32>,
    #[arg(long = "yolo", default_value_t = false)]
    yolo: bool,
    #[arg(long = "pass-session-id", default_value_t = false)]
    pass_session_id: bool,
}

#[derive(Debug, Default, Args)]
struct GatewayArgs {
    #[arg(long = "setup", default_value_t = false)]
    setup: bool,
    #[arg(long = "start", default_value_t = false)]
    start: bool,
}

#[derive(Debug, Default, Args)]
struct SessionsArgs {
    #[arg(long = "list", default_value_t = false)]
    list: bool,
    #[arg(long = "browse", default_value_t = false)]
    browse: bool,
}

#[derive(Debug, Default, Args)]
struct LogsArgs {
    #[arg(short = 'f', long = "follow", default_value_t = false)]
    follow: bool,
    #[arg(long = "since")]
    since: Option<String>,
}

#[derive(Debug, Default, Args)]
struct DashboardArgs {
    #[arg(long = "stop", default_value_t = false)]
    stop: bool,
    #[arg(long = "status", default_value_t = false)]
    status: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Chat(ChatArgs),
    Setup,
    Gateway(GatewayArgs),
    Sessions(SessionsArgs),
    Logs(LogsArgs),
    Model,
    Config,
    Skills,
    Tools,
    Memory,
    Cron,
    Mcp,
    Status,
    Update,
    Dashboard(DashboardArgs),
    Auth,
    Pairing,
    Version,
    Plan,
}

fn main() -> Result<()> {
    let (argv, preparse_profile) = vela_runtime::preparse_profile_override(std::env::args())?;
    tracing_subscriber::fmt::init();
    let cli = Cli::parse_from(argv);
    let bootstrap = vela_runtime::initialize_bootstrap(
        preparse_profile.or_else(|| cli.profile.clone()),
        cli.ignore_user_config,
    )?;

    if cli.version {
        println!("vela-rs 0.1.0-parity");
        vela_runtime::bootstrap_banner();
        return Ok(());
    }

    match cli.command {
        Some(Commands::Chat(_)) | None => {
            Cli::command().print_help()?;
            println!();
        }
        Some(Commands::Status) => {
            println!("{}", bootstrap.summary_line());
            if bootstrap.loaded_env_paths.is_empty() {
                println!("loaded env: none");
            } else {
                for path in &bootstrap.loaded_env_paths {
                    println!("loaded env: {}", path.display());
                }
            }
            for source in &bootstrap.config_sources {
                println!("config source [{}]: {}", source.kind.label(), source.path.display());
            }
            println!(
                "resolved config: display.interface={:?} hooks_auto_accept={:?} security.redact_secrets={:?} network.force_ipv4={:?}",
                bootstrap.resolved_config.display_interface,
                bootstrap.resolved_config.hooks_auto_accept,
                bootstrap.resolved_config.security_redact_secrets,
                bootstrap.resolved_config.network_force_ipv4,
            );
            println!(
                "persistence: state_db={} existed_before={} bootstrap_runs={} sessions_dir={} snapshot_pattern={}",
                bootstrap.persistence.state_db_path.display(),
                bootstrap.persistence.state_db_existed_before,
                bootstrap.persistence.bootstrap_runs,
                bootstrap.persistence.sessions_dir.display(),
                bootstrap.persistence.snapshot_pattern,
            );
        }
        Some(Commands::Plan) => println!("docs/vela-rust-parity-plan.md"),
        Some(Commands::Gateway(args)) => println!("gateway placeholder: setup={} start={}", args.setup, args.start),
        Some(Commands::Sessions(args)) => println!("sessions placeholder: list={} browse={}", args.list, args.browse),
        Some(Commands::Logs(args)) => println!("logs placeholder: follow={} since={:?}", args.follow, args.since),
        Some(Commands::Dashboard(args)) => println!("dashboard placeholder: stop={} status={}", args.stop, args.status),
        Some(other) => println!("placeholder command: {:?}", other),
    }

    vela_runtime::bootstrap_banner();
    Ok(())
}
