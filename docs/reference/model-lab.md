# Model-lab criteria and boundaries

## Goal
Keep deeper model-core experimentation bounded, reversible, and evidence-driven before it can influence the live kernel route.

## Graduation gates
- document a bounded experiment surface before changing runtime routing
- capture repeatable eval evidence across at least two backends
- preserve restart-only ownership boundaries for runtime config and transport changes

## Allowed strategies
- shadow-routing
- offline replay
- bounded backend comparison

## Prohibited behaviors
- silent live-route mutation without an explicit bounded slot
- remote model execution by default for local-backend slices
- unreviewed persistence or policy mutation from experimental paths

## Required evidence
- persisted eval runs with per-backend outcomes
- bounded failure-path coverage
- operator-visible docs or CLI inspection surface

## Current durable surface
- `~/.vela/evals/policy.json`
- `vela eval --show-policy`
- `~/.vela/evals/slots.json`
- `vela eval --list-slots`
- `vela eval --run-slot ternary-preview`
- `vela eval --run-slot local-first-replay`
- `vela eval --run-slot capability-parity-scan`
