# Kernel slice — CLI command surface scaffold

## Behavior target
The Rust `vela` binary exposes a coherent top-level command surface that supports the first end-to-end kernel slice and leaves room for future agentic-OS growth.

## Reference input
- source: existing Vela/Hermes-style command surfaces
- flow: invoke `vela --help` and key subcommand help screens
- notes: command compatibility matters, but a livable kernel matters more than exhaustive day-one surface matching

## Rust target
- crate/module: bootstrap CLI layer
- executable path: `vela`

## Checklist
- [x] contract understood
- [x] Rust command surface scaffold implemented
- [x] help output shape reviewed
- [x] mismatch list recorded
- [ ] verify the command surface supports the first real kernel workflow
- [ ] add proof notes

## Current command strategy
Prioritize commands needed for the first alive kernel path:
- session/chat entrypoint
- status/diagnostics
- config visibility
- reload/restart path
- tool/runtime testing path
- future scheduler/gateway/memory expansion points

## Implemented scaffold
The Rust CLI currently exposes an initial broad scaffold and should now be judged by whether it supports the first believable Vela workflow rather than by exhaustive top-level surface matching alone.
