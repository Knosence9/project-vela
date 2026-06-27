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
- exact config precedence order across `.env`, user config, project config, and legacy gateway config
- default values for key settings
- profile/account behavior
- invalid-config failure behavior
- exact mapping from config keys to env vars
