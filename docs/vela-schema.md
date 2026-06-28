# Vela Durable Schema

## Purpose
This document defines the first durable schema for Vela's Rust-first agentic OS core.

It follows the current strategy:
- Hermes-faithful memory/learning behavior first
- SQLite as the canonical runtime store
- small built-in memory files for always-on context
- skills as procedural memory
- approval staging before uncontrolled self-modification
- Honcho later as an augmentation layer

This is a **design schema**, not yet migration SQL.

---

## Storage surfaces
Vela should persist across four main surfaces:

1. **SQLite runtime store**
   - `~/.vela/state.db`
2. **Built-in memory files**
   - `~/.vela/memories/MEMORY.md`
   - `~/.vela/memories/USER.md`
3. **Procedural memory files**
   - `~/.vela/skills/**`
4. **Review candidate queue**
   - `~/.vela/reviews/candidates/*.json`
5. **Compatibility/export snapshots**
   - `~/.vela/sessions/session_<id>.json`

---

## Canonical ownership rules
### Canonical store for runtime state
- `state.db`

### Canonical store for always-on memory
- `MEMORY.md`
- `USER.md`

### Canonical store for procedural memory
- `skills/`

### Candidate-review surface
- `reviews/candidates/`
- this is the pre-approval intake queue for background learning suggestions
- transcript/event analyzers can write here first
- promoted candidates flow into `pending/memory/` or `pending/skills/`

### Compatibility/debug/export surface
- `sessions/*.json`

### Important rule
The JSON session snapshots are **not** the source of truth.
They exist for compatibility, export, migration, and tooling support.

---

## Filesystem layout
```text
~/.vela/
  state.db
  config.yaml
  gateway.json                 # legacy/compat if needed
  active_profile
  sessions/
    session_<id>.json
  memories/
    MEMORY.md
    USER.md
  skills/
    <skill>/SKILL.md
    <skill>/...
  pending/
    memory/
      <id>.json
    skills/
      <id>.json
  reviews/
    candidates/
      <id>.json
```

---

# 1. SQLite schema

## SQLite pragmas and durability
At open time, Vela should attempt:
- `PRAGMA foreign_keys = ON`
- `PRAGMA journal_mode = WAL`

If WAL is unsupported:
- fall back to `journal_mode = DELETE`
- record the effective journal mode in state metadata
- surface DB init problems in diagnostics instead of failing silently

---

## 1.1 `state_meta`
Small key/value store for bootstrap and runtime metadata.

### Purpose
- schema version
- bootstrap counters
- effective journal mode
- snapshot compatibility markers
- most recent DB init error
- migration markers

### Shape
```sql
CREATE TABLE state_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### Example keys
- `schema_version`
- `bootstrap_runs`
- `journal_mode`
- `snapshot_pattern`
- `last_db_init_error`
- `active_profile`

---

## 1.2 `profiles`
Profiles represent long-lived environment/account identities.

### Purpose
- support `--profile`
- separate long-lived contexts
- attach sessions/memory/search to a profile

### Shape
```sql
CREATE TABLE profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  vela_home TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 0
);
```

### Notes
- v0 can start with only one active profile
- the table still helps avoid smearing profile logic across config/state code later

---

## 1.3 `sessions`
Canonical session metadata.

### Purpose
- create/resume/continue
- session listing
- per-session model/provider identity
- CLI vs gateway source tags

### Shape
```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  profile_id TEXT,
  title TEXT NOT NULL,
  source TEXT NOT NULL,
  command_name TEXT NOT NULL,
  interaction_mode TEXT NOT NULL,
  model TEXT,
  provider TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  archived_at INTEGER,
  FOREIGN KEY(profile_id) REFERENCES profiles(id)
);
```

### Fields
- `source`: `cli`, `telegram`, `discord`, `gateway`, etc.
- `interaction_mode`: `interactive`, `single-turn`, later maybe others
- `archived_at`: soft archival rather than hard delete

---

## 1.4 `messages`
Canonical transcript store.

### Purpose
- reconstruct sessions
- power session search
- inspect prior turns
- support future replay/debugging

### Shape
```sql
CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  tool_name TEXT,
  tool_call_id TEXT,
  is_error INTEGER NOT NULL DEFAULT 0,
  metadata_json TEXT,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);
```

### Notes
- `role`: `system`, `user`, `assistant`, `tool`, `tool_result`, etc. as needed
- `metadata_json`: escape hatch for evolving transcript details without churn in v1

---

## 1.5 `session_events`
Higher-level runtime event log.

### Purpose
Separate event semantics from transcript text.

Examples:
- session created
- session resumed
- tool approval requested
- reload performed
- review suggested memory write
- memory_signal emitted from transcript/correction analysis
- skill_signal emitted from captured successful procedures
- skill patch staged

### Shape
```sql
CREATE TABLE session_events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);
```

### Why keep this separate from `messages`
Because not every state change is a message, and later scheduling/gateway/review flows will be easier to reason about if event log semantics are explicit.

---

## 1.6 `message_fts`
FTS5 index for session search.

### Purpose
- search conversation history
- search tool outputs and user/assistant text
- support session_search-like tooling

### Shape
```sql
CREATE VIRTUAL TABLE message_fts USING fts5(
  message_id UNINDEXED,
  session_id UNINDEXED,
  content
);
```

### Sync rule
Whenever `messages` changes:
- insert/update/delete corresponding `message_fts` row

### v1 rule
FTS index only transcript content.
Do not overcomplicate it by indexing every JSON field first.

---

## 1.7 `pending_memory_writes`
Approval staging for built-in memory changes.

### Purpose
When memory approval is enabled, writes should stage here before landing in `MEMORY.md` or `USER.md`.

### Shape
```sql
CREATE TABLE pending_memory_writes (
  id TEXT PRIMARY KEY,
  profile_id TEXT,
  target TEXT NOT NULL,
  action TEXT NOT NULL,
  old_text TEXT,
  new_text TEXT,
  gist TEXT,
  origin_session_id TEXT,
  created_at INTEGER NOT NULL,
  status TEXT NOT NULL,
  reviewer_note TEXT,
  FOREIGN KEY(profile_id) REFERENCES profiles(id),
  FOREIGN KEY(origin_session_id) REFERENCES sessions(id)
);
```

### Status values
- `pending`
- `approved`
- `rejected`
- `applied`

---

## 1.8 `pending_skill_writes`
Approval staging for procedural memory writes.

### Purpose
When skill approval is enabled, writes stage here before changing files in `skills/`.

### Shape
```sql
CREATE TABLE pending_skill_writes (
  id TEXT PRIMARY KEY,
  profile_id TEXT,
  skill_name TEXT NOT NULL,
  action TEXT NOT NULL,
  relative_path TEXT,
  gist TEXT,
  patch_text TEXT,
  origin_session_id TEXT,
  created_at INTEGER NOT NULL,
  status TEXT NOT NULL,
  reviewer_note TEXT,
  FOREIGN KEY(profile_id) REFERENCES profiles(id),
  FOREIGN KEY(origin_session_id) REFERENCES sessions(id)
);
```

### Note
The canonical skill content still lives in files. This table stores the staged proposal, not the final source of truth.

---

## 1.9 `review_outcomes`
Optional but useful structured output of the learning loop.

### Purpose
Record what the background review decided.

### Shape
```sql
CREATE TABLE review_outcomes (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  outcome_type TEXT NOT NULL,
  target TEXT,
  summary TEXT NOT NULL,
  payload_json TEXT,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);
```

### Example `outcome_type`
- `memory_candidate`
- `skill_candidate`
- `memory_write`
- `skill_write`
- `no_action`

---

## 1.10 `external_memory_providers`
Future registration/status for Honcho and friends.

### Purpose
Track external provider activation without making them canonical.

### Shape
```sql
CREATE TABLE external_memory_providers (
  id TEXT PRIMARY KEY,
  provider_name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  config_json TEXT,
  last_status TEXT,
  last_sync_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### v1 note
This can exist before Honcho is implemented, but it does not need to block the first core memory slice.

---

# 2. Built-in memory file schema

## 2.1 `MEMORY.md`
### Purpose
Vela's compact personal notes.

### Content type
Curated line- or paragraph-level durable facts.

### Storage format
Human-readable Markdown/plain text list with entry separators.

### Rules
- bounded to ~2200 chars
- no silent compaction
- persisted immediately on successful write
- injected into session prompt only at session start

### Example
```text
This machine runs Ubuntu 22.04 and has Docker installed.§The user's main Rust workspace is ~/code/vela.§When editing plans, prefer incremental changes over clean-slate rewrites.
```

---

## 2.2 `USER.md`
### Purpose
User profile memory.

### Content type
Preferences and expectations.

### Rules
- bounded to ~1375 chars
- same write semantics as `MEMORY.md`
- also frozen per session start

### Example
```text
User prefers concise responses.§User likes Rust-first architectural decisions.§User wants Hermes-faithful behavior before experimentation.
```

---

## 2.3 Rendered prompt snapshot shape
At session start, the runtime should render a memory block like:

```text
MEMORY (personal notes) [67% — 1474/2200 chars]
entry§entry§entry

USER PROFILE [54% — 745/1375 chars]
entry§entry§entry
```

### Important rule
This is a **frozen snapshot** per session start.
The runtime should not keep mutating the prompt mid-session after writes.

---

# 3. Skills file schema

## Root
```text
~/.vela/skills/
```

## Canonical unit
A skill directory containing:
- `SKILL.md`
- optional referenced files/assets

### Example
```text
~/.vela/skills/deploy-staging/
  SKILL.md
  checklist.md
```

## `SKILL.md`
### Purpose
Procedural memory.

### Schema shape
Follow a structured markdown contract with at least:
- name
- short description
- when to use
- steps/procedure
- references or linked files if needed

### v1 rule
Keep it file-based and human-readable.
Do not move procedural memory into SQLite first.

---

# 4. Session snapshot JSON schema

## Path
```text
~/.vela/sessions/session_<id>.json
```

## Purpose
Compatibility/export/debug snapshot.

## Suggested shape
```json
{
  "session_id": "session-123",
  "title": "chat-1719550000",
  "profile": "default",
  "source": "cli",
  "model": "...",
  "provider": "...",
  "created_at": 1719550000,
  "updated_at": 1719551000,
  "messages": [
    {
      "id": "msg-1",
      "role": "user",
      "content": "...",
      "created_at": 1719550001
    }
  ]
}
```

### Rule
Generate these from canonical DB state.
Do not treat them as the primary runtime store.

---

# 5. Write paths

## 5.1 Memory write path
```text
runtime/review/tool call
  -> vela-memory validates target + limit
  -> if approval off: update MEMORY.md or USER.md
  -> if approval on: stage into pending_memory_writes (+ pending file if desired)
  -> log event in session_events
```

## 5.2 Skill write path
```text
runtime/review/tool call
  -> vela-skills validates skill write
  -> if approval off: write files under skills/
  -> if approval on: stage into pending_skill_writes (+ pending file)
  -> log event in session_events
```

## 5.3 Transcript write path
```text
runtime turn loop
  -> messages row
  -> message_fts row
  -> optional session snapshot refresh
```

---

# 6. Read paths

## 6.1 Session start
```text
load config/profile
  -> load MEMORY.md and USER.md
  -> render frozen memory snapshot
  -> create/resume session from state.db
```

## 6.2 Session search
```text
search query
  -> FTS5 over message_fts
  -> resolve message/session metadata
  -> allow scroll/browse
```

## 6.3 Skills retrieval
```text
skills_list -> summary only
skill_view(name) -> full SKILL.md
skill_view(name, path) -> targeted file
```

---

# 7. Honcho integration seam

## Role
Honcho should be an **augmenting external provider**, not the canonical store.

## What remains canonical
- `MEMORY.md`
- `USER.md`
- `state.db`
- `skills/`

## Honcho can add later
- deeper user modeling
- semantic retrieval
- graph-like memory associations
- cross-session enrichment

## Integration shape
- `external_memory_providers` tracks provider enablement/status
- runtime/review may emit selected events to Honcho
- Honcho search results can be queried alongside built-in memory/search

### Important rule
Vela should remain fully functional if Honcho is absent or down.

---

# 8. Migration priorities

## First migration set
1. `state_meta`
2. `profiles`
3. `sessions`
4. `messages`
5. `message_fts`

## Second migration set
6. `session_events`
7. `pending_memory_writes`
8. `pending_skill_writes`
9. `review_outcomes`

## Third migration set
10. `external_memory_providers`

---

# 9. Explicit non-goals for v1
Do not put into v1 schema first:
- vector embeddings tables
- knowledge graph tables
- MoE/model-weight memory nonsense
- giant generalized document stores
- over-normalized skill internals
- external provider dependence

v1 should be simple, durable, inspectable, and Hermes-faithful.

---

# 10. Short summary
If crate layout answers **where the code lives**, this schema answers **what survives across time**.

The key split is:
- **`state.db`** = runtime truth and search
- **`MEMORY.md` / `USER.md`** = tiny always-on memory
- **`skills/`** = procedural memory
- **`pending_*`** = approval staging
- **Honcho later** = optional augmentation, never replacement
