# vela-rs-plan

Planning workspace for a **Rust-first agentic OS** named Vela.

## Current direction
Vela is **not** a model-first project and **not** a big-bang rewrite of Hermes.

Vela should become:
1. a **Rust agentic OS core**
2. a **reloadable extension/plugin runtime**
3. a **model lab** for future backend experiments (ternary, sparse, MoE, custom routing)

## Recommendation
Use a **progressive replacement strategy**:
1. build a small but real Rust Vela kernel
2. make it livable early
3. add Hermes-class capabilities incrementally
4. keep fast experimentation through config + reload + plugins
5. defer deep model architecture work until the OS/runtime exists

## Priority
1. make Vela alive as a minimal Rust kernel
2. preserve or reintroduce Hermes-class capabilities in bounded slices
3. keep the system reloadable and plugin-friendly
4. only then expand into advanced model experiments

## Core principles
- **Rust for the kernel**
- **reloadable extensions for fast experimentation**
- **progressive replacement, not big-bang rewrite**
- **vertical slices over giant infrastructure phases**
- **agentic OS first, model lab second**

## Contents
- `docs/vela-rust-agentic-os-plan.md` — updated Vela Rust architecture and phased roadmap
- `docs/vela-crate-layout.md` — concrete crate/file ownership layout for the Rust kernel and memory system
- `docs/vela-schema.md` — durable schema for sessions, memory, skills, approvals, and future Honcho integration
- `docs/reference/` — compatibility notes and prior rewrite research that remain useful as reference input

## Current scaffold highlights
- `vela-config` now owns profile/home/env/config bootstrap logic
- `vela-memory` now bootstraps Hermes-style built-in memory files (`MEMORY.md`, `USER.md`) and can render a frozen prompt snapshot
- `vela-state` now owns SQLite session persistence plus transcript/event foundation tables (`sessions`, `messages`, `session_events`)
- `vela-state` now also maintains an FTS5-backed `message_fts` index and the CLI can run basic session history searches via `vela sessions --search <query>`
- `vela-memory` now supports first real built-in memory actions through the CLI: view/add/replace/remove plus prompt-snapshot rendering via `vela memory ...`
- `vela-skills` now bootstraps the procedural-memory directory and exposes basic list/view behavior through `vela skills` and `vela skills --view <name>`
- `vela-skills` now also supports first real management actions through the CLI: `--create`, `--write`, and `--delete`
- both `vela memory` and `vela skills` now support approval staging flows via `--stage`, `--pending`, `--show`, `--approve`, and `--reject`
- `vela review` now stages background-review candidates under `~/.vela/reviews/`, can promote them into the existing pending approval queues, can emit structured `memory_signal` / `skill_signal` events via `--emit-signals`, and can run an end-to-end background pass via `--auto`
- `.github/ISSUE_TEMPLATE/` — issue templates for milestones, slices, regressions, and proof work

## Working style
- start with a thin but end-to-end kernel
- ship one believable capability slice at a time
- keep config and runtime reload in scope from the beginning
- use compatibility work as input, not as the only organizing principle
- treat Hermes as the capability benchmark, not as a requirement for day-one feature completeness

## Suggested next steps
1. define the Rust kernel boundary
2. define plugin/runtime reload semantics
3. implement one end-to-end session loop with persistence and tools
4. add one Hermes-class vertical slice at a time
5. create a backend abstraction so future model experiments can plug in cleanly
