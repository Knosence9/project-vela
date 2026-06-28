# Milestone 0 — Architecture framing and compatibility inventory

## Goal
Capture the Hermes/Vela capability surface that matters **without** trapping the project in a strict surface-matching-only rewrite.

This milestone exists to define:
- what the Rust kernel must own first
- what should remain reloadable/configurable
- what Hermes-class capabilities matter most
- what Vela v0 must do to count as alive

## Scope
- must-have capability inventory
- kernel boundary candidates
- extension/runtime boundary candidates
- config + reload expectations
- persistence/state expectations
- future backend/model-lab requirements
- compatibility notes that remain useful as reference input

## Checklist
- [ ] group capabilities by phase, not only by old compatibility buckets
- [ ] define the Rust kernel boundary
- [ ] define the reloadable extension boundary
- [ ] identify the minimum believable Vela v0 slice
- [ ] capture config/reload expectations
- [ ] capture persistence/state expectations
- [ ] identify Hermes-class capabilities to add later in vertical slices
- [ ] identify non-goals for early phases
- [ ] freeze the architecture framing docs for Phase 1

## Exit gate
- the first kernel slice is small enough to build
- the project is no longer framed as “rewrite everything first”
- compatibility research is usable as input without dictating a full match-everything-before-life strategy
