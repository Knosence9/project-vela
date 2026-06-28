# Kernel slice — Config, env, and reloadable runtime resolution

## Behavior target
The Rust runtime resolves config files, env vars, defaults, and precedence predictably enough to support a reloadable agentic OS core.

## Reference input
- source: existing Vela/Hermes config bootstrap behavior
- flow: run Vela under representative env/config combinations
- notes: compatibility is important, but this slice now also owns the future reload story

## Rust target
- crate/module: config/bootstrap layer
- state surface: config discovery, resolved runtime settings, and reload-safe runtime view

## Checklist
- [x] contract understood
- [x] initial resolution behavior implemented
- [x] failure behavior matched for the current scaffold
- [x] baseline verification notes captured
- [ ] define which config values are hot-reloadable vs restart-only
- [ ] implement first reload-safe config refresh path
- [ ] document the config boundary between kernel and extensions

## Implemented scaffold
- pre-parse profile override resolves `VELA_HOME` before clap parsing
- `VELA_HOME` directory bootstrap exists
- env loading order matches the captured target: `{VELA_HOME}/.env` first, project `.env` fallback second
- config discovery checks `{VELA_HOME}/config.yaml` first, then `./cli-config.yaml`
- `--ignore-user-config` and `VELA_IGNORE_USER_CONFIG=1` suppress the user config
- YAML parsing/merge resolves selected bootstrap keys
- resolved bootstrap settings are surfaced by `status` for verification

## Next proof target
Show that config resolution is not only compatible, but also usable as the foundation for plugin enable/disable and runtime reload semantics.
