# Milestone 2 — State and persistence parity

## Goal
Preserve Vela continuity across sessions and restarts.

## Scope
- sessions
- transcripts/history
- approvals
- pairing/auth state
- scheduler state
- migration/import path if formats differ

## Checklist
- [x] define state schema (initial `state_meta` bootstrap schema)
- [x] implement SQLite persistence (bootstrap-level `state.db` creation)
- [ ] implement session persistence
- [ ] implement transcript/history persistence
- [ ] implement approval persistence
- [ ] implement pairing/auth persistence
- [ ] implement scheduler persistence
- [ ] implement migration/import path
- [x] verify restart continuity parity (bootstrap run counter survives repeated starts)

## Exit gate
- Rust can preserve or migrate Vela state
- restart behavior is acceptably equivalent
