# ADR-0020: Require explicit workflow-phase provider composition

- **Status:** accepted
- **Date:** 2026-07-29
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0005, ADR-0019, issues #742 and #743

## Context

ADR-0019 lets a caller resolve one chosen workflow phase's inert skill bindings through a caller-owned `SkillRegistry`. ADR-0005 separately defines the provider-neutral authority structure and durable ordering of an explicitly composed tool-free assistant turn. Callers need a narrow bridge between those contracts, but workflow topology movement must not silently become model authority or execution.

## Decision

`AssistantRuntime::execute_workflow_phase_turn` is the explicit tool-free bridge. The caller supplies the exact borrowed `RegisteredWorkflowPhase`, process-local skill registry, durable session, human content, system policy, and developer policy. The runtime resolves the phase through ADR-0019 before any transcript or provider side effect, then reuses ADR-0005's composed-turn implementation.

The provider receives distinct authority fields in descending order: caller system policy, caller developer policy, the phase's resolved registered skill blocks in deterministic ascending exact-ID order, and the durable transcript after the human turn. Registered but unbound skills are excluded. The phase retains its authored binding order as inert topology; resolution order does not create precedence semantics.

Phase-resolution failures are exposed as `RuntimeError::WorkflowPhaseSkills` with the typed `WorkflowPhaseSkillResolutionError` as their source. A missing, duplicate, malformed, or invalid terminal binding fails before transcript persistence and before provider invocation. After successful resolution, session and provider failures retain the existing composed-turn durability semantics, including preservation of a committed human turn when the provider fails.

The operation does not discover or infer a current phase, load or validate a workflow run, start, advance, pause, resume, cancel, fail, complete, or schedule workflow lifecycle state, persist skill-selection evidence, choose a transition, evaluate a gate, grant tool permission, or invoke a tool. A caller that wants the current cursor or durable-run phase must choose and pass that borrowed phase explicitly.

## Alternatives considered

### Accept a workflow run ID and infer the current phase

That would combine event replay, lifecycle eligibility, skill authority, and provider work in one boundary. Explicit borrowed phase input keeps lifecycle reads and model influence separately caller-owned.

### Automatically execute after workflow start or advancement

Topology movement is not permission to invoke a provider. Automatic execution would make replay or movement trigger effects and obscure where model authority entered the system.

### Reimplement skill selection inside the runtime

ADR-0019 already owns phase validation and exact registry selection. Reusing it preserves deterministic failure behavior and avoids a second resolution policy.

### Add tool-capable and durable task outcomes now

Those require separate decisions about workflow/task attribution, invocation identities, continuation ownership, lifecycle eligibility, and terminal evidence. A tool-free session turn proves the authority bridge without prematurely coupling those concerns.

## Consequences

- Callers can explicitly apply one selected phase's skills to a provider turn without converting workflows into schedulers.
- Existing skill-free and explicit exact-ID composed methods remain unchanged.
- Workflow lifecycle state and provider side effects remain separate operations.
- The caller remains responsible for deciding whether the chosen phase is appropriate for the session and broader task.

## Verification

The bounded slice follows RED→GREEN tests proving deterministic phase-bound provider skills, exclusion of unrelated registrations, authored topology preservation, and missing or malformed binding failure before transcript and provider side effects. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding workflow-aware tool turns, durable task outcomes, persisted phase execution evidence, automatic scheduling, lifecycle eligibility policy, retries, token budgets, dependencies, or phase completion semantics.
