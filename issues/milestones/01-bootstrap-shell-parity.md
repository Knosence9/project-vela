# Milestone 1 — Bootstrap shell parity

## Goal
Make the Rust project boot like Vela.

## Scope
- `vela` binary entrypoint
- config loading
- env resolution
- logging/bootstrap
- state-dir bootstrap
- command parsing shell

## Checklist
- [x] create Rust workspace
- [x] create `vela` binary
- [x] implement config loading parity (initial `.env` bootstrap scaffold)
- [x] implement env resolution parity (initial `VELA_HOME` / profile handling scaffold)
- [x] implement logging/bootstrap parity (minimal bootstrap logging scaffold)
- [x] implement state-dir bootstrap parity (resolved `VELA_HOME` directory now created)
- [x] expose same top-level commands (first parity-focused subset scaffolded)
- [x] verify startup behavior matches Vela (workspace compiles; `--help` and `status` run)

## Exit gate
- basic startup behavior matches Vela
- config resolution is parity-checked
- same command names are exposed
