# Vela Rust Parity Plan

## Goal
Rewrite `NousResearch/vela-agent` in Rust with **behavior preservation first** and architectural cleanup later.

## Operating rule
When parity and elegance conflict, choose parity.

This rewrite should preserve, before redesign:
- CLI behavior
- config and env behavior
- session lifecycle
- gateway behavior
- scheduler behavior
- tool behavior
- provider behavior
- memory and skills behavior
- on-disk state semantics or a tested migration path

---

## Milestone workflow
Each milestone is gated by:
1. behavior contract
2. implementation boundary
3. parity proof
4. exit gate

Do not move to the next milestone until the current one passes.

---

## Milestone 0 — Compatibility inventory
### Scope
- CLI commands and flags
- config files, env vars, defaults, precedence
- state directories and persistence behavior
- session lifecycle behavior
- gateway/platform semantics
- scheduler behavior
- tool contracts
- provider contracts
- memory/skills behavior

### Checklist
- [ ] Enumerate CLI commands
- [ ] Enumerate config files, env vars, and defaults
- [ ] Enumerate on-disk state locations
- [ ] Enumerate session lifecycle behaviors
- [ ] Enumerate gateway/platform behaviors
- [ ] Enumerate scheduler behaviors
- [ ] Enumerate tool contracts
- [ ] Enumerate provider contracts
- [ ] Enumerate memory/skills behaviors
- [ ] Freeze compatibility target docs

### Exit gate
- All public Vela surfaces are enumerated.
- No rewrite work begins without a target behavior list.

---

## Milestone 1 — Rust bootstrap shell
### Scope
- Rust workspace
- `vela` binary entrypoint
- config loading
- env resolution
- logging/bootstrap
- state-dir bootstrap
- command parsing shell

### Checklist
- [ ] Create Rust workspace
- [ ] Create `vela` binary
- [ ] Implement config loading parity
- [ ] Implement env resolution parity
- [ ] Implement logging/bootstrap parity
- [ ] Implement state-dir bootstrap parity
- [ ] Expose same top-level commands
- [ ] Verify startup behavior matches Vela

### Exit gate
- Basic startup behavior matches Vela.
- Config resolution is parity-checked.
- Same command names are exposed.

---

## Milestone 2 — State and persistence parity
### Scope
- sessions
- transcripts/history
- approvals
- pairing/auth state
- scheduler state
- migration/import path if formats differ

### Checklist
- [ ] Define state schema
- [ ] Implement SQLite persistence
- [ ] Implement session persistence
- [ ] Implement transcript/history persistence
- [ ] Implement approval persistence
- [ ] Implement pairing/auth persistence
- [ ] Implement scheduler persistence
- [ ] Implement migration/import path
- [ ] Verify restart continuity parity

### Exit gate
- Rust can preserve or migrate Vela state.
- Restart behavior is acceptably equivalent.

---

## Milestone 3 — Runtime parity
### Scope
- turn lifecycle
- interruption
- cancellation
- context assembly
- runtime state transitions

### Checklist
- [ ] Implement turn lifecycle
- [ ] Implement interruption semantics
- [ ] Implement cancellation semantics
- [ ] Implement context assembly path
- [ ] Implement runtime state transitions
- [ ] Verify one complete turn matches Vela
- [ ] Verify interrupt/resume parity

### Exit gate
- One complete turn behaves like Vela.
- Interruption and resume semantics match.

---

## Milestone 4 — Tool core parity
### Scope
Start with:
- shell/process
- file read/write/edit
- approvals
- project/local tools

Then extend to:
- MCP bridge
- browser/computer-use shell
- messaging helpers

### Checklist
- [ ] Implement tool registry
- [ ] Implement shell/process tools
- [ ] Implement file read/write/edit tools
- [ ] Implement approval gating
- [ ] Implement project/local tools
- [ ] Verify tool args/results parity
- [ ] Verify failure/approval parity

### Exit gate
- Core tools behave the same from the user view.
- Approval and safety behavior match.

---

## Milestone 5 — Gateway parity
### Scope
- gateway daemon
- pairing/auth flow
- attach/resume flow
- inbound/outbound routing
- one platform adapter first

### Checklist
- [ ] Implement gateway daemon
- [ ] Implement pairing/auth flow
- [ ] Implement attach/resume flow
- [ ] Implement inbound/outbound routing
- [ ] Implement one platform adapter
- [ ] Verify delivery semantics parity
- [ ] Verify session continuity parity

### Exit gate
- One real gateway path works end-to-end.
- Attach/resume/delivery behavior matches Vela.

---

## Milestone 6 — Scheduler parity
### Scope
- cron spec parsing
- recurrence execution
- persistence across restarts
- delivery integration

### Checklist
- [ ] Implement cron spec parsing
- [ ] Implement recurrence execution
- [ ] Implement persistence across restarts
- [ ] Implement delivery integration
- [ ] Verify schedule timing parity
- [ ] Verify job lifecycle parity

### Exit gate
- Scheduled tasks fire with equivalent semantics.
- Restart persistence matches Vela expectations.

---

## Milestone 7 — Provider parity
### Scope
Recommended order:
1. OpenAI-compatible
2. Anthropic-compatible
3. OpenRouter/local
4. secondary providers

### Checklist
- [ ] Implement provider abstraction
- [ ] Port OpenAI-compatible provider
- [ ] Port Anthropic-compatible provider
- [ ] Port OpenRouter/local provider
- [ ] Verify streaming parity
- [ ] Verify error behavior parity
- [ ] Verify model switching parity

### Exit gate
- Model selection, streaming, and failures are parity-checked.

---

## Milestone 8 — Memory parity
### Scope
- persistent recall
- retrieval/search
- memory write triggers
- memory read boundaries

### Checklist
- [ ] Implement persistent recall
- [ ] Implement retrieval/search
- [ ] Implement memory write triggers
- [ ] Implement memory read boundaries
- [ ] Verify user-visible memory behavior parity

### Exit gate
- Memory behavior matches Vela from the user view.

---

## Milestone 9 — Skills parity
### Scope
- skill discovery
- skill loading
- skill invocation
- skill indexing/cache
- failure behavior

### Checklist
- [ ] Implement skill discovery
- [ ] Implement skill loading
- [ ] Implement skill invocation
- [ ] Implement skill indexing/cache
- [ ] Verify failure behavior parity

### Exit gate
- Skills preserve current workflows with equivalent behavior.

---

## Milestone 10 — Python de-risking
### Scope
- remove Python from already-ported core paths
- keep only remaining adapters on Python if needed
- make Rust the primary execution path

### Checklist
- [ ] Remove Python from ported paths
- [ ] Keep only remaining adapters on Python if needed
- [ ] Verify Rust is the primary execution path

### Exit gate
- Rust owns the critical Vela path.

---

## Milestone 11 — Surface strategy
### Scope
- web integration approach
- TUI integration approach
- desktop integration approach
- final cutover path

### Checklist
- [ ] Decide web integration approach
- [ ] Decide TUI integration approach
- [ ] Decide desktop integration approach
- [ ] Document final cutover path

### Exit gate
- Surface plan is explicit.
- No accidental frontend rewrite drift remains.

---

## GitHub issue structure
### Suggested trackers
1. Vela Rust parity rewrite
2. Bootstrap shell parity
3. State and persistence parity
4. Runtime parity
5. Tool core parity
6. Gateway parity
7. Scheduler parity
8. Provider parity
9. Memory parity
10. Skills parity
11. Surface strategy
12. Parity test harness

### Suggested child issue shapes
- one milestone tracker per major subsystem
- one child issue per bounded parity slice
- one parity-proof issue if verification scope is large enough to need separate tracking

---

## Parity test strategy
Use all of:
- golden tests
- fixture replay
- side-by-side Vela vs Rust comparisons
- state transition assertions

### Milestone-specific proof examples
#### Bootstrap shell
- compare CLI help output shape
- compare config resolution under identical env
- compare startup side effects

#### State and persistence
- create state in Python Vela
- load or migrate it into Rust
- verify continuity, transcripts, and approvals

#### Runtime
- replay the same prompt/tool scenario
- compare turn state transitions, interruption behavior, and final output

#### Tool core
- run identical tool invocations
- compare args accepted, outputs returned, approvals, and failures

#### Gateway
- replay attach/resume/pair flows
- compare event ordering, delivery semantics, and blocked/failure handling

#### Scheduler
- compare next-fire calculations, restart persistence, and delivery behavior

#### Providers
- compare request shapes, stream chunks, model switching, and error behavior

#### Memory
- compare recall queries and user-visible retrieval output

#### Skills
- compare loading, invocation, indexing, and failure behavior

---

## Pass criteria
A milestone passes only when:
- visible behavior matches
- persisted state behavior matches
- failure behavior is acceptably equivalent
- parity tests are green

## Fail criteria
A milestone fails when:
- command/config/state semantics drift
- user-visible wording changes meaningfully
- state continuity breaks
- gateway/session ordering changes unexpectedly

---

## Non-goals during parity
Do not, during parity:
- rename concepts
- redesign workflows
- simplify behavior users rely on
- merge subsystems just because Rust can
- optimize before parity is proven

Architecture cleanup belongs after parity.