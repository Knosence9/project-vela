use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "vela")]
#[command(about = "Rust-first agentic OS shell for Vela kernel work")]
pub(crate) struct Cli {
    #[arg(short = 'z', long = "oneshot")]
    pub(crate) oneshot: Option<String>,
    #[arg(short = 'm', long = "model")]
    pub(crate) model: Option<String>,
    #[arg(long = "provider")]
    pub(crate) provider: Option<String>,
    #[arg(short = 't', long = "toolsets")]
    pub(crate) toolsets: Option<String>,
    #[arg(short = 'r', long = "resume")]
    pub(crate) resume: Option<String>,
    #[arg(short = 's', long = "skills")]
    pub(crate) skills: Vec<String>,
    #[arg(short = 'c', long = "continue")]
    pub(crate) continue_last: Option<String>,
    #[arg(short = 'w', long = "worktree", default_value_t = false)]
    pub(crate) worktree: bool,
    #[arg(long = "accept-hooks", default_value_t = false)]
    pub(crate) accept_hooks: bool,
    #[arg(long = "yolo", default_value_t = false)]
    pub(crate) yolo: bool,
    #[arg(long = "pass-session-id", default_value_t = false)]
    pub(crate) pass_session_id: bool,
    #[arg(long = "ignore-user-config", default_value_t = false)]
    pub(crate) ignore_user_config: bool,
    #[arg(long = "ignore-rules", default_value_t = false)]
    pub(crate) ignore_rules: bool,
    #[arg(long = "safe-mode", default_value_t = false)]
    pub(crate) safe_mode: bool,
    #[arg(short = 'p', long = "profile")]
    pub(crate) profile: Option<String>,
    #[arg(long = "cli", default_value_t = false, group = "run_mode")]
    pub(crate) cli_mode: bool,
    #[arg(long = "tui", default_value_t = false, group = "run_mode")]
    pub(crate) tui_mode: bool,
    #[arg(long = "version", default_value_t = false)]
    pub(crate) version: bool,
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct ChatArgs {
    #[arg(short = 'q', long = "query")]
    pub(crate) query: Option<String>,
    #[arg(long = "image")]
    pub(crate) image: Option<String>,
    #[arg(short = 'm', long = "model")]
    pub(crate) model: Option<String>,
    #[arg(short = 't', long = "toolsets")]
    pub(crate) toolsets: Option<String>,
    #[arg(short = 's', long = "skills")]
    pub(crate) skills: Vec<String>,
    #[arg(long = "provider")]
    pub(crate) provider: Option<String>,
    #[arg(short = 'v', long = "verbose", default_value_t = false)]
    pub(crate) verbose: bool,
    #[arg(short = 'r', long = "resume")]
    pub(crate) resume: Option<String>,
    #[arg(short = 'c', long = "continue")]
    pub(crate) continue_last: Option<String>,
    #[arg(short = 'w', long = "worktree", default_value_t = false)]
    pub(crate) worktree: bool,
    #[arg(long = "accept-hooks", default_value_t = false)]
    pub(crate) accept_hooks: bool,
    #[arg(long = "checkpoints", default_value_t = false)]
    pub(crate) checkpoints: bool,
    #[arg(long = "max-turns")]
    pub(crate) max_turns: Option<u32>,
    #[arg(long = "yolo", default_value_t = false)]
    pub(crate) yolo: bool,
    #[arg(long = "pass-session-id", default_value_t = false)]
    pub(crate) pass_session_id: bool,
}

#[derive(Debug, Default, Args)]
pub(crate) struct GatewayArgs {
    #[arg(long = "setup", default_value_t = false, group = "action")]
    pub(crate) setup: bool,
    #[arg(long = "start", default_value_t = false, group = "action")]
    pub(crate) start: bool,
    #[arg(long = "webhook-url", group = "action")]
    pub(crate) webhook_url: Option<String>,
    #[arg(long = "payload", requires = "webhook_url")]
    pub(crate) payload: Option<String>,
    #[arg(long = "event-type", requires = "webhook_url")]
    pub(crate) event_type: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct SessionsArgs {
    #[arg(long = "list", default_value_t = false)]
    pub(crate) list: bool,
    #[arg(long = "browse", default_value_t = false)]
    pub(crate) browse: bool,
    #[arg(long = "search")]
    pub(crate) search: Option<String>,
    #[arg(long = "show")]
    pub(crate) show: Option<String>,
    #[arg(long = "branch")]
    pub(crate) branch: Option<String>,
    #[arg(long = "title")]
    pub(crate) title: Option<String>,
    #[arg(long = "note")]
    pub(crate) note: Option<String>,
    #[arg(long = "compress")]
    pub(crate) compress: Option<String>,
    #[arg(long = "summary")]
    pub(crate) summary: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct CronArgs {
    #[arg(long = "setup", default_value_t = false, group = "action")]
    pub(crate) setup: bool,
    #[arg(long = "start", default_value_t = false, group = "action")]
    pub(crate) start: bool,
    #[arg(long = "list", default_value_t = false, group = "action")]
    pub(crate) list: bool,
    #[arg(long = "add", group = "action")]
    pub(crate) add: Option<String>,
    #[arg(long = "show", group = "action")]
    pub(crate) show: Option<String>,
    #[arg(long = "schedule", requires = "add")]
    pub(crate) schedule: Option<String>,
    #[arg(long = "source", requires = "add")]
    pub(crate) source: Option<String>,
    #[arg(long = "delivery-webhook-url", requires = "add")]
    pub(crate) delivery_webhook_url: Option<String>,
    #[arg(long = "delivery-event-type", requires = "delivery_webhook_url")]
    pub(crate) delivery_event_type: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct AgentsArgs {
    #[arg(long = "delegate", group = "action")]
    pub(crate) delegate: Option<String>,
    #[arg(long = "role", requires = "delegate")]
    pub(crate) role: Option<String>,
    #[arg(long = "note", requires = "delegate")]
    pub(crate) note: Option<String>,
    #[arg(long = "list", default_value_t = false, group = "action")]
    pub(crate) list: bool,
    #[arg(long = "show", group = "action")]
    pub(crate) show: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct McpArgs {
    #[arg(long = "bridge", group = "action")]
    pub(crate) bridge: Option<String>,
    #[arg(long = "tool", requires = "bridge")]
    pub(crate) tool: Option<String>,
    #[arg(long = "payload", requires = "bridge")]
    pub(crate) payload: Option<String>,
    #[arg(long = "note", requires = "bridge")]
    pub(crate) note: Option<String>,
    #[arg(long = "list", default_value_t = false, group = "action")]
    pub(crate) list: bool,
    #[arg(long = "show", group = "action")]
    pub(crate) show: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct LogsArgs {
    #[arg(short = 'f', long = "follow", default_value_t = false)]
    pub(crate) follow: bool,
    #[arg(long = "since")]
    pub(crate) since: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct DashboardArgs {
    #[arg(long = "stop", default_value_t = false)]
    pub(crate) stop: bool,
    #[arg(long = "status", default_value_t = false)]
    pub(crate) status: bool,
}

#[derive(Debug, Default, Args)]
pub(crate) struct EvalArgs {
    #[arg(long = "run", group = "action")]
    pub(crate) run: Option<String>,
    #[arg(long = "run-slot", group = "action")]
    pub(crate) run_slot: Option<String>,
    #[arg(long = "backend")]
    pub(crate) backends: Vec<String>,
    #[arg(long = "model")]
    pub(crate) model: Option<String>,
    #[arg(long = "list", default_value_t = false, group = "action")]
    pub(crate) list: bool,
    #[arg(long = "show", group = "action")]
    pub(crate) show: Option<String>,
    #[arg(long = "list-slots", default_value_t = false, group = "action")]
    pub(crate) list_slots: bool,
    #[arg(long = "show-slot", group = "action")]
    pub(crate) show_slot: Option<String>,
    #[arg(long = "show-policy", default_value_t = false, group = "action")]
    pub(crate) show_policy: bool,
}

#[derive(Debug, Default, Args)]
pub(crate) struct ExtensionsArgs {
    #[arg(long = "reload", default_value_t = false, group = "action")]
    pub(crate) reload: bool,
    #[arg(long = "list", default_value_t = false, group = "action")]
    pub(crate) list: bool,
}

#[derive(Debug, Default, Args)]
pub(crate) struct SkillsArgs {
    #[arg(long = "list", default_value_t = false, group = "action")]
    pub(crate) list: bool,
    #[arg(long = "view", group = "action")]
    pub(crate) view: Option<String>,
    #[arg(long = "create", group = "action")]
    pub(crate) create: Option<String>,
    #[arg(long = "write", group = "action")]
    pub(crate) write: Option<String>,
    #[arg(long = "delete", group = "action")]
    pub(crate) delete: Option<String>,
    #[arg(long = "description")]
    pub(crate) description: Option<String>,
    #[arg(long = "body")]
    pub(crate) body: Option<String>,
    #[arg(long = "stage", default_value_t = false)]
    pub(crate) stage: bool,
    #[arg(long = "pending", default_value_t = false, group = "action")]
    pub(crate) pending: bool,
    #[arg(long = "approve", group = "action")]
    pub(crate) approve: Option<String>,
    #[arg(long = "reject", group = "action")]
    pub(crate) reject: Option<String>,
    #[arg(long = "show", group = "action")]
    pub(crate) show: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct ReviewArgs {
    #[arg(long = "list", default_value_t = false, group = "action")]
    pub(crate) list: bool,
    #[arg(long = "emit-signals", default_value_t = false, group = "action")]
    pub(crate) emit_signals: bool,
    #[arg(long = "suggest", default_value_t = false, group = "action")]
    pub(crate) suggest: bool,
    #[arg(long = "auto", default_value_t = false, group = "action")]
    pub(crate) auto: bool,
    #[arg(long = "limit", default_value_t = 20)]
    pub(crate) limit: usize,
    #[arg(long = "show", group = "action")]
    pub(crate) show: Option<String>,
    #[arg(long = "promote", group = "action")]
    pub(crate) promote: Option<String>,
    #[arg(long = "reject", group = "action")]
    pub(crate) reject: Option<String>,
    #[arg(long = "target", default_value = "memory")]
    pub(crate) target: String,
    #[arg(long = "memory-add", group = "action")]
    pub(crate) memory_add: Option<String>,
    #[arg(long = "memory-replace", group = "action")]
    pub(crate) memory_replace: Option<String>,
    #[arg(long = "memory-remove", group = "action")]
    pub(crate) memory_remove: Option<String>,
    #[arg(long = "match")]
    pub(crate) match_text: Option<String>,
    #[arg(long = "skill-create", group = "action")]
    pub(crate) skill_create: Option<String>,
    #[arg(long = "skill-write", group = "action")]
    pub(crate) skill_write: Option<String>,
    #[arg(long = "skill-delete", group = "action")]
    pub(crate) skill_delete: Option<String>,
    #[arg(long = "description")]
    pub(crate) description: Option<String>,
    #[arg(long = "body")]
    pub(crate) body: Option<String>,
    #[arg(long = "reason")]
    pub(crate) reason: Option<String>,
    #[arg(long = "source")]
    pub(crate) source: Option<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct MemoryArgs {
    #[arg(long = "target", default_value = "memory")]
    pub(crate) target: String,
    #[arg(long = "view", default_value_t = false, group = "action")]
    pub(crate) view: bool,
    #[arg(long = "prompt-snapshot", default_value_t = false, group = "action")]
    pub(crate) prompt_snapshot: bool,
    #[arg(long = "add", group = "action")]
    pub(crate) add: Option<String>,
    #[arg(long = "replace", group = "action")]
    pub(crate) replace: Option<String>,
    #[arg(long = "match")]
    pub(crate) match_text: Option<String>,
    #[arg(long = "remove", group = "action")]
    pub(crate) remove: Option<String>,
    #[arg(long = "stage", default_value_t = false)]
    pub(crate) stage: bool,
    #[arg(long = "pending", default_value_t = false, group = "action")]
    pub(crate) pending: bool,
    #[arg(long = "approve", group = "action")]
    pub(crate) approve: Option<String>,
    #[arg(long = "reject", group = "action")]
    pub(crate) reject: Option<String>,
    #[arg(long = "show", group = "action")]
    pub(crate) show: Option<String>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    Chat(ChatArgs),
    Setup,
    Gateway(GatewayArgs),
    Sessions(SessionsArgs),
    Logs(LogsArgs),
    Model,
    Config,
    Extensions(ExtensionsArgs),
    Skills(SkillsArgs),
    Tools,
    Memory(MemoryArgs),
    Review(ReviewArgs),
    Cron(CronArgs),
    Agents(AgentsArgs),
    Mcp(McpArgs),
    Eval(EvalArgs),
    Status,
    Update,
    Dashboard(DashboardArgs),
    Auth,
    Pairing,
    Version,
    Plan,
}

pub(crate) fn print_extension_record(entry: &vela_runtime::ExtensionRecord) {
    let capabilities = if entry.capabilities.is_empty() {
        "none".to_string()
    } else {
        entry.capabilities.join(",")
    };
    println!(
        "extension [{}]: id={:?} title={:?} kind={:?} activation={:?} version={:?} entry={:?} capabilities={} path={} detail={:?}",
        entry.lifecycle.label(),
        entry.id,
        entry.title,
        entry.kind.as_ref().map(|kind| kind.label()),
        entry.activation.as_ref().map(|activation| activation.label()),
        entry.version,
        entry.entry,
        capabilities,
        entry.manifest_path.display(),
        entry.detail,
    );
}
