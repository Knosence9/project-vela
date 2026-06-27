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
- [ ] contract understood
- [ ] parity fixtures identified
- [ ] Rust resolution behavior implemented
- [ ] failure behavior matched
- [ ] parity proof added
