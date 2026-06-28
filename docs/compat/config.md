# Config compatibility notes

## Confirmed packaging and config surfaces
- Python package: `vela-agent`
- build backend: `setuptools.build_meta`
- Python requirement: `>=3.11,<3.14`
- root contains:
  - `.env.example`
  - `cli-config.yaml.example`
  - `pyproject.toml`
  - `package.json`

## Confirmed config-loading behavior from source
- `cli.py` and `run_agent.py` load `.env` from `~/.vela/.env` first.
- They use project-root `.env` as a development fallback.
- `run_agent.py` reports whether env files were loaded.
- `vela_state.py` comments indicate user config lives at `{VELA_HOME}/config.yaml`.
- `vela_state.py` references project fallback config at `cli-config.yaml`.
- `vela_state.py` supports `VELA_IGNORE_USER_CONFIG=1` to suppress user config.
- `vela_state.py` notes credentials in `.env` are still loaded even when user config is ignored.
- `gateway/config.py` explicitly documents legacy fallback from `~/.vela/gateway.json` into `config.yaml` processing.
- `gateway/config.py` says failed `config.yaml` processing falls back to `.env` / `gateway.json` values.

## Implemented in the Rust scaffold
- pre-argparse `--profile` / `-p` handling now runs before clap parsing
- profile override sets `VELA_HOME` before the main parser runs
- sticky profile fallback reads `~/.vela/active_profile` if present
- bootstrap creates the resolved `VELA_HOME` directory
- env loading prefers `{VELA_HOME}/.env`
- project `.env` is only used as fallback when `{VELA_HOME}/.env` is absent
- user config is discovered at `{VELA_HOME}/config.yaml`
- project fallback config is discovered at `./cli-config.yaml`
- `--ignore-user-config` and `VELA_IGNORE_USER_CONFIG=1` suppress the user config and allow project fallback to become active
- when both configs exist and user config is not ignored, project config is marked lower precedence
- YAML parsing and merge scaffolding now resolves these fields:
  - `display.interface`
  - `hooks_auto_accept`
  - `security.redact_secrets`
  - `network.force_ipv4`
- resolved `hooks_auto_accept` and `security.redact_secrets` are also bridged into env vars
- `status` prints the resolved home path, loaded env files, config-source decisions, and resolved config fields for parity checking

## Confirmed env vars and config knobs worth preserving first
- `VELA_IGNORE_USER_CONFIG`
- `VELA_DEFER_AGENT_STARTUP`
- `VELA_ACCEPT_HOOKS`
- `VELA_PREFILL_MESSAGES_FILE`
- `VELA_SESSION_SOURCE`
- `VELA_API_TIMEOUT`
- `VELA_API_CALL_STALE_TIMEOUT`
- `VELA_REDACT_SECRETS`
- `VELA_FILE_MUTATION_VERIFIER`
- `VELA_TURN_COMPLETION_EXPLAINER`
- `VELA_KANBAN_TASK`
- `VELA_LAZY_INSTALL_TARGET`
- `VELA_PYTHON_SRC_ROOT`
- `PYTHONUTF8`
- `PYTHONIOENCODING`

## Gateway/platform config signals observed
`gateway/config.py` exposes many platform env surfaces, including examples like:
- `DISCORD_BOT_TOKEN`
- `DISCORD_HOME_CHANNEL`
- `TELEGRAM_REQUIRE_MENTION`
- `SIGNAL_REQUIRE_MENTION`
- `MATRIX_HOMESERVER`
- `EMAIL_IMAP_HOST`
- `EMAIL_SMTP_HOST`
- `API_SERVER_ENABLED`
- `API_SERVER_PORT`

## Still needed
- exact config precedence order including legacy `gateway.json`
- default values for key settings
- profile/account behavior beyond path selection
- invalid-config failure behavior
- exact mapping from config keys to env vars beyond the currently bridged fields
- broader YAML coverage beyond the first resolved field set
