# Parity slice — CLI command surface parity

## Behavior target
The Rust `vela` binary exposes the same top-level command surface as Vela.

## Current Vela source
- source: CLI command definitions and help output
- flow: invoke `vela --help` and key subcommand help screens

## Rust target
- crate/module: bootstrap CLI layer
- executable path: `vela`

## Checklist
- [x] contract understood
- [x] Rust command surface implemented
- [x] help output shape reviewed
- [x] mismatch list recorded
- [ ] parity proof added

## Current parity target
Match these real top-level Vela surfaces first:
- root flags: `--profile`, `--oneshot`, `--model`, `--provider`, `--toolsets`, `--resume`, `--skills`, `--continue`, `--worktree`, `--accept-hooks`, `--yolo`, `--pass-session-id`, `--version`, `--cli`, `--tui`
- root command groups: `chat`, `setup`, `gateway`, `sessions`, `logs`, `model`, `config`, `skills`, `tools`, `memory`, `cron`, `mcp`, `status`, `update`, `dashboard`, `auth`, `pairing`, `version`, `help`

## Implemented first scaffold
The Rust CLI now exposes a first parity-focused subset:
- top-level commands: `chat`, `setup`, `gateway`, `sessions`, `logs`, `model`, `config`, `skills`, `tools`, `memory`, `cron`, `mcp`, `status`, `update`, `dashboard`, `auth`, `pairing`, `version`, `plan`
- top-level flags: `--oneshot`, `--model`, `--provider`, `--toolsets`, `--resume`, `--skills`, `--continue`, `--worktree`, `--accept-hooks`, `--yolo`, `--pass-session-id`, `--ignore-user-config`, `--ignore-rules`, `--safe-mode`, `--profile`, `--cli`, `--tui`, `--version`
- verified via `cargo run -q -p vela -- --help` and `cargo run -q -p vela -- status`
