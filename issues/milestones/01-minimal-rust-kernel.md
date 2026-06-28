# Milestone 1 — Minimal Rust kernel that is actually alive

## Goal
Make Vela real as a small end-to-end Rust kernel.

This milestone is not about porting every subsystem. It is about proving that Vela can already:
- start
- load config/state
- run a real session loop
- talk to one backend
- call at least one tool path
- persist enough state to resume continuity

## Scope
- `vela` binary entrypoint
- config loading and env resolution
- logging/bootstrap
- state-dir bootstrap
- one provider/backend path
- one session lifecycle path
- one tool execution path
- basic session/event persistence
- first reload mechanism

## Checklist
- [x] create Rust workspace
- [x] create `vela` binary
- [x] implement config loading scaffold
- [x] implement env resolution scaffold
- [x] implement logging/bootstrap scaffold
- [x] implement state-dir bootstrap scaffold
- [x] expose initial top-level command surface
- [ ] implement one real provider/backend path
- [ ] implement one real session lifecycle path
- [ ] implement one real tool invocation path
- [ ] persist enough session/event state for continuity
- [ ] implement first reload command or trigger
- [ ] verify one end-to-end kernel path works in Rust

## Exit gate
- Vela can run a real session in Rust
- Vela can persist and recover that session state
- Vela can reload config or plugin state in at least one believable path
- the project now has a livable kernel rather than only scaffolding
