# Parity slice — CLI command surface parity

## Behavior target
The Rust `hermes` binary exposes the same top-level command surface as Hermes.

## Current Hermes source
- source: CLI command definitions and help output
- flow: invoke `hermes --help` and key subcommand help screens

## Rust target
- crate/module: bootstrap CLI layer
- executable path: `hermes`

## Checklist
- [ ] contract understood
- [ ] Rust command surface implemented
- [ ] help output shape reviewed
- [ ] mismatch list recorded
- [ ] parity proof added
