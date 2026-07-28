# ADR-0008: Advance workflows through caller-owned in-memory cursors

- **Status:** accepted
- **Date:** 2026-07-28
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0006, ADR-0007, issues #682 and #683

## Context

ADR-0006 validates extension-authored workflow topology, and ADR-0007 admits that topology into caller-owned process-local registries without execution. Vela now needs a first machine-checkable advancement boundary, but phase actions, provider/tool authority, run persistence, and external gate evaluation remain undecided. Combining those concerns would make the first runtime boundary unnecessarily large and could let workflow availability imply authority.

The North Star defines workflows as explicit phases, gates, transitions, and stop conditions. A deterministic in-memory cursor can model only those structural semantics while leaving all side effects and durable orchestration for later decisions.

## Decision

The kernel provides a borrowed, process-local `WorkflowCursor` over one immutable `RegisteredWorkflow`. Construction resolves the workflow's exact declared start ID to exactly one authored phase, exposes the borrowed workflow and current phase, and performs no advancement. A missing or ambiguous exact start fails closed.

The caller alone chooses a transition by its authored zero-based index. Index selection preserves authored order and distinguishes repeated edges even when they have the same target. For a transition with a gate ID, the caller must provide an exact matching acknowledgement. A transition without a gate ID accepts no acknowledgement. Missing, unexpected, or mismatched acknowledgements are typed failures; acknowledgement means only that the caller resolved the named condition outside this boundary.

Advancement resolves the transition's exact target ID to exactly one authored phase, then updates only the cursor's current phase. Terminal phases reject advancement. Out-of-range transition indexes and missing or ambiguous targets fail before mutation. Every failed operation leaves the current phase unchanged. Cycles and repeated phase visits are permitted because validated version-one workflows may contain cycles.

The cursor borrows registry topology rather than cloning or modifying it. It has no run identity and is lost with process memory. Registration and cursor construction do not choose an edge automatically.

This boundary does not bind or invoke phase actions, skills, tools, agents, humans, or providers; evaluate external conditions; grant permission; persist state or evidence; schedule work; create checkpoints; pause, resume, retry, compensate, cancel, or run concurrently; replace definitions; or hot reload packages.

## Alternatives considered

### Treat exact registry lookup as workflow execution

Lookup exposes topology but does not represent a current phase, explicit transition choice, terminal rejection, gate acknowledgement, or atomic advancement failure. Conflating inspection with execution would erase a useful authority boundary.

### Choose transitions by target ID

Version-one validation preserves authored transition order and does not prohibit repeated targets. Target-only choice would make those edges ambiguous and discard authored identity. The authored index is the smallest deterministic selector.

### Evaluate gates inside the cursor

Gate IDs are inert metadata, and no gate registry, evaluator, evidence model, or permission contract exists yet. Exact caller acknowledgement keeps resolution authority outside the cursor and makes the trust boundary explicit.

### Persist runs immediately

Durable runs require run identity, definition snapshot/provenance, event versioning, recovery, cancellation, and checkpoint semantics. A pure cursor establishes deterministic transition rules that a later durable aggregate can reuse without prematurely choosing those policies.

### Invoke phase actions while advancing

Version-one definitions do not contain action bindings. Adding provider or tool calls now would invent authority and failure semantics outside the accepted schema.

## Consequences

### Positive

- Workflow advancement becomes explicit, deterministic, and independently testable.
- Authored transition order and repeated edges remain meaningful.
- Gate acknowledgement is exact without pretending to evaluate external conditions.
- Every failure is atomic and grants no capability authority.
- Later durable orchestration can reuse a small pure state transition boundary.

### Negative

- Cursor progress is process-local and cannot recover after restart.
- The caller owns gate-resolution correctness; the cursor checks identity only.
- Workflows still cannot perform useful phase actions on their own.
- Public kernel topology constructors can create malformed definitions, so cursor start and target resolution must fail closed even though extension-authored definitions were validated earlier.

## Verification

The first execution slice follows RED→GREEN tests proving exact borrowed start/current-phase exposure, authored-index selection for repeated edges, exact gated and ungated acknowledgement behavior, atomic typed failures for terminal phases, transition indexes, and malformed start/target resolution, and cyclic repeated visits. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding action bindings, automatic transition choice, gate evaluators or evidence, run identity, persistence/checkpoints, scheduling, pause/resume, cancellation, retries, compensation, concurrency, definition replacement, hot reload, or remote execution.
