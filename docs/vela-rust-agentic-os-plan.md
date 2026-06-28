# Vela Rust Agentic OS Plan

## Goal
Build **Vela** as a **Rust-first agentic OS** with a reloadable extension layer and a future-facing model experimentation surface.

This is **not** a model-first plan.
This is **not** a big-bang “rewrite all of Hermes before Vela lives” plan.

The goal is to:
- own the core runtime in Rust
- keep experimentation fast
- grow toward Hermes-class capabilities
- leave room for future model/backend experiments like ternary, sparse, and MoE

---

## Strategic position
### What Vela is
Vela should be:
- a persistent agent runtime
- a scheduler and event-driven OS for agents
- a memory-bearing system with long-lived identity
- a plugin/configurable experimentation platform
- a host for multiple model backends and future architecture experiments

### What Vela is not
Vela is not, at least initially:
- a custom LLM from scratch
- a MoE training project
- a feature-complete day-one Rust replacement for everything Hermes does
- a narrow coding harness only

---

## Core recommendation
Use a **progressive replacement** strategy, not a big-bang rewrite.

### Why
A full immediate Rust rewrite of every Hermes-class subsystem would front-load months of infrastructure work before Vela is even alive.

That creates three major risks:
1. lots of plumbing, little usable system
2. ownership of every provider/platform edge case before the core vision is proven
3. slower experimentation than the user actually wants

### Instead
Build a **small but real Vela kernel** first, then absorb more territory over time.

The end state can still be “Hermes-class Vela in Rust.”
The difference is that the path stays livable, testable, and experiment-friendly.

---

## Design principles
1. **Rust owns the kernel**
   - sessions
   - runtime state
   - scheduler
   - persistence
   - policy/permissions
   - event bus
   - provider/backend abstraction
   - plugin lifecycle

2. **Extensions stay reloadable**
   - tools
   - skills
   - workflows
   - memory policies
   - planners/verifiers
   - surface adapters where possible

3. **Agentic OS first**
   The runtime, memory, orchestration, scheduling, identity, and plugin system matter before custom model architecture.

4. **Vertical slices over giant subsystem rewrites**
   Each milestone should produce a more alive Vela, not only more scaffolding.

5. **Compatibility research is reference input**
   Prior Hermes/Vela behavior research remains useful, but strict surface matching is not the only organizing principle.

6. **Backend experimentation is a first-class future goal**
   The core must make it easy later to swap in:
   - different providers
   - local model runtimes
   - fine-tunes
   - ternary experiments
   - sparse/MoE backends

---

## Target architecture

## Layer 1 — Rust kernel
Own in Rust from the beginning:
- session manager
- turn/runtime state machine
- event log / event bus
- persistence layer
- task scheduler
- config loader
- hot reload coordinator
- provider/backend abstraction
- policy and approval engine
- tool runtime boundary
- plugin registry

## Layer 2 — Reloadable extension runtime
Support rapid experimentation without recompiling the kernel whenever possible:
- tools
- skills
- workflows
- memory rules
- planner modules
- reflection/verifier modules
- surface integrations that can be adapterized

## Layer 3 — Vela cognition/services
Higher-level built-ins that make Vela feel like an agentic OS:
- memory service
- retrieval service
- planner
- reflection loop
- verifier
- self-revision hooks
- long-lived identity/profile system

## Layer 4 — Model/backend lab
Swappable backends and future experiments:
- OpenAI-compatible APIs
- local inference runtimes
- fine-tuned coding models
- ternary model experiments
- sparse routing and future MoE experiments

---

## Relationship to Hermes
Hermes is the **capability benchmark**, not a requirement for day-one feature completeness.

That means:
- preserve what is strategically important
- study how Hermes solves real OS problems
- avoid reimplementing all of Hermes before the Vela kernel is usable

### Desired end state
A Rust-native Vela that can eventually offer Hermes-class capabilities while remaining more hackable, reloadable, and experiment-friendly for custom research.

---

## Roadmap

## Phase 0 — Architecture framing and inventory
### Scope
- keep existing compatibility notes
- identify must-have Hermes/Vela capabilities
- separate kernel concerns from extension concerns
- define what “alive” means for Vela v0

### Deliverables
- kernel boundary document
- extension boundary document
- capability inventory grouped by phase, not only by old compatibility buckets
- clear statement of non-goals for v0

### Exit gate
- the first kernel slice is small enough to build
- the project is no longer framed as “rewrite everything first”

---

## Phase 1 — Minimal Rust kernel that is actually alive
### Scope
Build the smallest believable Vela core:
- `vela` binary
- config loading
- state directory bootstrap
- one provider/backend path
- one session loop
- one tool execution path
- persistence for sessions/events
- basic reload command or reload trigger

### Checklist
- [ ] define kernel crate boundaries
- [ ] boot the `vela` binary end-to-end
- [ ] load config and state paths
- [ ] implement one session lifecycle path
- [ ] implement one provider/backend adapter
- [ ] implement one tool invocation path
- [ ] persist sessions/events
- [ ] implement first reload mechanism

### Exit gate
- Vela can run a real session in Rust
- Vela can persist and resume that session state
- Vela can reload config or plugin state without pretending the whole OS is finished

---

## Phase 2 — Reloadable extension system
### Scope
Create the Neovim-like experimentation layer the user wants.

This is where Vela starts to feel configurable rather than hardcoded.

### Capabilities
- plugin manifest/registry
- extension lifecycle
- tool modules
- skill/workflow modules
- config-driven enable/disable
- reload without rebuilding the full world where possible

### Checklist
- [ ] define plugin manifest format
- [ ] define extension lifecycle hooks
- [ ] load tool extensions from config
- [ ] load skill/workflow extensions from config
- [ ] implement enable/disable flags
- [ ] implement safe reload path
- [ ] document plugin boundaries vs kernel boundaries

### Exit gate
- Vela can add/remove selected capabilities through config and reload
- experimentation speed improves materially over kernel-only builds

---

## Phase 3 — Core agentic OS services
### Scope
Add the services that move Vela from “agent runtime” to “agentic OS.”

### Capabilities
- scheduler/background tasks
- memory service
- retrieval/search
- approval/policy model
- planner/reflection/verifier hooks
- durable identity/profile layer

### Checklist
- [ ] add scheduler core
- [ ] add memory write/read pipeline
- [ ] add retrieval/search interface
- [ ] add approval and policy engine
- [ ] add planner hooks
- [ ] add reflection/verifier hooks
- [ ] add profile/identity persistence

### Exit gate
- Vela behaves like a persistent agent system, not just a CLI wrapper

---

## Phase 4 — Hermes-class capability expansion
### Scope
Grow toward the broader capabilities the user wants from Hermes.

### Candidate slices
- gateway/daemon
- attach/resume flows
- messaging/platform surfaces
- subagents/delegation
- MCP integration
- richer automation and delivery semantics

### Working rule
Implement these as bounded vertical slices rather than “port the whole subsystem because Hermes has it.”

### Checklist
- [ ] pick first gateway path
- [ ] implement attach/resume for that path
- [ ] implement one real delivery surface
- [ ] add subagent/delegation support
- [ ] add MCP bridge/support
- [ ] add automated job delivery path

### Exit gate
- at least one real Hermes-class flow works end-to-end in Vela
- each added surface rests on the existing kernel rather than bypassing it

---

## Phase 5 — Backend and model lab
### Scope
Only after the OS/runtime is credible should Vela expand into deeper model experimentation.

### Capabilities
- backend switching
- local inference integrations
- evaluation harness for backend comparison
- adapter/fine-tune support
- ternary experiments
- sparse routing experiments
- future MoE experiments

### Checklist
- [ ] formalize backend API
- [ ] support backend switching from config
- [ ] add local inference backend(s)
- [ ] add backend benchmark/eval harness
- [ ] add first architecture experiment slot (e.g. ternary)
- [ ] document criteria for deeper model-core work

### Exit gate
- backend/model experimentation becomes a routine capability of Vela rather than a separate moonshot

---

## What should be preserved from current compatibility work
The existing compatibility and inventory work is still valuable for:
- naming and surface discovery
- persistence/state expectations
- config and env semantics
- session/gateway behavior reference
- regression checks while replacing features incrementally

### But it should no longer imply
- “all surface matching before architecture”
- “no redesign until total reproduction is done”
- “rewrite every subsystem before a real Vela exists”

Use compatibility research as **input**, not as a prison.

---

## Verification strategy
Use all of:
- kernel smoke tests
- real end-to-end session tests
- state continuity tests
- reload tests
- plugin enable/disable tests
- side-by-side reference checks against existing systems where helpful
- backend comparison tests once model-lab work begins

### Priority order
1. does Vela stay alive and coherent?
2. does state persist and recover correctly?
3. does reload/config/plugin behavior work?
4. do Hermes-derived capabilities land without destabilizing the kernel?
5. do backend experiments plug in cleanly?

---

## Non-goals for early phases
Do not front-load:
- custom MoE training
- custom ternary model training
- full platform surface reproduction
- every Hermes integration at once
- a giant plugin API before the kernel has one good slice
- speculative optimization before runtime shape is proven

---

## Decision summary
### Recommended direction
Build Vela as:
- a **Rust agentic OS kernel**
- with a **reloadable extension system**
- that **incrementally grows toward Hermes-class capabilities**
- while preserving a path for **future custom model/backend experiments**

### Explicit non-recommendation
Do **not** make the plan “rewrite all of Hermes in Rust before Vela is usable.”

That path maximizes infrastructure work and minimizes early Vela life.

### End-state intention
The long-term destination can still be a largely Rust-native, Hermes-class, deeply hackable Vela OS.

The recommendation is about the **path**, not the ambition.
