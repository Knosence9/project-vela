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
- [ ] implement config loading parity
- [ ] implement env resolution parity
- [x] implement logging/bootstrap parity (minimal bootstrap logging scaffold)
- [ ] implement state-dir bootstrap parity
- [x] expose same top-level commands (first parity-focused subset scaffolded)
- [x] verify startup behavior matches Vela (workspace compiles; `--help` and `status` run)

## Exit gate
- basic startup behavior matches Vela
- config resolution is parity-checked
- same command names are exposed
