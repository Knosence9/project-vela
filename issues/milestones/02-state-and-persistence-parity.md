# Milestone 2 — State and persistence parity

## Goal
Preserve Hermes continuity across sessions and restarts.

## Scope
- sessions
- transcripts/history
- approvals
- pairing/auth state
- scheduler state
- migration/import path if formats differ

## Checklist
- [ ] define state schema
- [ ] implement SQLite persistence
- [ ] implement session persistence
- [ ] implement transcript/history persistence
- [ ] implement approval persistence
- [ ] implement pairing/auth persistence
- [ ] implement scheduler persistence
- [ ] implement migration/import path
- [ ] verify restart continuity parity

## Exit gate
- Rust can preserve or migrate Hermes state
- restart behavior is acceptably equivalent
