# Vela Rust Crate and File Layout

## Purpose
This document turns the Vela plan into a concrete workspace layout.

It optimizes for:
- a **small but real Rust kernel first**
- a **Hermes-faithful memory/learning subsystem first**
- **reloadable extensions** where experimentation speed matters
- a clean seam for **Honcho later**

---

## Design rule
Put durable, shared, behavior-critical logic in Rust crates.

Keep rapidly changing or high-variability behavior behind:
- config
- markdown/content files
- tool/skill/workflow registries
- provider/plugin boundaries

---

## Top-level workspace
```text
apps/
  vela/
crates/
  vela-compat/
  vela-config/
  vela-state/
  vela-session/
  vela-memory/
  vela-skills/
  vela-tools/
  vela-models/
  vela-runtime/
  vela-scheduler/
  vela-gateway/
  vela-review/
  vela-plugins/
  vela-memory-honcho/   # later
```

Some of these already exist. Some are proposed next-step splits.

---

## What each crate should own

## `apps/vela`
### Purpose
The user-facing binary.

### Owns
- CLI entrypoint
- top-level command routing
- bootstrap into runtime
- human-readable status/diagnostics output

### Should not own
- SQLite logic
- memory rules
- skill logic
- provider internals
- gateway internals

### Likely files
```text
apps/vela/src/
  main.rs
  commands/
    status.rs
    chat.rs
    memory.rs
    skills.rs
    sessions.rs
    gateway.rs
    reload.rs
```

---

## `vela-compat`
### Purpose
Reference/migration helpers for Hermes/Vela-compatible behavior.

### Owns
- import/export helpers
- compatibility adapters
- legacy surface interpretation
- migration transforms

### Example responsibilities
- OpenClaw/Hermes/Vela migration helpers
- compatibility snapshot readers/writers
- legacy config interpretation

### Likely files
```text
crates/vela-compat/src/
  lib.rs
  import/
    hermes.rs
    openclaw.rs
  snapshot/
    session_json.rs
  config/
    legacy.rs
```

---

## `vela-config`
### Purpose
Central config loading and resolved runtime settings.

### Why split it out
Config is going to become foundational for:
- profiles
- reload behavior
- plugin enable/disable
- provider selection
- memory approval gates
- future Honcho activation

It should not stay buried inside runtime forever.

### Owns
- `VELA_HOME` resolution
- profile selection
- env loading
- YAML config loading
- precedence rules
- resolved config struct
- hot-reload-safe config snapshots

### Likely files
```text
crates/vela-config/src/
  lib.rs
  home.rs
  profile.rs
  env.rs
  loader.rs
  merge.rs
  schema.rs
  reload.rs
```

---

## `vela-state`
### Purpose
Canonical persistence layer.

### Owns
- `state.db`
- migrations
- connection setup
- WAL/DELETE fallback
- FTS5 virtual tables
- durable metadata tables
- snapshot compatibility metadata

### This is the bottom persistence layer
Other crates should call into `vela-state`; they should not open ad-hoc SQLite connections all over the repo.

### Owns tables like
- `state_meta`
- `sessions`
- `messages`
- `session_events`
- FTS tables
- later approval/pending tables if desired

### Likely files
```text
crates/vela-state/src/
  lib.rs
  db.rs
  connection.rs
  pragma.rs
  migrations/
    mod.rs
    v1_bootstrap.rs
    v2_sessions.rs
    v3_messages.rs
    v4_fts.rs
  repo/
    sessions.rs
    messages.rs
    events.rs
    meta.rs
```

---

## `vela-session`
### Purpose
Session lifecycle and continuity semantics.

### Owns
- create/resume/continue behavior
- title/id policy
- interaction mode selection
- latest-session lookup semantics
- snapshot export triggers

### Why separate from `vela-runtime`
Runtime will become too large if it owns both turn orchestration and persistence-facing session lineage rules.

### Likely files
```text
crates/vela-session/src/
  lib.rs
  identity.rs
  lifecycle.rs
  lineage.rs
  titles.rs
  snapshots.rs
```

---

## `vela-memory`
### Purpose
Hermes-faithful built-in memory.

### Owns
- `MEMORY.md`
- `USER.md`
- bounded write logic
- frozen prompt snapshot construction
- memory tool actions
- write approval staging for memory
- memory target semantics (`memory` vs `user`)

### Important boundary
`vela-memory` owns **small curated always-on memory**.
It does **not** own session search.
It does **not** own skills.

### Likely files
```text
crates/vela-memory/src/
  lib.rs
  store.rs
  files.rs
  limits.rs
  render.rs
  tool.rs
  approvals.rs
  types.rs
```

### Key behaviors to copy first
- `MEMORY.md` and `USER.md` under `~/.vela/memories/`
- char limits
- frozen snapshot at session start
- add/replace/remove/view semantics
- no silent compaction

---

## `vela-skills`
### Purpose
Procedural memory.

### Owns
- `~/.vela/skills/`
- skill discovery/indexing
- progressive disclosure loading
- skill read/write/update/delete
- staged approvals for skill writes
- external skill directory support later

### Important boundary
`vela-skills` owns longer reusable procedures.
It should be separate from tiny built-in memory.

### Likely files
```text
crates/vela-skills/src/
  lib.rs
  index.rs
  loader.rs
  parser.rs
  manage.rs
  approvals.rs
  types.rs
```

---

## `vela-tools`
### Purpose
Tool interfaces and core local tool implementations.

### Owns
- tool registry
- tool trait
- read/write/edit/process tools
- approval-aware local execution boundaries
- memory and skill tool adapters into `vela-memory` / `vela-skills`

### Important boundary
Tool implementations can call memory/skills/state crates, but the canonical memory logic should not live in `vela-tools`.

### Likely files
```text
crates/vela-tools/src/
  lib.rs
  registry.rs
  traits.rs
  approval.rs
  file/
    read.rs
    write.rs
    edit.rs
  process/
    shell.rs
  memory_tool.rs
  skills_tool.rs
```

---

## `vela-models`
### Purpose
Model/provider abstraction.

### Owns
- provider trait
- request/response types
- streaming event abstraction
- backend registration
- local/OpenAI-compatible backend adapters

### Important boundary
This crate should not know about:
- memory file formats
- skill files
- gateway transports

It should only expose a clean inference/backend layer.

### Likely files
```text
crates/vela-models/src/
  lib.rs
  types.rs
  streaming.rs
  registry.rs
  providers/
    openai_compatible.rs
    anthropic_compatible.rs
    local.rs
```

---

## `vela-runtime`
### Purpose
The kernel orchestrator.

### Owns
- bootstrap assembly
- session start logic
- turn loop coordination
- prompt/context assembly
- memory snapshot injection at session start
- routing between models, tools, sessions, review, state

### Important boundary
`vela-runtime` should orchestrate, not become the implementation home for every subsystem.

### Depends on
- `vela-config`
- `vela-state`
- `vela-session`
- `vela-memory`
- `vela-skills`
- `vela-tools`
- `vela-models`
- later `vela-review`, `vela-scheduler`, `vela-plugins`

### Likely files
```text
crates/vela-runtime/src/
  lib.rs
  bootstrap.rs
  context.rs
  turn_loop.rs
  session_start.rs
  resume.rs
  status.rs
  reload.rs
```

---

## `vela-scheduler`
### Purpose
Background jobs and recurring execution.

### Owns
- cron parsing
- wakeups
- durable job persistence hooks
- restart recovery logic for jobs
- delivery handoff into runtime/gateway

### Likely files
```text
crates/vela-scheduler/src/
  lib.rs
  cron.rs
  jobs.rs
  recovery.rs
  dispatch.rs
```

---

## `vela-gateway`
### Purpose
Long-lived daemon and external surfaces.

### Owns
- gateway daemon
- attach/resume over surfaces
- pairing/auth state
- platform routing
- inbound/outbound message dispatch

### Important boundary
This crate should use runtime/session/state, not reinvent them.

### Likely files
```text
crates/vela-gateway/src/
  lib.rs
  daemon.rs
  pairing.rs
  routing.rs
  sessions.rs
  platforms/
    mod.rs
    telegram.rs
    discord.rs
```

---

## `vela-review`
### Purpose
Background self-improvement review.

### Owns
- post-turn review pipeline
- classify durable fact vs procedural skill
- write proposals into memory/skills
- approval-stage integration

### Why separate
This is the “learning loop” logic. It will evolve a lot, so keeping it separate from core runtime will help.

### Likely files
```text
crates/vela-review/src/
  lib.rs
  review.rs
  classify.rs
  memory_candidate.rs
  skill_candidate.rs
  stage.rs
```

---

## `vela-plugins`
### Purpose
Reloadable extension/plugin registry.

### Owns
- plugin manifests
- enable/disable state
- lifecycle hooks
- plugin catalog/index
- reload coordination for extension-level capabilities

### Important boundary
This crate manages extension metadata and lifecycle, not the entire runtime.

### Likely files
```text
crates/vela-plugins/src/
  lib.rs
  manifest.rs
  registry.rs
  lifecycle.rs
  reload.rs
```

---

## `vela-memory-honcho` (later)
### Purpose
Honcho integration as an external provider.

### Owns
- Honcho adapter
- ingest from session/review events
- optional retrieval/user-model enrichment

### Important rule
Honcho should **augment**, not replace:
- `MEMORY.md`
- `USER.md`
- `state.db`
- `skills/`

### Likely files
```text
crates/vela-memory-honcho/src/
  lib.rs
  adapter.rs
  ingest.rs
  search.rs
  profile.rs
```

---

## First-step concrete recommendation for the current workspace
You do **not** need to create all crates immediately.

Start by evolving the current workspace like this:

### Keep now
- `vela`
- `vela-compat`
- `vela-runtime`
- `vela-state`
- `vela-tools`
- `vela-models`

### Add next
- `vela-config`
- `vela-memory`
- `vela-skills`

### Add after that
- `vela-session`
- `vela-review`
- `vela-scheduler`

### Add later
- `vela-gateway`
- `vela-plugins`
- `vela-memory-honcho`

---

## Immediate file split recommendation

## `vela-runtime`
Should stop being the home for all bootstrap/state logic over time.

Move out first:
- config loading → `vela-config`
- session persistence semantics → `vela-session`
- memory file logic → `vela-memory`

Keep in runtime:
- orchestration glue
- session start wiring
- turn coordination

---

## Data flow for memory and learning

### Session start
```text
apps/vela
  -> vela-runtime
    -> vela-config
    -> vela-session
    -> vela-memory (load MEMORY.md + USER.md snapshot)
    -> vela-state (session lookup / create)
    -> vela-models (build model request)
```

### During turn
```text
vela-runtime
  -> vela-tools
  -> vela-state (messages/events)
  -> vela-models
```

### After turn review
```text
vela-runtime
  -> vela-review
    -> durable fact? -> vela-memory
    -> reusable procedure? -> vela-skills
    -> all writes logged in vela-state
```

### Later with Honcho
```text
vela-review or vela-runtime
  -> vela-memory-honcho
    -> ingest selected events/memory candidates
```

---

## Most important architectural separations
1. **Built-in memory != session search**
   - `vela-memory` owns MEMORY.md / USER.md
   - `vela-state` owns FTS5 session history

2. **Memory != skills**
   - facts/preferences in `vela-memory`
   - procedures in `vela-skills`

3. **Runtime != persistence**
   - runtime orchestrates
   - state owns DB semantics

4. **Models != memory**
   - models infer
   - memory/context policy lives elsewhere

5. **Honcho != canonical memory**
   - Honcho enriches
   - Vela’s built-in memory remains canonical at first

---

## Suggested next implementation step
After this layout, the highest-leverage coding step is:

### Build these three crates next
1. `vela-config`
2. `vela-memory`
3. strengthen `vela-state`

That unlocks:
- proper memory files
- prompt snapshot injection
- stable config/reload foundation
- clean future Honcho seam

---

## Short version
If you want the shortest path to a believable Vela kernel:
- **`vela-state`** = durable DB + FTS
- **`vela-memory`** = MEMORY.md / USER.md
- **`vela-skills`** = procedural memory
- **`vela-runtime`** = orchestration
- **`vela-models`** = backend abstraction
- **`vela-memory-honcho`** = later augmentation layer
