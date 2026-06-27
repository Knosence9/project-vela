# Milestone 0 — Compatibility inventory

## Goal
Enumerate the full Vela behavior surface that the Rust rewrite must preserve.

## Scope
- CLI commands and flags
- config files, env vars, defaults, precedence
- state directories and persistence behavior
- session lifecycle behavior
- gateway/platform semantics
- scheduler behavior
- tool contracts
- provider contracts
- memory/skills behavior

## Checklist
- [x] enumerate CLI commands (README-visible set plus exact pyproject entrypoints captured)
- [x] enumerate config files, env vars, defaults (initial precedence/config surfaces captured)
- [x] enumerate on-disk state locations (initial `state.db`, `sessions/`, and `gateway.json` surfaces captured)
- [ ] enumerate session lifecycle behaviors
- [x] enumerate gateway/platform behaviors (initial gateway/home-channel surface capture)
- [ ] enumerate scheduler behaviors
- [x] enumerate tool contracts (initial tool surface capture)
- [x] enumerate provider contracts (initial provider/package surfaces captured)
- [ ] enumerate memory/skills behaviors
- [x] freeze compatibility target docs (CLI/config/state docs expanded)

## Exit gate
- all public Vela surfaces are enumerated
- parity target docs exist and are usable for implementation
