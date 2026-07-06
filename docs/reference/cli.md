# CLI compatibility notes

## Exact entrypoints confirmed
Python package scripts declared in `pyproject.toml`:
- `vela -> vela_cli.main:main`
- `vela-agent -> run_agent:main`
- `vela-acp -> acp_adapter.entry:main`

Installed `vela` command behavior is driven by `vela_cli.main` and `vela_cli._parser`.

## Confirmed user-facing flows
README-visible flows to preserve:
- `vela`
- `vela gateway setup`
- `vela gateway start`
- `vela gateway --webhook-url <url> --payload <text> [--event-type <name>]`
- `vela cron --add <task> --schedule <expr> [--delivery-webhook-url <url>] [--delivery-event-type <name>]`
- `vela agents --delegate <task> --role <role> [--note <text>]`
- `vela mcp --bridge <server> --tool <tool> --payload <json> [--note <text>]`
- `vela claw migrate`
- `vela claw migrate --dry-run`
- `vela claw migrate --preset user-data`
- `vela claw migrate --overwrite`

README-visible shared slash commands:
- `/new`
- `/reset`
- `/model [provider:model]`
- `/personality [name]`
- `/retry`
- `/undo`
- `/compress`
- `/usage`
- `/insights [--days N]`
- `/skills`
- `/<skill-name>`
- `/stop`
- `/platforms`
- `/status`
- `/sethome`

## Confirmed top-level parser behavior
- `--profile` / `-p` is consumed before argparse, sets `VELA_HOME`, and is stripped from `sys.argv`.
- Bare `vela` defaults to the `chat` path.
- `vela_cli._parser` owns the top-level parser plus the `chat` subparser.
- Most other subcommands are added inline in `vela_cli.main`.

## Confirmed built-in top-level subcommands
`vela_cli.main` declares these built-in command groups:
- `chat`
- `setup`
- `gateway`
- `sessions`
- `logs`
- `model`
- `config`
- `skills`
- `tools`
- `memory`
- `cron`
- `mcp`
- `status`
- `update`
- `dashboard`
- `auth`
- `pairing`
- `version`
- plus many others including `backup`, `claw`, `doctor`, `fallback`, `kanban`, `profile`, `security`, `send`, `webhook`, `whatsapp`, and `help`

## Confirmed top-level flags worth matching first
- `--profile`, `-p`
- `--oneshot`, `-z`
- `--model`, `-m`
- `--provider`
- `--toolsets`, `-t`
- `--resume`, `-r`
- `--skills`, `-s`
- `--continue`, `-c`
- `--worktree`, `-w`
- `--accept-hooks`
- `--yolo`
- `--pass-session-id`
- `--ignore-user-config`
- `--ignore-rules`
- `--safe-mode`
- `--version`
- `--cli`
- `--tui`

## Confirmed chat-subparser flags worth matching first
- `--query`, `-q`
- `--image`
- `--model`, `-m`
- `--toolsets`, `-t`
- `--skills`, `-s`
- `--provider`
- `--verbose`, `-v`
- `--resume`, `-r`
- `--continue`, `-c`
- `--worktree`, `-w`
- `--accept-hooks`
- `--checkpoints`
- `--max-turns`
- `--yolo`
- `--pass-session-id`

## Bootstrap behavior visible in source
- `cli.py` uses `fire.Fire(main)` in its direct `__main__` path.
- `cli.py` sets `VELA_QUIET=1` for a clean CLI startup path.
- `cli.py` loads `.env` from `~/.vela/.env` first, then falls back to project `.env` for development.
- `run_agent.py` also loads env through `vela_cli.env_loader.load_vela_dotenv(...)`.
- `run_agent.py` logs which `.env` files were loaded, or logs that none were found.
- `vela_cli.main` imports `vela_bootstrap` first for Windows UTF-8 safety.
- `vela_cli.main` can set `VELA_DEFER_AGENT_STARTUP=1` on fast chat paths.

## Known Rust gateway surface
- `vela gateway --setup` ensures durable gateway config plus inbox/outbox directories
- `vela gateway --start` starts or resumes the gateway-scoped runtime session
- `vela gateway --webhook-url <url> --payload <text> [--event-type <name>]` delivers one bounded outbound webhook payload through the kernel-owned gateway path and persists an outbox record

## Known Rust scheduler delivery surface
- `vela cron --add <task> --schedule <expr> [--delivery-webhook-url <url>] [--delivery-event-type <name>]` registers one durable scheduled job and can route completed/failed job outcomes through the gateway webhook delivery path
- `vela cron --list` and `vela cron --show <id>` surface the configured delivery target plus the latest delivery outcome/error state

## Known Rust delegation surface
- `vela agents --delegate <task> --role <role> [--note <text>]` records one bounded subagent delegation request through the kernel-owned runtime surface and persists it for later inspection
- `vela agents --list` shows durable delegation requests
- `vela agents --show <id>` shows one durable delegation request by id

## Known Rust MCP surface
- `vela mcp --bridge <server> --tool <tool> --payload <json> [--note <text>]` records one bounded durable MCP bridge request through the kernel-owned runtime surface
- `vela mcp --list` shows durable MCP bridge requests
- `vela mcp --show <id>` shows one durable MCP bridge request by id

## Backend API status surface
- `vela status` now prints the explicit backend API contract list plus the resolved backend contract from config, including bounded local backends such as Ollama, mock, and llama.cpp, so future adapters can target stable kernel-owned interfaces

## Backend eval surface
- `vela eval --run <prompt> --backend <id>... [--model <name>]` compares bounded backend behavior through one repeatable persisted evaluation run
- `vela eval --run-slot ternary-preview [--backend <id>...] [--model <name>]` executes the first bounded architecture experiment slot without changing the live kernel route
- `vela eval --list` shows durable backend eval runs
- `vela eval --show <id>` shows one durable backend eval run with per-backend results
- `vela eval --list-slots` shows published bounded architecture experiment slots
- `vela eval --show-slot <id>` shows one bounded architecture experiment slot by id
- `vela eval --show-policy` shows the durable model-lab criteria and boundaries that govern deeper model-core experimentation

## Still needed
- exact subcommands under groups like `sessions`, `auth`, `cron`, and `dashboard`
- help output shape and wording
- exit code semantics
- interactive vs non-interactive behavior
- exact relaunch behavior across `vela`, `cli.py`, and `vela_cli.main`
