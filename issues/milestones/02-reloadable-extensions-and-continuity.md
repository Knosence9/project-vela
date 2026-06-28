# Milestone 2 — Reloadable extension system and core continuity

## Goal
Give Vela the first real signs of being an agentic OS instead of only a CLI shell.

This milestone focuses on two things together:
1. continuity through persistence
2. fast experimentation through config-driven reloadable extensions

## Scope
- sessions and event persistence
- transcript/history persistence
- minimal scheduler/pending-work state if needed by the first slice
- plugin manifest/registry
- extension lifecycle hooks
- config-driven enable/disable behavior
- reload path for selected runtime capabilities

## Checklist
- [x] define bootstrap state schema
- [x] implement bootstrap-level SQLite creation
- [x] implement session persistence
- [x] implement transcript/history persistence
- [x] implement minimal continuity/restart recovery path
- [ ] define plugin manifest format
- [ ] define extension lifecycle hooks
- [ ] load tool extensions from config
- [ ] load skill/workflow extensions from config
- [ ] implement enable/disable flags
- [ ] implement safe reload path
- [ ] verify continuity plus reload work together in one end-to-end slice
- [x] add pending approval and review-candidate continuity surfaces for memory/skills

## Exit gate
- Vela preserves useful continuity across restarts
- Vela can add/remove at least selected capabilities via config and reload
- experimentation speed materially improves over a hardcoded kernel-only build
