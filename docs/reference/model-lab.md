# Model-lab criteria and boundaries

## Goal
Keep deeper model-core experimentation bounded, reversible, and evidence-driven before it can influence the live kernel route.

## Graduation gates
- document a bounded experiment surface before changing runtime routing
- capture repeatable eval evidence across at least two backends
- preserve restart-only ownership boundaries for runtime config and transport changes

## Deeper model-core entry criteria
Deeper model-core work is justified only when all of the following are already true:
- the live kernel path is stable enough that model-lab work is not being used to paper over scheduler, state, reload, or session regressions
- at least one real local-first backend path works end-to-end and remains covered by durable verification
- bounded eval slots can already compare candidate backends without mutating the default live route
- operator-visible evidence exists for pass/fail backend outcomes, not just anecdotal prompt screenshots
- the next experiment can be expressed as one reversible vertical slice with an explicit `Plan-Ref:` and a proof path

## Approved next experiment classes
The next layer of work may deepen only along these bounded classes:
- adapter and fine-tune intake criteria for already-supported backend contracts
- tighter eval policy, scoring, and experiment-slot evidence surfaces
- architecture experiment slots for ternary or sparse-routing comparisons that remain shadow-only
- backend capability comparisons that clarify when a new backend contract is worth promoting

## Adapter and fine-tune intake criteria
Adapter or fine-tune work can enter the model-lab only as criteria/evidence work until a later reviewed promotion slice changes routing. The durable policy exposed by `vela eval --show-policy` requires:
- candidate work must target an existing provider backend contract
- eval evidence must compare at least two allowed backends or explain the single-backend constraint
- provider capabilities and pass/fail outcomes must be visible before runtime influence
- live runtime routing, config policy, and persistence defaults remain unchanged until a separate reviewed promotion slice

## Explicitly deferred model-core directions
The following remain out of scope until the entry criteria are met again at a higher bar:
- custom ternary model training
- custom sparse or MoE training loops
- silent promotion of experimental routes into the default runtime path
- persistence or policy mutations driven automatically by model-lab experiments
- speculative optimization work that is not tied to an operator-visible experiment question

## Required evidence package
Any deeper model-core slice should land with all of the following:
- persisted eval runs with per-backend outcomes
- bounded failure-path coverage
- operator-visible docs or CLI inspection surface
- an explicit statement of what route, state, and policy surfaces are intentionally unchanged
- a rollback story that returns the system to the prior bounded slot/policy state

## Stop conditions
Pause model-core deepening and return to kernel/runtime hardening if any of these appear:
- embedded or provider routes begin failing baseline end-to-end checks
- experiment work requires live-route mutation to be observable
- runtime ownership, restart, or reload boundaries become ambiguous
- evidence capture becomes manual or non-repeatable

## Allowed strategies
- shadow-routing
- offline replay
- bounded backend comparison

## Prohibited behaviors
- silent live-route mutation without an explicit bounded slot
- remote model execution by default for local-backend slices
- unreviewed persistence or policy mutation from experimental paths

## Current durable surface
- `~/.vela/evals/policy.json`
- `vela eval --show-policy` surfaces graduation gates, required evidence, prohibited behaviors, and adapter/fine-tune intake criteria
- `~/.vela/evals/slots.json`
- `vela eval --list-slots`
- `vela eval --show-slot <id>`
- `vela eval --run-slot ternary-preview`
- `vela eval --run-slot sparse-routing-preview`
- `vela eval --run-slot local-first-replay`
- `vela eval --run-slot adapter-intake-gate`
- `vela eval --run-slot capability-parity-scan`
- published shadow-routing, offline-replay, and parity slot backend sets now include `embedded` when recording bounded eval evidence
- eval runs now persist `score_summary` evidence (`passed`, `failed`, `total`, and `pass_rate`) and surface it through `vela eval --run`, `--run-slot`, `--list`, `--show`, `--list-slots`, and `--show-slot`
- slot inspection surfaces now expose the latest passed backend set, failed backend set, score summary, capability-group evidence, unchanged live surfaces, and rollback notes for each published slot so provider differences stay operator-visible without changing the live route
