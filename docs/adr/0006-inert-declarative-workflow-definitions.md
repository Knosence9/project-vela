# ADR-0006: Validate workflows as inert declarative state machines

- **Status:** accepted
- **Date:** 2026-07-28
- **Owners:** Project Vela maintainers
- **Related issue:** [#676](https://github.com/Knosence9/project-vela/issues/676)
- **First execution issue:** [#677](https://github.com/Knosence9/project-vela/issues/677)

## Context

Version-one extension manifests distinguish tools, skills, and workflows, but the implemented lifecycle previously stopped at workflow metadata. ADR-0003 deliberately limits executable activation to tools, while ADR-0004 and ADR-0005 give skill text a separate inert preparation and explicit prompt-authority lifecycle. A workflow is neither one executable tool call nor instruction text to inject into a model request.

The North Star defines a workflow as an orchestrated sequence or state machine with explicit phases, gates, transitions, and stop conditions that should be machine-checkable where possible. The architecture plan also calls for a separate workflow registry and deterministic validation. Vela therefore needs a structural workflow boundary before choosing execution, persistence, scheduling, or capability-binding semantics.

## Decision

The first workflow boundary prepares selected version-one `workflow` entrypoints as bounded UTF-8 YAML and validates them into immutable, non-executing state-machine definitions.

A definition is independently versioned and has this shape:

```yaml
workflow_version: 1
start: plan
phases:
  - id: plan
    transitions:
      - to: review
        gate: plan.approved
  - id: review
    transitions:
      - to: done
  - id: done
    terminal: true
```

Unknown fields are rejected at every level. `workflow_version` must be `1`. `start`, phase IDs, transition targets, and present gate IDs preserve exact authored text but must not be blank. A definition contains at least one phase and one explicit terminal phase, has unique exact phase IDs, and names an existing start phase. Every terminal phase has no transitions; every non-terminal phase has at least one transition. Every transition targets an existing phase, and every authored phase is reachable from the start. Phase and transition order remain authored order; batches remain exact extension-ID order.

A gate ID is inert caller-resolved metadata on a transition. Its presence does not select, load, authorize, or execute any capability, and absence means only that the static definition declares no named gate for that edge. Gate evaluation and transition choice are later runtime decisions.

Preparation first rejects the lexicographically first selected non-workflow before filesystem access. An empty selection is a filesystem-free success. Non-empty preparation reuses the descriptor-anchored root, package, manifest, and entrypoint identity boundary, reads at most 1 MiB plus one probe byte, requires UTF-8, parses strict YAML, and validates the complete graph. The operation is all-or-nothing and exposes exact workflow IDs in typed failures while preserving filesystem, encoding, and parser sources where applicable.

Prepared definitions are inert. Preparation does not register, activate, execute, schedule, pause, resume, persist, retry, resolve a gate, choose a transition, bind phases to skills/tools/agents/humans, compose a prompt, or grant permission.

## Alternatives considered

### Treat workflow entrypoints as skill instructions

Model instructions cannot enforce graph topology, stop conditions, or deterministic transition invariants. Reusing skill composition would also let workflow availability influence a provider without an explicit workflow execution contract.

### Execute workflow entrypoints as WebAssembly tools

One synchronous tool invocation does not model a caller-controlled multi-phase lifecycle. This would erase checkpoint and permission boundaries before they have been designed.

### Define an executor and persistence model immediately

Execution raises separate decisions about transition authority, gate resolution, phase actions, retries, concurrency, checkpoints, recovery, and cancellation. Static validation is useful independently and prevents those policies from entering accidentally.

### Permit unreachable phases or implicit terminal phases

Unreachable phases are almost certainly authoring errors, and implicit stop behavior weakens deterministic inspection. Requiring reachable topology and explicit terminal markers keeps stop conditions machine-checkable.

## Consequences

### Positive

- Workflow packages gain deterministic, machine-checkable structure without gaining execution authority.
- The existing descriptor-anchored filesystem boundary protects definition identity between discovery and preparation.
- Independently versioned definitions can evolve without changing extension manifest version one.
- Exact authored ordering remains available to future inspection and execution layers.
- Gate names can be modeled without prematurely deciding their registry or evaluator.

### Negative

- Prepared workflows still cannot run.
- Version one supports only static phases and transitions; it has no action, input/output, retry, timeout, compensation, or parallelism schema.
- Reachability validation allows cycles but does not prove eventual termination.
- A 1 MiB bound is intentionally conservative and may be tightened with operational evidence.

## Verification

The first execution slice follows RED→GREEN tests proving successful ordered preparation, exact authored topology, redacted debug output, wrong-kind rejection before filesystem access, empty-selection behavior, exact size bounds, invalid UTF-8/YAML/version/unknown-field failures, every graph invariant, descriptor-anchored revalidation, and all-or-nothing failure. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding a workflow registry, activation or replacement lifecycle, execution engine, gate registry/evaluator, phase action bindings, durable checkpoints, pause/resume, retries, compensation, concurrency, loops policy, input/output schemas, automatic prompt influence, remote packages, signatures, or migration support.
