<div align="center">

# Project Vela

**A Rust-native, self-improving AI assistant operating system.**

Part practical assistant. Part adversarial research partner. Part honest best friend.

[Vision](plans/00-north-star.md) · [Architecture](plans/01-architecture-research.md) · [System map](docs/project-vela-visual.html)

</div>

> [!IMPORTANT]
> Vela is at the specification and bootstrap stage. The repository does not yet contain a runnable assistant.

## North star

Vela is not intended to be a passive chatbot. She should notice conflict, challenge weak assumptions, remember useful lessons, improve her procedures, and build missing tools within explicit safety and review boundaries.

The system is guided by one engineering rule:

> Models reason, decide, synthesize, and communicate. Code parses, validates, schedules, checks, records, and enforces contracts.

## Bootstrap loop

Project Vela develops through an evidence-producing loop:

```text
External Rust references
        ↓
Retrieve relevant patterns
        ↓
Implement a focused Vela change
        ↓
Format · compile · lint · test · review
        ↓
Correct and verify
        ↓
Capture the development episode
        ↓
Grow the Vela-native corpus
        ↺
```

External Rust datasets help the assistant write better Rust. At the same time, Vela's own implementation produces higher-value project-native examples: tasks, context, patches, diagnostics, corrections, rationale, tests, and verified outcomes. Vela will eventually inherit this evidence from her own development.

## Intended architecture

Vela's small, inspectable Rust kernel will eventually own:

- identity and behavioral policy
- task and session lifecycles
- durable memory and event replay
- tools, skills, workflows, and extensions
- permissions and isolation
- observability and deterministic validation
- controlled, auditable self-improvement

Existing Rust frameworks may be studied or integrated, but Vela should not be a thin wrapper around one framework.

## Development environment

Nix is the supported development entry point. The flake pins the Rust toolchain, Rust quality tools, GitHub tooling, formatters, native libraries, and CI utilities used by the project:

```bash
nix develop
```

Run a single command without entering an interactive shell with:

```bash
nix develop --command <command>
```

The committed `flake.lock` keeps local development and CI reproducible across x86-64 Linux, ARM64 Linux, and Apple Silicon macOS.

Run the complete local quality gate with:

```bash
nix develop --command just verify
```

Before arming squash auto-merge for a pull request, verify its exact head and
review evidence with:

```bash
head=$(nix develop --command git rev-parse HEAD)
nix develop --command scripts/verify-merge-readiness <pr-number> "$head"
```

The read-only verifier reports deterministic `READY` or `BLOCKED` diagnostics;
it never merges the pull request. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for
the fail-closed review contract.

## Secret management

Vela commits secret **declarations** in [`secretspec.toml`](secretspec.toml), but
secret values must remain in an external SecretSpec provider such as the system
keyring. Set up your preferred provider once, verify the required values, and
then run commands with only their declared secrets injected:

```bash
nix develop --command secretspec config init
nix develop --command secretspec check
nix develop --command just with-secrets cargo run --locked -p vela-dev -- --help
```

CI and the local quality gate use an ephemeral, permission-restricted dotenv
fixture containing a disposable test value. They remove it on exit and never
require or print a developer credential. See
[`docs/adr/0001-declarative-secret-management.md`](docs/adr/0001-declarative-secret-management.md)
for the trust boundary and rationale.

## Developer CLI

The initial Rust workspace provides `vela-dev`, the command-line home for corpus and development-evidence tooling:

```bash
nix develop --command cargo run --locked -p vela-dev -- --help
nix develop --command cargo run --locked -p vela-dev -- record --help
```

The workspace includes schema-versioned development-record validation:

```bash
nix develop --command cargo run --locked -p vela-dev -- record validate path/to/record.json
```

Verified project-native records live under `corpus/development/`. Inspect every JSON record recursively, in deterministic relative-path order, with:

```bash
nix develop --command cargo run --locked -p vela-dev -- corpus inspect corpus/development
```

Inspection prints each valid record and an aggregate summary. It continues past malformed, unreadable, or semantically invalid records, emits path-prefixed diagnostics, and exits non-zero when any record is invalid.

Inspect one caller-selected extension root through the same validated, descriptor-anchored discovery boundary used by the extension library:

```bash
nix develop --command cargo run --locked -p vela-dev -- extension inspect path/to/extensions
```

The command prints tab-separated, debug-escaped exact ID, validated kind, authored entrypoint, and root-relative manifest path fields in deterministic manifest-path order, followed by an aggregate count. Escaping keeps untrusted tabs, newlines, terminal controls, and path bytes inside one quoted field. Invalid roots fail with one escaped diagnostic and without printing a partial catalog. Inspection is read-only and does not select, activate, compile, execute, or persist capabilities.

Invoke one exact validated tool with one JSON value through the existing isolated WebAssembly and per-invocation permission boundaries:

```bash
nix develop --command cargo run --locked -p vela-dev -- extension invoke path/to/extensions local.search '{"query":"Vela"}'
```

The explicit command authorizes only that one selected version-one `Pure` tool invocation. It uses a fresh process-local registry, default resource limits, and a fresh guest store; successful output is one compact JSON value. Malformed `INPUT_JSON` and discovery, selection, activation, authorization, guest, trap, resource-limit, or malformed-output failures emit one escaped diagnostic without partial stdout. Invocation does not persist activation or permission, grant host capabilities, retry, or run skills and workflows.

Create one inert durable one-shot schedule in the exact caller-selected
event-log database:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule create path/to/events.sqlite3 schedule-id 'exact task goal' 1750000000000
```

The command validates the exact ID and non-empty goal before opening writable
storage, appends one pending intent, and emits its complete compact JSON
projection. It may create the selected database. Duplicate IDs and storage
failures produce one escaped diagnostic without replacing existing intent.
Creation does not read ambient time, claim, dispatch, retry, or execute work.

Cancel one exact observed pending schedule revision with caller-owned evidence:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule cancel path/to/events.sqlite3 schedule-id 1 'operator request'
```

The command validates the exact ID and non-blank reason before opening writable
storage, then appends cancellation only if the supplied revision is still
pending. Success emits the complete compact cancelled schedule projection.
Missing, stale, claimed, materialized, or already-cancelled schedules fail with
one escaped diagnostic and no lifecycle append. Cancellation does not read
ambient time, interrupt dispatched work, claim, retry, or execute anything.

Claim one exact observed due schedule revision against a caller-owned cutoff:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule claim path/to/events.sqlite3 schedule-id 1 1750000000000
```

The command validates the exact ID and numeric revision/cutoff before opening
writable storage, then appends a claim only if the supplied revision remains
pending and due at or before the inclusive cutoff. Success emits the complete
compact claimed schedule JSON object. Missing, future, stale, cancelled,
materialized, or already-claimed schedules fail with one escaped diagnostic and
no lifecycle append. Claiming does not read ambient time, identify a worker,
dispatch, materialize, retry, grant permission, or execute anything.

Claim the earliest pending schedule due by one caller-owned cutoff:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule claim-next path/to/events.sqlite3 1750000000000
```

The command delegates due-instant then exact-ID selection and revision-bound
conflict handling to the kernel. Success emits `{"schedule":...}` with the
complete claimed projection, or `{"schedule":null}` when no eligible work
remains. Failures emit `schedule_claim_failed` and no partial stdout. It does
not read ambient time, generate a task ID, identify a worker, dispatch,
materialize, grant permission, or execute work.

Release one exact observed claim with caller-owned recovery evidence:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule release path/to/events.sqlite3 schedule-id 2 'worker recovery'
```

The command validates the exact ID and non-blank reason before opening writable
storage, then appends a release only if the supplied revision remains claimed.
Success emits the complete compact pending schedule JSON object with its exact
latest release reason. Missing, stale, pending, cancelled, or materialized
schedules fail with one escaped diagnostic and no lifecycle append. Release
does not infer worker death, read ambient time, dispatch, materialize, retry,
grant permission, or execute anything.

Materialize one exact observed claim as one caller-identified inert active task:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule materialize path/to/events.sqlite3 schedule-id 2 task-id
```

The command validates both exact identities and the numeric revision before
opening writable storage, then atomically appends the materialization and task
start only if the supplied revision remains claimed and the task ID is unused.
Success emits the complete compact materialized schedule JSON object. Missing,
stale, pending, cancelled, already-materialized, or task-colliding inputs fail
with one escaped diagnostic, no orphan task, and no schedule append.
Materialization does not read ambient time, infer worker identity, dispatch,
advance a workflow, call a provider or tool, retry, or execute work.

Atomically materialize the earliest pending due schedule as one caller-identified
inert active task:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule materialize-next path/to/events.sqlite3 1750000000000 task-id
```

The command validates the exact task identity and numeric cutoff before opening
writable storage, then delegates deterministic due-instant/exact-ID selection and
atomic schedule/task persistence to the kernel. Success emits
`{"schedule":...}` with the complete materialized projection, or
`{"schedule":null}` when no eligible work remains. Task-ID collisions and
storage or replay failures emit `schedule_materialization_failed`, no partial
stdout, no schedule append, and no orphan task. The command does not read
ambient time, generate identity, dispatch, advance a workflow, call a provider
or tool, grant permission, retry work, or execute anything.

Inspect every durable one-shot schedule in an existing event-log database without
granting the CLI write or creation authority:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule inspect path/to/events.sqlite3
```

The command emits one compact JSON document whose `schedules` array is ordered
by exact schedule ID. Each entry preserves the exact ID, goal, Unix-millisecond
due instant, lowercase lifecycle status, revision, and nullable cancellation,
latest-release, and task-binding evidence. A missing or invalid database fails
without creating storage or printing partial JSON. Inventory inspection cannot
read time, choose a due cutoff, mutate lifecycle state, dispatch, or execute
work.

Inspect one exact schedule's current validated projection:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule get path/to/events.sqlite3 schedule-id
```

The command validates the exact schedule ID before opening storage read-only and
emits `id` plus either the complete schedule object used by inventory inspection
or `"schedule":null` for a valid missing ID. Invalid IDs and malformed durable
state fail without partial output or storage creation. Exact lookup cannot read
time, mutate lifecycle state, dispatch, or execute work.

Inspect schedules with one exact persisted lifecycle status:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule status path/to/events.sqlite3 pending
```

The final argument must be exactly `pending`, `cancelled`, `claimed`, or
`materialized`. Validation happens before storage access. The command uses the
same complete schedule-object shape and exact-ID ordering as inventory
inspection; no matches produce an empty `schedules` array. Status inspection is
read-only and cannot read time, mutate, claim, dispatch, retry, materialize, or
execute work.

Inspect only pending schedules due at or before one explicit caller-owned cutoff
without reading ambient time:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule due path/to/events.sqlite3 1754049600000
```

The command uses the same read-only database and compact schedule-object
contract, while preserving the kernel's due-instant then exact-ID ordering. An
invalid cutoff fails during argument parsing without opening or creating the
database. Due inspection does not claim, dispatch, materialize, or execute work.

Inspect one exact schedule's complete validated lifecycle history without
granting lifecycle authority:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule history path/to/events.sqlite3 schedule-id
```

The command emits the exact schedule ID plus revision-ordered typed creation,
claim, release, cancellation, and materialization evidence. A valid missing ID
is represented by `"history":null`; invalid IDs fail before storage is opened.
History inspection opens storage read-only and cannot mutate or execute work.

Resolve the exact materialized schedule bound to one caller-owned task identity:

```bash
nix develop --command cargo run --locked -p vela-dev -- schedule task path/to/events.sqlite3 task-id
```

The command validates the exact task ID before opening storage read-only and
emits `task_id` plus either the complete deterministic schedule object or
`"schedule":null` for an unbound identity. Ambiguous corrupted bindings fail
closed without partial output. Task lookup cannot mutate lifecycle state,
dispatch, or execute work.

Resolve the exact materialized recurrence occurrence bound to one caller-owned
task identity:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence task path/to/events.sqlite3 task-id
```

The command validates the exact task ID before opening existing storage
read-only and emits `task_id` plus either the complete deterministic materialized
occurrence object or `"occurrence":null` for an unbound identity. Missing
storage remains missing, and malformed or ambiguous selected bindings fail
closed without partial output. Recurrence task lookup cannot read time, mutate,
enumerate unrelated coordinates, dispatch, or execute work.

Page complete materialized task bindings for one exact recurrence through a
bounded read-only authored-offset window:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence materialized path/to/events.sqlite3 recurrence-id 0 128
```

The command emits bindings in ascending offset order with exact recurrence,
occurrence-revision, and task provenance. Missing and persisted-only coordinates
are omitted while `next_offset` advances by inspected authored coordinates, so
empty sparse pages still make deterministic progress. Validation precedes
storage access; missing storage remains missing, and selected corruption fails
closed without partial output. Paging reads no clock, mutates nothing, and
grants no catch-up, dispatch, permission, or execution authority.

Page one exact finite recurrence through an explicit inclusive due cutoff:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence due path/to/events.sqlite3 recurrence-id 0 128 1754049600000
```

The command validates the exact identity and bounded page size before opening
existing storage read-only, then emits complete occurrences in authored-offset
order. `next_offset` identifies the first uninspected coordinate after a full
page, the first future coordinate when the cutoff stops selection, or `null`
only at the finite definition end. An empty page can therefore preserve a
non-null cursor for a later caller-owned cutoff. Missing storage remains missing,
and selected malformed definition evidence fails closed without partial JSON.
The command reads no clock, persists no cursor, chooses no catch-up policy, and
cannot generate identity, materialize, dispatch, or execute work.

Select only the latest occurrence due from one caller-owned authored coordinate:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence latest-due path/to/events.sqlite3 recurrence-id 0 1754049600000
```

The command validates the exact identity before opening existing storage
read-only and delegates latest-only selection to the kernel. Success emits one
complete occurrence plus its following authored offset, `null` with an unchanged
cursor when the starting coordinate is still future, or a `null` cursor at finite
completion. Missing storage remains missing; selected malformed evidence fails
closed without stdout, while unrelated corruption cannot block the exact query.
The explicit cutoff remains caller authority. Selection reads no ambient clock,
persists no cursor or skip evidence, discovers no unrelated definitions, and
cannot generate identity, materialize, dispatch, retry, or execute work.

Atomically persist only that explicit latest-due selection against one observed
immutable definition revision:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence persist-latest-due path/to/events.sqlite3 recurrence-id 1 0 1754049600000
```

The command delegates constant-space latest-only selection and the atomic write
to the kernel. It emits the same complete occurrence and `next_offset` shape as
read-only selection. Skipped coordinates remain uninspected and unpersisted;
the returned cursor is not durable skip, acceptance, or lifecycle evidence. A
future horizon writes nothing, while stale definitions, duplicates, and
malformed selected provenance fail closed without partial stdout. The command
reads no clock, generates no identity, and cannot materialize, dispatch, retry,
or execute work.

Atomically materialize that explicit latest-due selection as one caller-owned
inert task:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence materialize-latest-due path/to/events.sqlite3 recurrence-id 1 0 1754049600000 task-id
```

The command validates both identities before storage access, then delegates the
constant-space selection and atomic persisted-to-materialized occurrence plus
`task.started` write to the kernel. Success emits the complete task-bound
occurrence and resumable `next_offset`; a future horizon emits a null occurrence,
preserves the cursor, and writes nothing. Skipped coordinates remain absent.
Stale definitions, selected duplicates or corruption, task collisions, and
storage failures emit no partial stdout and leave no orphan task. The command
reads no clock, generates no identity, and cannot dispatch, retry, or execute
work.

Atomically persist one exact recurrence's bounded due page through the same
explicit cutoff and one observed definition revision:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence persist-due path/to/events.sqlite3 recurrence-id 1 0 128 1754049600000
```

The command validates identity and page bounds before opening writable storage,
then delegates inclusive selection, cursor semantics, duplicate protection, and
all-or-nothing persistence to the kernel. Success emits the same complete JSON
page shape as read-only due paging. Missing or stale definitions, invalid starts,
duplicates, selected corruption, and storage failures emit no partial JSON and
persist no selected prefix. An empty future-horizon page keeps its unchanged
cursor; a later caller-owned cutoff can resume it. The command reads no ambient
clock, persists no cursor, generates no identity, and cannot choose catch-up
policy, materialize tasks, dispatch, or execute work.

Inspect one exact persisted recurrence occurrence without granting write,
inventory, catch-up, or execution authority:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence occurrence path/to/events.sqlite3 recurrence-id 0
```

The command validates the exact recurrence ID before opening the existing
database read-only, then emits compact JSON preserving the persisted coordinate's
exact ID, goal, offset, Unix-millisecond instant, and definition revision. A
valid absent coordinate emits `recurrence_occurrence_not_found`; malformed
selected provenance fails closed as `recurrence_occurrence_lookup_failed`.
Missing storage is never created. Exact lookup does not project unpersisted
coordinates, scan occurrence inventory, read time, choose catch-up policy,
materialize, dispatch, or execute work.

Page one exact recurrence's sparse persisted occurrence provenance without
granting global discovery or lifecycle authority:

```bash
nix develop --command cargo run --locked -p vela-dev -- recurrence persisted path/to/events.sqlite3 recurrence-id 0 100
```

The command validates the exact recurrence ID and bounded page size before
opening the existing database read-only. It emits only durable coordinates from
the selected authored window in ascending offset order; gaps are omitted and
`next_offset` advances by inspected authored coordinates, including across empty
pages. Missing storage is never created, and malformed selected-window evidence
fails closed. Paging reads no time, persists no cursor, performs no global
inventory, and cannot choose catch-up policy, materialize, dispatch, or execute
work.

See [`docs/development-record-v1.md`](docs/development-record-v1.md) for the version 1 shape, invariants, stable diagnostics, and exit statuses.

## Development status

The first milestone is the **evidence loop**:

1. Establish the public GitHub workflow and Rust quality gates. ✅
2. Build a minimal `vela-dev` CLI. ✅
3. Define and validate development records. ✅
4. Store and inspect a small Vela-native corpus. ✅
5. Use the creation of that tooling as the first real corpus episode. ✅
6. Begin the kernel with a typed append-only event log and replay. ✅
7. Start the persisted task lifecycle with durable start, output-bearing completion, reason-bearing cancellation, diagnosed failure, and load. ✅
8. Start the persisted session lifecycle with durable creation, close, reopen, and load. ✅
9. Connect persisted tasks to open sessions with an immutable association. ✅
10. Query each persisted session's associated tasks in deterministic order. ✅
11. Discover every persisted session in deterministic order without a separate index. ✅
12. Discover every persisted task in deterministic order without a separate index. ✅
13. Persist ordered human and assistant conversation turns within open sessions. ✅
14. Persist ordered typed task execution observations while tasks are active. ✅
15. Establish an explicit per-invocation permission boundary for in-process tool adapters. ✅
16. Persist metadata-only tool invocation intent and terminal outcomes with fail-closed replay. ✅
17. Discover every persisted tool invocation in deterministic order without a separate index. ✅
18. Immutably attribute durable tool invocation evidence to active tasks. ✅
19. Register, discover, and invoke in-process tool adapters by stable ID. ✅
20. Dispatch registered adapters by stable ID through durable task-associated invocation. ✅
21. Execute one provider-requested tool step with caller-owned identity and permission. ✅
22. Continue explicitly after one successful provider tool step. ✅
23. Start a durable task turn through one bounded provider/tool step. ✅
24. Persist explicit provider/tool continuations within the originating durable task turn. ✅
25. Complete tasks explicitly through bounded provider/tool turns and continuations. ✅
26. Fail tasks explicitly through bounded provider/tool turns and continuations. ✅
27. Cancel tasks explicitly through bounded provider/tool turns and continuations. ✅
28. Persist linked corrections through bounded provider/tool turns and continuations. ✅
29. Validate versioned tool, skill, and workflow extension manifests without activating them. ✅
30. Discover validated manifests deterministically from one caller-selected extension root. ✅
31. Reject exact duplicate capability IDs within one discovered extension root. ✅
32. Validate portable extension-local entrypoint references without resolving or activating them. ✅
33. Validate discovered entrypoint targets as extension-local regular files without reading or activating them. ✅
34. Build immutable discovered extension registry snapshots with exact-ID lookup. ✅
35. Compare immutable extension registry snapshots deterministically without activation. ✅
36. Select caller-requested registry capabilities by exact ID without activation. ✅
37. Expose immutable typed selections as non-activating enablement intent. ✅
38. Partition selected capabilities by validated kind without activation. ✅
39. Fail closed when typed selection intent disagrees with a capability's validated kind. ✅
40. Specify the first tools-only WebAssembly component activation and isolation boundary. ✅
41. Reacquire selected tool entrypoints as bounded, descriptor-anchored owned artifacts. ✅
42. Compile selected no-import tool components against the exact inert version 0.1.0 ABI. ✅
43. Invoke compiled tools through fresh resource-limited stores and exact JSON validation. ✅
44. Activate selected tools into caller-owned registries with atomic all-or-nothing registration. ✅
45. Apply caller-selected uniform resource limits through atomic tool activation. ✅
46. Revoke selected process-local tool adapters through atomic fail-closed deactivation. ✅
47. Refresh selected active tool adapters through explicit atomic fail-closed replacement. ✅
48. Reconcile previous and current selected active tool sets as one atomic remove/replace/add transition. ✅
49. Reject non-tool activation and replacement intent before filesystem access. ✅
50. Inspect validated extension catalogs through the developer CLI without activation. ✅
51. Invoke one exact validated WebAssembly tool through the developer CLI permission boundary. ✅
52. Prepare selected skill entrypoints as inert bounded UTF-8 instruction artifacts. ✅
53. Register prepared skills into caller-owned process-local registries atomically without prompt influence. ✅
54. Compose explicitly selected registered skills into provider-neutral tool-free turns. ✅
55. Preserve explicit skill composition across caller-driven bounded provider/tool continuations. ✅
56. Preserve explicit skill composition across durable attempt-producing task tool turns. ✅
57. Preserve explicit skill composition and parent lineage across durable correction tool turns. ✅
58. Preserve explicit skill composition across durable completion tool turns. ✅
59. Preserve explicit skill composition and caller-owned diagnostics across durable failure tool turns. ✅
60. Preserve explicit skill composition and caller-owned reasons across durable cancellation tool turns. ✅
61. Prepare selected declarative workflow definitions as inert validated state machines. ✅
62. Register prepared workflows into caller-owned process-local registries atomically without execution. ✅
63. Advance one registered workflow through a caller-owned in-memory cursor with explicit transition and gate acknowledgement. ✅
64. Persist workflow-run starts with immutable owned topology provenance and registry-free replay. ✅
65. Advance durable workflow runs through revision-bound explicit transitions and exact gate acknowledgement. ✅
66. Cancel durable non-terminal workflow runs through revision-bound caller-owned reasons without rewriting topology. ✅
67. Discover every durable workflow run in deterministic order from authoritative start events without a separate index. ✅
68. Pause and resume durable workflow runs through revision-bound caller-owned reasons without changing topology. ✅
69. Query exact durable workflow-run lifecycle history as revision-ordered typed semantic evidence. ✅
70. Fail durable workflow runs through revision-bound caller-owned diagnostics without changing topology. ✅
71. Classify every durable workflow run through one unified read-only lifecycle status projection. ✅
72. Attribute workflow-run starts immutably to active tasks with atomic task-revision validation. ✅
73. Discover task-attributed workflow runs through an exact deterministic historical query. ✅
74. Discover runs for one exact immutable workflow identity through a deterministic historical query. ✅
75. Discover workflow runs in one exact lifecycle status through a deterministic historical query. ✅
76. Compose exact task, workflow, and lifecycle constraints through one deterministic workflow-run filter. ✅
77. Preserve inert authored-order skill bindings through workflow preparation, registration, and durable run replay. ✅
78. Resolve one caller-chosen current phase's inert skill bindings explicitly through the existing skill registry selection boundary. ✅
79. Execute one caller-chosen workflow phase through the existing explicit tool-free composed provider boundary. ✅
80. Preserve one caller-chosen workflow phase response as durable task Attempt evidence. ✅
81. Preserve one caller-chosen workflow phase response as linked task Correction evidence. ✅
82. Complete a task explicitly through one caller-chosen workflow phase response. ✅
83. Fail a task explicitly through one caller-chosen workflow phase response and caller-owned diagnostic. ✅
84. Cancel a task explicitly through one caller-chosen workflow phase response and caller-owned reason. ✅
85. Preserve one caller-chosen workflow phase response as linked task Diagnostic evidence. ✅

86. Record independently observed Verification evidence for one exact task Attempt through a separate caller-owned verifier boundary. ✅
87. Persist explicit passed or failed outcomes for independent task Verification without granting lifecycle authority. ✅
88. Identify each new structured independent task Verification check without fabricating historical provenance. ✅
89. Evaluate a caller-owned required Verification gate set deterministically for one exact task Attempt. ✅
90. Complete a task through an explicit caller-selected gate boundary without stale green authorization. ✅
91. Advance a task-attributed workflow through exact authored gate Verification without stale green authorization. ✅
92. Atomically complete an active task while its attributed workflow advances through verified authored gate evidence into terminal. ✅
93. Persist inert one-shot task schedule intent and query due work against a caller-owned cutoff deterministically. ✅
94. Durably cancel pending one-shot schedule intent before it can be selected as due work. ✅
95. Durably claim due one-shot schedule intent without granting dispatch or execution authority. ✅
96. Discover every durable one-shot schedule intent in deterministic exact-ID order. ✅
97. Explicitly release claimed one-shot schedule intent back to pending eligibility with durable recovery evidence. ✅
98. Atomically materialize one claimed schedule as one caller-identified active task without executing it. ✅
99. Filter durable one-shot schedule inventory by exact persisted lifecycle status without granting execution authority. ✅
100. Query one exact durable schedule's complete typed lifecycle history without granting execution authority. ✅
101. Resolve a materialized schedule from its exact task identity without granting lifecycle authority. ✅
102. Bind schedule release and materialization to one exact persisted claim revision so stale claimants cannot consume later claims. ✅
103. Bind pending schedule cancellation and claiming to exact persisted revisions so stale observers cannot consume recovered intent. ✅
104. Open existing event-log and schedule evidence through an explicit read-only SQLite boundary without creating a database or initializing event schema. ✅
105. Inspect durable one-shot schedule inventory through deterministic JSON without granting CLI mutation authority. ✅
106. Inspect pending schedules due by one explicit cutoff through deterministic JSON without reading time or granting CLI mutation authority. ✅
107. Inspect one exact durable schedule's typed lifecycle history through deterministic JSON without granting CLI lifecycle authority. ✅
108. Resolve durable schedule provenance from one exact task identity through deterministic JSON without granting CLI lifecycle authority. ✅
109. Inspect durable schedules by one exact lifecycle status through deterministic JSON without granting CLI lifecycle authority. ✅
110. Create inert durable one-shot schedule intent through deterministic JSON without granting dispatch or execution authority. ✅
111. Cancel one exact pending durable schedule revision through deterministic JSON without granting interruption or execution authority. ✅
112. Claim one exact due durable schedule revision through deterministic JSON without granting dispatch or execution authority. ✅
113. Release one exact claimed durable schedule revision through deterministic JSON without inferring worker state or granting dispatch authority. ✅
114. Materialize one exact claimed durable schedule revision through deterministic JSON without granting dispatch or execution authority. ✅
115. Reserve the next deterministic due schedule through revision-bound optimistic concurrency without granting dispatch or execution authority. ✅
116. Reserve the next deterministic due schedule through compact CLI JSON without reading ambient time or granting dispatch authority. ✅
117. Atomically materialize the next deterministic due schedule as one caller-identified inert active task without granting execution authority. ✅
118. Materialize the next deterministic due schedule through compact CLI JSON without reading ambient time or granting dispatch authority. ✅
119. Derive zero-based fixed-interval occurrence instants with overflow-safe constant-time arithmetic without granting persistence or dispatch authority. ✅
120. Persist immutable finite fixed-interval recurrence definitions with prevalidated representable bounds without generating occurrences or granting dispatch authority. ✅
121. Project one exact finite recurrence occurrence with complete read-only provenance and typed bounds without granting persistence or execution authority. ✅
122. Page finite recurrence occurrences through allocation-bounded ordered read-only projections with deterministic cursors and no lifecycle authority. ✅
123. Inspect every finite recurrence definition through deterministic fail-closed read-only inventory without granting storage mutation or execution authority. ✅
124. Create one immutable finite recurrence definition through deterministic CLI JSON without granting occurrence or execution authority. ✅
125. Inspect one exact finite recurrence definition through deterministic read-only CLI JSON without scanning unrelated streams. ✅
126. Page one exact durable recurrence's occurrences through bounded deterministic CLI JSON without granting lifecycle or execution authority. ✅
127. Persist one exact finite recurrence occurrence as canonical fail-closed durable provenance without selecting, materializing, or executing work. ✅
128. Persist one exact recurrence occurrence through deterministic writable CLI JSON without granting catch-up, materialization, or execution authority. ✅
129. Inspect one exact persisted recurrence occurrence through deterministic read-only CLI JSON without scanning unrelated streams or granting lifecycle authority. ✅
130. Page sparse persisted recurrence provenance through bounded exact-recurrence authored-offset windows without granting global discovery or lifecycle authority. ✅
131. Page sparse persisted recurrence provenance through bounded deterministic CLI JSON without granting global discovery or lifecycle authority. ✅
132. Atomically materialize one exact persisted recurrence occurrence as one caller-identified inert active task without granting execution authority. ✅
133. Materialize one exact persisted recurrence occurrence through deterministic writable CLI JSON without granting catch-up, dispatch, or execution authority. ✅
134. Resolve one exact materialized recurrence occurrence from its task identity without granting lifecycle or execution authority. ✅
135. Inspect exact materialized recurrence provenance by task identity through deterministic read-only CLI JSON without granting discovery or execution authority. ✅
136. Page sparse materialized recurrence bindings through bounded exact-recurrence authored-offset windows without granting global discovery or lifecycle authority. ✅
137. Page sparse materialized recurrence bindings through bounded deterministic CLI JSON without granting global discovery or lifecycle authority. ✅
138. Page one exact finite recurrence through an inclusive caller-owned due cutoff with a resumable bounded cursor and no catch-up or execution authority. ✅
139. Page one exact recurrence's due occurrences through deterministic read-only CLI JSON without reading ambient time or granting catch-up or execution authority. ✅
140. Atomically persist one exact recurrence's bounded due page without partial provenance or granting catch-up or execution authority. ✅
141. Persist one exact recurrence's bounded due page through deterministic writable CLI JSON without reading ambient time or granting catch-up or execution authority. ✅
142. Select the latest due occurrence from one exact finite recurrence through explicit constant-space catch-up policy without reading ambient time or granting lifecycle authority. ✅
143. Expose exact latest-due recurrence selection through deterministic read-only CLI JSON without adding clock, persistence, discovery, or execution authority. ✅
144. Atomically persist one exact latest-due recurrence selection without recording skipped work or granting lifecycle or execution authority. ✅
145. Persist one exact latest-due recurrence selection through deterministic writable CLI JSON without recording skipped work or granting execution authority. ✅
146. Atomically materialize one exact latest-due recurrence selection as one caller-identified inert task without partial provenance or granting execution authority. ✅
147. Materialize one exact latest-due recurrence selection through deterministic writable CLI JSON without recording skipped work or granting execution authority. ✅

## Project documents

- [`plans/00-north-star.md`](plans/00-north-star.md) — identity and operating principles
- [`plans/01-architecture-research.md`](plans/01-architecture-research.md) — Rust ecosystem and kernel boundaries
- [`plans/02-rust-dataset-understanding.md`](plans/02-rust-dataset-understanding.md) — external dataset findings
- [`plans/03-rust-corpus-strategy.md`](plans/03-rust-corpus-strategy.md) — corpus design and quality strategy
- [`plans/04-assistant-first-rust-mentor.md`](plans/04-assistant-first-rust-mentor.md) — assistant-first Rust feedback loop
- [`docs/project-vela-visual.html`](docs/project-vela-visual.html) — standalone visual system map
- [`docs/event-log.md`](docs/event-log.md) — typed append/replay behavior and stable errors
- [`docs/task-lifecycle.md`](docs/task-lifecycle.md) — persisted task start/completion/cancellation/load behavior
- [`docs/session-lifecycle.md`](docs/session-lifecycle.md) — persisted session lifecycle behavior
- [`docs/tool-permissions.md`](docs/tool-permissions.md) — in-process tool permission and durable invocation-evidence behavior
- [`docs/extension-manifests.md`](docs/extension-manifests.md) — versioned capability metadata and its non-executing trust boundary
- [`docs/scheduler.md`](docs/scheduler.md) — durable one-shot schedule intent and caller-owned time authority
- [`docs/adr/`](docs/adr/) — architecture decision records

## Contributing

Project Vela is being built in public. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for branch, review, verification, and corpus-safety rules.

## License

No project license has been selected yet. Until one is added, copyright law reserves all rights to the project owner.
