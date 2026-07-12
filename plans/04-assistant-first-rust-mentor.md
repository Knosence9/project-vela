# Assistant-First Rust Mentor Plan

## Correction

The Rust corpus system is not only for future Vela.

It must be created for the assistant first.

The assistant will use it while building Project Vela. That usage is how the system becomes reliable enough for Vela to inherit.

## Purpose

Before Vela can be a Rust-native self-improving assistant OS, the assistant building her needs a better Rust feedback loop.

This plan defines an assistant-facing Rust Mentor system that helps the assistant:

- write more idiomatic Rust
- avoid weak Rust patterns
- catch ownership, lifetime, async, and error-handling mistakes earlier
- ground decisions in curated examples and compiler-backed checks
- improve the corpus through real usage
- turn failures into better guidance, tools, and examples

## Principle

The assistant should not rely on model intuition alone for Rust.

The assistant should use:

1. compiler feedback
2. tests
3. clippy/rustfmt
4. curated dataset examples
5. crate documentation
6. local project patterns
7. recorded failure lessons
8. deterministic corpus quality gates

The assistant writes better Rust by running a process, not by pretending to remember everything.

## Who uses this first?

The assistant uses it first.

Later, Vela internalizes it as a subsystem.

Development order:

1. Build assistant-facing corpus tools.
2. Use them during Project Vela implementation.
3. Record failures and improvements.
4. Rewrite/curate better examples.
5. Stabilize the workflow.
6. Turn the workflow into Vela’s Rust Coding Mentor subsystem.

## Assistant operating loop for Rust work

For every meaningful Rust implementation task, the assistant should do this:

### 1. Identify Rust risk areas

Before writing code, classify likely risks:

- ownership/borrowing
- lifetimes
- async cancellation
- trait/object boundaries
- `Send`/`Sync`
- error handling
- serialization/schema design
- unsafe/FFI
- module/crate organization
- testing strategy
- API ergonomics

### 2. Retrieve relevant corpus examples

Use the curated corpus to find examples by:

- Rust concept
- task type
- crate or domain
- error pattern
- API shape
- test pattern

If the corpus has no relevant example, note the gap.

### 3. Draft implementation

Write the smallest idiomatic Rust patch that satisfies the task.

Prefer:

- simple ownership
- explicit error types where useful
- typed schemas
- small modules
- compile-time validation
- deterministic tests

Avoid:

- clever lifetime gymnastics when simpler ownership works
- unnecessary `Arc<Mutex<_>>`
- fire-and-forget async tasks without lifecycle tracking
- stringly typed internal APIs
- broad `anyhow` boundaries in core library code unless justified

### 4. Run deterministic checks

At minimum when a Rust project exists:

- `cargo fmt --check`
- `cargo check`
- targeted `cargo test`
- `cargo clippy` when practical

For snippets/corpus entries:

- parse with `syn`
- compile in a temporary crate when possible
- run embedded tests when possible

### 5. Compare against corpus and local style

Ask:

- Does the implementation resemble high-trust examples?
- Did it copy a weak pattern?
- Is the code more complex than comparable examples?
- Does the error handling match idiomatic Rust?
- Is async cancellation handled explicitly?
- Are tests meaningful?

### 6. Improve the patch

Use compiler/test/corpus feedback to revise.

### 7. Feed lessons back

After each Rust task, classify what happened:

- new good example
- new bad example
- missing corpus topic
- repeated assistant mistake
- useful deterministic check
- needed tool
- needed rewrite rule

Update the corpus/workflow accordingly.

## Assistant corpus creation responsibilities

The assistant must create the system, not just describe it.

Initial implementation targets:

1. `vela-corpus inspect`
   - inspect Hugging Face dataset metadata, schemas, splits, row samples, and licenses

2. `vela-corpus sample`
   - sample rows by dataset/task/category/topic

3. `vela-corpus score`
   - score rows for parseability, compileability, semantic alignment, and idiom quality

4. `vela-corpus rewrite`
   - propose improved examples from weak-but-salvageable rows

5. `vela-corpus verify`
   - compile/check/test examples in temporary Rust harnesses

6. `vela-corpus index`
   - build a local searchable curated index

These tools should be written in Rust once the repo foundation exists.

Before the Rust CLI exists, the assistant can prototype inspection with scripts, but the durable implementation should be Rust.

## First usable version

The first usable assistant-facing version does not need all features.

Minimum useful loop:

1. Download or fetch metadata for the datasets.
2. Sample rows.
3. Store normalized entries in a local JSONL or SQLite file.
4. Run simple Rust snippet parse checks.
5. Mark obvious mismatches as rejected.
6. Curate a small set of high-quality examples.
7. Use those examples before writing Rust code.

## Corpus quality labels for assistant use

The assistant must know how much to trust each entry.

Labels:

- `raw`: imported, not reviewed
- `suspect`: likely weak or mismatched
- `rejected`: should not guide coding
- `reference_only`: useful for context but not direct imitation
- `checked`: parse/compile checks passed
- `curated`: human/assistant-reviewed and useful
- `vela_derived`: rewritten/improved by this project

The assistant should retrieve from `curated` and `vela_derived` first.

## Rewriting datasets

The assistant may rewrite weak examples, but must preserve provenance.

Every rewritten entry must include:

- original dataset
- original row id or row fingerprint
- original license
- defect found
- rewrite rationale
- rewritten code
- verification result
- whether it is safe to use as guidance

The rewritten corpus is not the same as the source datasets. It is Vela’s curated derivative Rust learning corpus.

## Combining datasets

The assistant should combine datasets by role, not by flattening everything together.

Recommended order for assistant retrieval:

1. Vela-derived curated examples
2. verified `Strandset-Rust-v1` crate-context examples
3. verified `Convence/Rust-Coder` concept examples
4. reference-only semantic insights from rust-analyzer-style material
5. rejected examples only when learning what to avoid

## How the assistant uses it to perfect it

The assistant perfects the system through use.

Every Rust implementation task becomes a test of the mentor:

- Did retrieval help?
- Did it surface the right examples?
- Did it miss an important idiom?
- Did a curated example turn out bad?
- Did checks catch the real bug?
- Did the assistant still make the same mistake?

When the answer reveals a gap, update one of:

- corpus entry
- scoring rule
- rewrite rule
- retrieval query
- verification check
- assistant operating rule
- future Vela workflow

## Initial milestones for assistant-first usage

### Milestone A — Corpus inspector

Create a Rust CLI that can inspect dataset metadata and sample rows.

### Milestone B — Local normalized store

Create a local schema for normalized corpus entries.

### Milestone C — First quality gate

Parse/check sampled Rust snippets and label weak examples.

### Milestone D — First curated pack

Create the first 50–100 high-trust examples the assistant can actually use.

### Milestone E — Assistant Rust work protocol

Before writing Project Vela Rust code, the assistant retrieves relevant examples and runs deterministic checks after writing code.

### Milestone F — Feedback loop

After each Rust task, update the corpus/workflow with lessons.

## Success criteria

This system is working when:

- the assistant uses curated examples before writing nontrivial Rust
- generated Rust patches compile more often on the first or second attempt
- repeated Rust mistakes become less frequent
- weak dataset examples are rejected instead of imitated
- useful rewritten examples accumulate
- Vela’s own implementation becomes an evidence trail for improving the mentor

## Relationship to Vela

This is the seed of Vela’s self-improvement system.

First, the assistant uses it.

Then Vela inherits it.

Eventually, Vela uses the same pattern for other domains:

- planning
- research
- code review
- workflow creation
- extension authoring
- tool building
- memory improvement

But Rust comes first because Vela herself is being built in Rust.
