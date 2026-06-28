# Vela compatibility inventory — initial notes

## Scope of this pass
This pass captures the first behavior-first inventory facts for `NousResearch/vela-agent`.

## High-confidence findings
- Vela is a **polyglot system** with a Python-heavy core and additional Node workspace surfaces.
- Python packaging is defined in `pyproject.toml`.
  - package name: `vela-agent`
  - version: `0.17.0`
  - build backend: `setuptools.build_meta`
  - Python requirement: `>=3.11,<3.14`
- Node workspace packaging is defined in `package.json`.
  - package name: `vela-agent`
  - `private: true`
  - workspaces: `apps/*`, `ui-tui`, `ui-tui/packages/*`, `web`
- The rewrite should continue to support **two main entry surfaces** documented in the README:
  - CLI via `vela`
  - messaging gateway via `vela gateway setup` and `vela gateway start`

## Top-level surfaces observed
Important root entries seen in the upstream repo:
- `agent/`
- `gateway/`
- `cron/`
- `tools/`
- `skills/`
- `providers/`
- `apps/`
- `web/`
- `ui-tui/`
- `tests/`
- `cli.py`
- `run_agent.py`
- `batch_runner.py`
- `mcp_serve.py`
- `mini_swe_runner.py`
- `vela_bootstrap.py`
- `vela_state.py`
- `toolsets.py`
- `toolset_distributions.py`

## Behavior surfaces that matter first
### CLI / startup
- `vela`
- `cli.py`
- `run_agent.py`
- `vela_bootstrap.py`
- `vela_cli/`

### Gateway / messaging
- `gateway/`
- `mcp_serve.py`
- `tui_gateway/`

### Scheduling
- `cron/`
- `batch_runner.py`

### Tools
- `tools/`
- `toolsets.py`
- `toolset_distributions.py`

### State / continuity
- `vela_state.py`
- `gateway/session.py`
- `gateway/restart.py`
- `gateway/session_context.py`

## README-visible user flows to preserve first
- install Vela
- start chatting with `vela`
- configure and start the messaging gateway
- shared slash-command behavior across CLI and messaging surfaces
- migration from OpenClaw via `vela claw migrate`

## README-visible command examples to preserve
- `vela`
- `vela gateway setup`
- `vela gateway start`
- `vela claw migrate`
- `vela claw migrate --dry-run`
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

## Core directories sampled so far
### `gateway/`
Observed files include:
- `config.py`
- `delivery.py`
- `pairing.py`
- `run.py`
- `session.py`
- `session_context.py`
- `status.py`
- `stream_consumer.py`
- `stream_dispatch.py`
- `stream_events.py`
- `slash_commands.py`
- `restart.py`
- `platform_registry.py`
- `platforms/`

### `tools/`
Observed files include:
- `approval.py`
- `code_execution_tool.py`
- `file_operations.py`
- `file_tools.py`
- `managed_tool_gateway.py`
- `mcp_tool.py`
- `memory_tool.py`
- `process_registry.py`
- `project_tools.py`
- `registry.py`
- `session_search_tool.py`
- `browser_tool.py`
- `computer_use_tool.py`
- `cronjob_tools.py`

### `agent/`
Observed behavior-heavy modules include:
- `agent_init.py`
- `agent_runtime_helpers.py`
- `conversation_loop.py`
- `context_engine.py`
- `memory_manager.py`
- `memory_provider.py`
- `conversation_compression.py`
- `context_compressor.py`
- `credential_persistence.py`
- `iteration_budget.py`

## Packaging implications for the Rust migration
- A Rust rewrite must treat Vela as more than a Python CLI; it includes Node workspace surfaces.
- The first kernel target should preserve the Python-visible entry behavior while leaving web/TUI/desktop surfaces as clients until later.

## Newly confirmed exact compatibility facts
- `pyproject.toml` declares these Python entrypoints:
  - `vela -> vela_cli.main:main`
  - `vela-agent -> run_agent:main`
  - `vela-acp -> acp_adapter.entry:main`
- `cli.py` direct execution path uses `fire.Fire(main)`.
- `cli.py` and `run_agent.py` load `.env` from `~/.vela/.env` first and project `.env` second.
- `vela_state.py` defines `DEFAULT_DB_PATH` as `get_vela_home() / "state.db"`.
- `gateway/config.py` defaults gateway session storage to `get_vela_home() / "sessions"`.
- `gateway/config.py` uses `~/.vela/gateway.json` as a legacy fallback layer.
- `vela_state.py` prefers WAL mode for `state.db` and falls back to DELETE mode when WAL is unsupported.
- `run_agent.py` still preserves optional JSON snapshots like `~/.vela/sessions/session_{sid}.json` for external tooling compatibility.

## Next inventory passes
1. extract exact `vela_cli.main` command tree and top-level flags
2. capture config precedence more precisely across user config, project config, `.env`, and `gateway.json`
3. inspect full `~/.vela` layout and additional persistence files
4. inspect gateway startup, pairing, and resume flows
5. inspect scheduler entrypoints and job definitions
6. inspect memory and skills persistence behavior
