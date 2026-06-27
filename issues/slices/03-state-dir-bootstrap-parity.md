# Parity slice — State directory bootstrap parity

## Behavior target
The Rust runtime creates and uses state directories with the same semantics Hermes expects.

## Current Hermes source
- source: startup/bootstrap and persistence setup
- flow: first run, repeated run, missing directory, partial state cases

## Rust target
- crate/module: state bootstrap layer
- state surface: directory creation and initialization behavior

## Checklist
- [ ] contract understood
- [ ] first-run behavior captured
- [ ] repeat-run behavior captured
- [ ] Rust bootstrap behavior implemented
- [ ] parity proof added
