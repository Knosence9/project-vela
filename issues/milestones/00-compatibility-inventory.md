# Milestone 0 — Compatibility inventory

## Goal
Enumerate the full Hermes behavior surface that the Rust rewrite must preserve.

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
- [ ] enumerate CLI commands
- [ ] enumerate config files, env vars, defaults
- [ ] enumerate on-disk state locations
- [ ] enumerate session lifecycle behaviors
- [ ] enumerate gateway/platform behaviors
- [ ] enumerate scheduler behaviors
- [ ] enumerate tool contracts
- [ ] enumerate provider contracts
- [ ] enumerate memory/skills behaviors
- [ ] freeze compatibility target docs

## Exit gate
- all public Hermes surfaces are enumerated
- parity target docs exist and are usable for implementation
