# Milestone 1 — Bootstrap shell parity

## Goal
Make the Rust project boot like Hermes.

## Scope
- `hermes` binary entrypoint
- config loading
- env resolution
- logging/bootstrap
- state-dir bootstrap
- command parsing shell

## Checklist
- [ ] create Rust workspace
- [ ] create `hermes` binary
- [ ] implement config loading parity
- [ ] implement env resolution parity
- [ ] implement logging/bootstrap parity
- [ ] implement state-dir bootstrap parity
- [ ] expose same top-level commands
- [ ] verify startup behavior matches Hermes

## Exit gate
- basic startup behavior matches Hermes
- config resolution is parity-checked
- same command names are exposed
