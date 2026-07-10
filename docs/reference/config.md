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
- YAML parsing and merge scaffolding now resolves this supported field set:
  - `display.interface`
  - `hooks_auto_accept`
  - `security.redact_secrets`
  - `network.force_ipv4`
  - `runtime.provider`
  - `runtime.model`
  - `runtime.ollama_base_url`
  - `runtime.llamacpp_base_url`
  - `runtime.embedded_model_path`
  - `extensions.manifests_dir`
  - `extensions.entries.<id>.enabled`
- resolved `hooks_auto_accept` and `security.redact_secrets` are also bridged into env vars
- `status` prints the resolved home path, loaded env files, config-source decisions, and resolved config fields for compatibility checking

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

## Current precedence and failure contract
- `VELA_HOME` wins if explicitly set before config bootstrap.
- Otherwise `--profile` / `-p` selects `{HOME}/.vela/profiles/<profile>` before command parsing continues.
- Without an explicit profile flag, bootstrap falls back to the sticky profile file at `~/.vela/active_profile` when present.
- Without either override, the default home is `~/.vela`.
- `.env` loading prefers `{VELA_HOME}/.env` and only falls back to `./.env` when the home-scoped file is absent.
- user config lives at `{VELA_HOME}/config.yaml`.
- project fallback config lives at `./cli-config.yaml`.
- `--ignore-user-config` and `VELA_IGNORE_USER_CONFIG=1` suppress the user config while still allowing project fallback config and `.env` loading.
- when both configs exist and the user config is active, project config remains visible only as lower precedence and is not merged.
- when the user config is unreadable or invalid, project config is promoted to `project-fallback` and becomes the resolved source of truth for supported keys.

## Regression-protected behavior in Rust tests
- invalid user YAML falls back to project config and marks the user source as `skipped-invalid`
- unreadable user config falls back to project config and marks the user source as `skipped-unreadable`
- `VELA_IGNORE_USER_CONFIG=1` forces project fallback even when `{VELA_HOME}/config.yaml` exists
- `{VELA_HOME}/.env` beats project `.env` when both exist
- sticky profile fallback updates `VELA_HOME` to the selected profile path before full bootstrap
- `hooks_auto_accept` and `security.redact_secrets` bridge back into `VELA_ACCEPT_HOOKS` and `VELA_REDACT_SECRETS`
- `gateway.json` alone does not participate in Rust bootstrap resolution
- `extensions.entries.<id>.enabled` defaults to `true` when omitted

## Current surfaced defaults and env bridges
- All currently surfaced resolved config fields default to unset / `None` until a config file provides a value.
- Runtime provider transport defaults come from the runtime contract layer rather than YAML defaults:
  - `runtime.provider=ollama` defaults `runtime.ollama_base_url` to `http://127.0.0.1:11434` when unset.
  - `runtime.provider=llamacpp` defaults `runtime.llamacpp_base_url` to `http://127.0.0.1:8080` when unset.
  - `runtime.provider=embedded` has no default URL and instead requires `runtime.embedded_model_path`.
- Config bootstrap currently bridges these resolved values back into env vars:
  - `hooks_auto_accept -> VELA_ACCEPT_HOOKS` (`true => 1`, `false => 0`)
  - `security.redact_secrets -> VELA_REDACT_SECRETS` (`true` / `false`)
  - effective ignore-user-config state -> `VELA_IGNORE_USER_CONFIG=1`
- `extensions.entries.<id>.enabled` defaults to `true` when the entry is present without an explicit `enabled:` value.

## Legacy gateway compatibility boundary
- The Python compatibility notes still matter as inventory: `gateway/config.py` documents legacy fallback from `~/.vela/gateway.json` into `config.yaml` processing and says failed `config.yaml` processing can still fall back to `.env` / `gateway.json` values.
- The current Rust bootstrap contract does **not** read `gateway.json`; live config resolution is bounded to `{VELA_HOME}/.env`, project `.env` fallback, `{VELA_HOME}/config.yaml`, and `./cli-config.yaml` under the precedence rules above.
- Treat `gateway.json` as historical compatibility context only until a future issue deliberately reintroduces it into the Rust surface.

## Explicit non-goals for the current Rust config surface
- The broader Python-era platform env matrix listed above (`DISCORD_*`, `TELEGRAM_*`, `MATRIX_*`, email, API server toggles, etc.) is not currently mapped into `vela-config` and should not be inferred as part of the Rust bootstrap contract.
- YAML keys outside the supported field set above are currently ignored by `vela-config` and need a dedicated future issue before they become part of the live Rust config surface.
