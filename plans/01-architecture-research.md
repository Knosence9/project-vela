# Project Vela Architecture Research Notes

## Research question

What should Vela build itself, and what should she borrow from the Rust agent ecosystem?

## Early conclusion

Vela should be a Rust-native agent OS with her own kernel concepts, not merely a wrapper around a single existing framework. Existing frameworks should be studied and selectively used where they match Vela’s needs.

The most important borrowed patterns are likely:

- Tokio as the async runtime foundation.
- Structured concurrency for sub-agent/task lifecycles.
- Typed tool schemas and compile-time validation.
- Actor/supervision patterns for multi-agent coordination.
- WASM sandboxing or another isolation layer for risky tools.
- SQLite/event-log persistence for durable memory and replay.
- Deterministic validators for plans, workflows, skills, and issue translation.

## Rust ecosystem candidates

### Runtime and orchestration

- `tokio`: async runtime, channels, task scheduling, cancellation patterns.
- `tokio-util`: `CancellationToken` and utilities for cooperative cancellation.
- `futures`: stream/future combinators.
- `ractor` or `actix`: actor model and supervision semantics.
- `tracing` + OpenTelemetry: structured observability.

### Agent frameworks to study

- **Rig**: trait-first LLM/tool abstractions; useful as a reference for provider/tool boundaries.
- **AutoAgents**: Rust multi-agent framework with ReAct/basic executors, typed tools, memory, local/cloud providers, WASM tool execution, OpenTelemetry, MCP examples, and design-pattern examples.
- **ADK-Rust**: modular agent/workflow framework with LLM agents, sequential/parallel/loop/graph/router workflows, sessions, artifacts, telemetry, and A2A support.
- **ai-agents crate**: active crate to inspect for design ideas, but needs source/API review before depending on it.

### Tooling and integration

- `clap`: CLI surface.
- `ratatui`/`crossterm`: possible TUI surface.
- `serde`/`schemars`: typed config, JSON Schema, tool schemas.
- `sqlx` or `rusqlite`: SQLite persistence.
- `tantivy` or SQLite FTS: local search.
- `reqwest`: HTTP clients.
- MCP/A2A crates: inspect maturity before adopting.
- `wasmtime`: WASM sandboxing for tools/extensions.

### Local model / Rust ML candidates

- `candle`: Rust-native ML inference ecosystem.
- `mistral.rs`: local inference backend candidate.
- `llama.cpp` bindings: practical local inference path.
- Hugging Face datasets/libraries: useful first as Rust-code assistance material — retrieval references, idiom examples, coding evaluations, lint/checklist generation, and workflow guidance that helps Vela write better Rust. Training/fine-tuning is optional later work, not the first assumption.

## Rust learning/code datasets mentioned

Primary purpose: these datasets should help Vela write better idiomatic Rust. The first use should be as reference/evaluation material for coding assistance, not as an automatic model-training plan.

Detailed dataset notes live in [`02-rust-dataset-understanding.md`](./02-rust-dataset-understanding.md). The most important conclusion is that Vela must use dataset quality gates: compile checks, semantic alignment checks, license/provenance tracking, and stricter review for unsafe/FFI/concurrency examples.

### `introspector/rust-analyser`

Initial web evidence:

- Hugging Face dataset.
- Focus: rust-analyzer semantic analysis.
- Size: 100K–1M records.
- Formats: parquet.
- License shown as AGPL-3.0.

Implication:

- Useful for code-understanding research and rust-analyzer-style semantic insight.
- AGPL licensing may be risky for direct training/fine-tuning unless reviewed carefully.
- Better first use: offline analysis, retrieval-assisted Rust guidance, semantic evaluation ideas, and coding-review checklists rather than baked-in model training.

### `Fortytwo-Network/Strandset-Rust-v1`

Initial web evidence:

- Hugging Face dataset.
- Large Rust code modeling dataset.
- Search result reports 191,008 verified examples across 15 task categories.
- License shown as Apache-2.0.

Implication:

- Stronger candidate for Rust-code instruction/eval experiments and idiomatic Rust example retrieval.
- Should inspect schema, examples, task categories, and quality before use.
- Could feed a “Rust coding mentor” workflow that checks Vela’s Rust patches against examples, compiler expectations, ownership/lifetime patterns, and common crate idioms.

### `Convence/Rust-Coder` / `gubernac/Rust-Coder`

Verified dataset paths:

- `Convence/Rust-Coder` exists on Hugging Face.
- `gubernac/Rust-Coder` also exists and is marked as duplicated from `Convence/Rust-Coder`.
- `Convene/Rust-Coder` returned unauthorized/not available during direct fetch and should not be treated as the canonical path.

Initial metadata:

- Tasks: text generation and question answering.
- Modalities: text.
- Format: parquet.
- Language: English.
- Size: 10K–100K.
- Tags: Rust, programming, education, code generation.
- License: Apache-2.0.

Implication:

- Treat `Convence/Rust-Coder` as the canonical dataset candidate and `gubernac/Rust-Coder` as a duplicate/mirror unless deeper inspection proves otherwise.
- This looks safer to evaluate than AGPL-licensed sources, but schema/content quality still need to be inspected before use.
- First use should be coding assistance: examples, explanations, Rust idiom retrieval, benchmark prompts, and deterministic checks that help Vela produce better Rust code.

## Vela-specific design implications

### 1. Vela needs a kernel, not just agents

The kernel should own:

- identity/persona policy
- task/session lifecycle
- memory/event log
- tool registry
- skill/workflow registry
- extension registry
- permissions and safety policy
- deterministic validation
- self-improvement loop
- observability and replay

### 2. Skills/workflows/extensions need shared dependencies

Avoid copy-pasted skill logic. Introduce shared subprocess modules for deterministic work:

- plan parsing
- issue translation
- checklist reconciliation
- artifact validation
- review comment extraction
- test output classification
- memory lesson extraction
- tool schema generation

### 3. Self-improvement must be explicit and auditable

Vela should not silently rewrite herself.

Self-improvement pipeline:

1. Detect failure, friction, repeated pattern, or missing tool.
2. Propose improvement.
3. Classify target: memory, skill, workflow, extension, tool, test, docs.
4. Generate patch or new artifact.
5. Run deterministic validation.
6. Ask for approval when behavior changes matter.
7. Record rationale and outcome.

### 4. The model should not do deterministic bookkeeping

Examples that should be code-driven:

- Compare plan bullets to actionable work packets.
- Detect unrepresented milestones.
- Validate work packets have source references, scope, acceptance criteria, and verification.
- Parse review feedback.
- Check whether verification gates are green.
- Run translation gates.
- Extract changelog entries.
- Identify repeated failure signatures from logs.

### 5. Tone adaptation needs conflict detection

Vela should track potential conflicts among:

- user desire vs. project constraints
- current plan vs. established principles
- emotional support vs. truthful critique
- speed vs. correctness
- autonomy vs. safety
- self-improvement vs. uncontrolled drift

## Open architecture decisions

1. Should Vela depend on an existing agent framework early, or build a minimal in-house kernel first?
2. Should skills/workflows/extensions be defined as files, crates, database records, or all three?
3. What is the first interface: CLI, TUI, chat API, local daemon, or editor integration?
4. What memory substrate should be first: append-only event log, SQLite tables, vector store, or hybrid?
5. What is the minimum viable self-improvement loop?
6. How strict should the permission model be for tool creation and execution?

## Tentative recommendation

Start with a small in-house Rust kernel and borrow selectively:

- Use `tokio`, `clap`, `serde`, `tracing`, SQLite, and typed schemas immediately.
- Study AutoAgents/ADK-Rust/Rig before adopting framework-level dependencies.
- Keep the first Vela kernel small enough to understand completely.
- Add framework integrations later as extensions if they prove useful.

This preserves Vela’s identity as an agent OS while avoiding premature dependency lock-in.
