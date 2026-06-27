# Parity slice — Config and env resolution parity

## Behavior target
The Rust runtime resolves config files, env vars, defaults, and precedence the same way Vela does.

## Current Vela source
- source: config loading/bootstrap code
- flow: run Vela under representative env/config combinations

## Rust target
- crate/module: config/bootstrap layer
- state surface: config discovery and resolved runtime settings

## Checklist
- [x] contract understood
- [x] parity fixtures identified
- [x] Rust resolution behavior implemented
- [x] failure behavior matched
- [x] parity proof added

## Implemented scaffold
- pre-parse profile override resolves `VELA_HOME` before clap parsing
- `VELA_HOME` directory bootstrap exists
- env loading order matches the captured target: `{VELA_HOME}/.env` first, project `.env` fallback second
- config discovery checks `{VELA_HOME}/config.yaml` first, then `./cli-config.yaml`
- `--ignore-user-config` and `VELA_IGNORE_USER_CONFIG=1` suppress the user config
- YAML parsing/merge resolves `display.interface`, `hooks_auto_accept`, `security.redact_secrets`, and `network.force_ipv4`
- resolved `hooks_auto_accept` and `security.redact_secrets` are bridged back into env vars
- `status` emits the resolved home path, loaded env files, config-source decisions, and resolved config values for verification

## Verification notes
- verified with `VELA_HOME=/tmp/vela-home-test cargo run -q -p vela -- status`
- verified with `HOME=/tmp/vela-home-profile cargo run -q -p vela -- --profile demo status`
- verified with `VELA_HOME=/tmp/vela-home-precedence cargo run -q -p vela -- status`
- verified with `VELA_HOME=/tmp/vela-home-precedence cargo run -q -p vela -- --ignore-user-config status`
- verified with `VELA_HOME=/tmp/vela-home-precedence VELA_IGNORE_USER_CONFIG=1 cargo run -q -p vela -- status`
- verified with `VELA_HOME=/tmp/vela-home-merge cargo run -q -p vela -- status`
- verified with `VELA_HOME=/tmp/vela-home-merge cargo run -q -p vela -- --ignore-user-config status`
