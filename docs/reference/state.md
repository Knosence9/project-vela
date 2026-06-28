# State compatibility notes

## Confirmed persistence surfaces
Behavior-relevant state modules observed:
- `vela_state.py`
- `gateway/session.py`
- `gateway/session_context.py`
- `gateway/restart.py`
- `tools/process_registry.py`
- `tools/session_search_tool.py`

## Exact state facts confirmed from source
- `vela_state.py` defines `DEFAULT_DB_PATH = get_vela_home() / "state.db"`.
- Vela uses SQLite-backed persistent session storage instead of only per-session JSONL files.
- The state store is documented as storing:
  - session metadata
  - full message history
  - model configuration
  - session source tags like `cli`, `telegram`, and `discord`
- `vela_state.py` uses FTS5 for full-text search.
- `run_agent.py` still mentions optional JSON session snapshots under `~/.vela/sessions/session_{sid}.json` for external tooling compatibility.
- `gateway/config.py` uses `get_vela_home() / "sessions"` as the default gateway sessions directory.
- `gateway/config.py` also references `~/.vela/gateway.json` as a legacy config/state input.

## Exact SQLite durability behavior confirmed
- Vela prefers `PRAGMA journal_mode=WAL` for `state.db`.
- If WAL is unsupported on filesystems like NFS/SMB/FUSE, Vela falls back to `journal_mode=DELETE` instead of failing hard.
- Vela tracks and surfaces the most recent `state.db` init failure so resume/history-style features can explain DB availability problems.
- `vela_state.py` contains automatic repair paths for malformed `state.db` FTS/schema cases.
- The code explicitly treats `state.db` and `kanban.db` as behavior-relevant durability surfaces.

## Implemented in the Rust scaffold
- persistence bootstrap now creates `{VELA_HOME}/sessions`
- persistence bootstrap now creates and opens `{VELA_HOME}/state.db`
- `state.db` currently initializes a minimal `state_meta` table
- bootstrap writes and increments a durable `bootstrap_runs` counter in `state.db`
- bootstrap records the snapshot compatibility pattern `sessions/session_<id>.json` in `state.db`
- `status` reports:
  - `state.db` path
  - whether the DB existed before startup
  - bootstrap run count
  - sessions directory path
  - snapshot pattern
- repeated startup against the same `VELA_HOME` now proves first-step restart continuity by incrementing `bootstrap_runs`

## README-visible migration behavior to preserve
- OpenClaw migration via `vela claw migrate`
- imports can include settings, memories, skills, API keys, allowlists, messaging settings, TTS assets, and AGENTS.md workspace instructions

## Still needed
- complete session persistence format and schema beyond `state_meta`
- transcript/history query behavior
- approval persistence behavior
- pairing/auth persistence behavior
- scheduler persistence behavior
- WAL/DELETE durability behavior
- restart continuity semantics across CLI and gateway beyond the bootstrap counter
