# Vela Rust Corpus Strategy

## Purpose

Vela should not merely consume existing Rust datasets. She should use them as raw material to build a higher-quality, Vela-specific Rust corpus.

The goal is to help Vela write better Rust by creating a curated corpus that is:

- idiomatic
- compiler-checked
- semantically aligned
- license-aware
- provenance-tracked
- task-labeled
- retrieval-friendly
- useful for review, repair, explanation, and planning

This is not initially a fine-tuning project. It is first a **corpus engineering** project for Rust coding assistance, and the first user is the assistant building Vela. The assistant must create and use this system before Vela internalizes it.

## Core idea

Take multiple Rust datasets, inspect and score them, clean weak samples, rewrite/improve useful samples, and combine high-quality entries into one assistant-usable, Vela-native Rust knowledge corpus.

The immediate target is assistant use: every serious Rust implementation should be improved by this corpus before Vela relies on it internally.

Pipeline:

1. Ingest source datasets.
2. Normalize schemas.
3. Score sample quality.
4. Reject low-trust examples.
5. Rewrite/improve salvageable examples.
6. Compile/check/test where possible.
7. Attach provenance and license metadata.
8. Classify by Rust concept and task type.
9. Index for retrieval and evaluation.
10. Feed Vela’s Rust Coding Mentor subsystem.

## Source datasets

### `Convence/Rust-Coder`

Role:

- Educational Rust concept prompts.
- Code + explanation pairs.
- Broad topic coverage.

Strength:

- Simple schema.
- Apache-2.0.
- Useful for concept foundations and explanation tasks.

Weakness:

- Some examples may be generic or mismatched to the prompt.
- Needs semantic alignment checks.

Best use:

- Concept taxonomy.
- Rewrite source material.
- Explanation and teaching examples after quality improvement.

### `gubernac/Rust-Coder`

Role:

- Apparent duplicate/mirror of `Convence/Rust-Coder`.

Best use:

- Checksum/duplicate verification.
- Do not include separately unless it contains meaningful differences.

### `Fortytwo-Network/Strandset-Rust-v1`

Role:

- Crate-contextual Rust code tasks.
- Bug detection, comment generation, code repair, and similar task categories.

Strength:

- Apache-2.0.
- Has crate names and richer code context.
- Some entries include tests.

Weakness:

- Still needs validation.
- Need inspect task distribution and test executability.

Best use:

- Code repair examples.
- Patch-evaluation examples.
- Crate-context retrieval.
- Test-aware examples.

### `introspector/rust-analyser`

Role:

- Semantic analysis and rust-analyzer-style code intelligence.

Strength:

- Rich semantic fields.
- Useful for designing Vela’s deterministic Rust analysis tools.

Weakness:

- AGPL-3.0 license.
- Should be treated carefully and not blended into permissive corpora without review.

Best use:

- Architecture inspiration.
- Separate research-only index.
- Do not mix into Apache/permissive generated corpus by default.

## Combined corpus design

The Vela corpus should not preserve each dataset’s schema directly. It should normalize all acceptable material into a common schema.

### Proposed normalized schema

```text
VelaRustCorpusEntry
- id
- source_dataset
- source_split
- source_row_id
- source_license
- provenance_url
- trust_level
- task_type
- rust_concepts
- crate_context
- instruction
- input_code
- surrounding_context
- expected_output
- explanation
- tests
- compiler_status
- clippy_status
- semantic_alignment_score
- idiom_score
- safety_risk_level
- rewrite_status
- rewritten_by
- rewrite_rationale
- verification_artifacts
```

### Task types

Initial task taxonomy:

- concept_explanation
- code_generation
- code_repair
- bug_detection
- comment_generation
- doc_generation
- refactor
- API_design
- ownership_borrowing_review
- lifetime_review
- error_handling_review
- async_review
- unsafe_ffi_review
- concurrency_review
- crate_idiom_review
- test_generation
- patch_review

### Trust levels

- `raw`: ingested but not checked
- `parsed`: syntactically readable
- `compiles`: code compiles in an isolated harness
- `tested`: supplied/generated tests pass
- `semantically_aligned`: instruction, code, and explanation match
- `idiomatic`: accepted by Rust idiom checks/review
- `curated`: high-trust example suitable for retrieval
- `rejected`: should not guide Vela
- `research_only`: useful but not safe/licensed for blended use

## Rewrite and improvement pipeline

Some dataset rows should be rejected outright. Others can be improved.

### Reject when

- Code is unrelated to instruction.
- Explanation is technically false.
- Unsafe code is unjustified or misleading.
- Code cannot be parsed and cannot be repaired simply.
- License/provenance is incompatible.
- Sample is too generic to teach anything.

### Rewrite when

- The topic is useful but the code is weak.
- Explanation is vague but fixable.
- Code compiles but is not idiomatic.
- Example lacks tests but can be tested.
- Example demonstrates the right concept but needs clearer context.

### Rewrite rules

Each rewrite must preserve provenance:

- original source dataset
- original row id
- original license
- what changed
- why it changed
- verification performed

Rewritten entries should be marked as Vela-derived examples, not passed off as original dataset rows.

## Example improvement pattern

Problem pattern:

- Instruction: “How do you refactor Calling C functions (FFI) with strict memory constraints?”
- Code: simple `HashMap` insertion
- Issue: code does not meaningfully demonstrate FFI or memory constraints

Possible handling:

1. Mark original row as semantically weak.
2. Do not use it for retrieval as-is.
3. Rewrite into a real FFI-safe example, such as:
   - `extern "C"` declaration
   - safe wrapper
   - pointer/null checks
   - ownership boundary documentation
   - testable non-FFI mock where possible
4. Add explanation about unsafe boundary and memory ownership.
5. Mark rewritten example as Vela-derived.
6. Run `cargo check` on the example harness.

## Dataset combination strategy

### Do not make one flat blob

Combining should preserve source roles.

Recommended indexes:

1. **Concept Index**
   - Mostly curated/improved `Rust-Coder` entries.
   - Used for explanations and learning.

2. **Patch Pattern Index**
   - Mostly `Strandset-Rust-v1` entries.
   - Used for repairs, comments, bug detection, tests.

3. **Semantic Analysis Index**
   - Inspired by or separately sourced from rust-analyzer data.
   - License-sensitive; keep separate if using AGPL material.

4. **Vela-Derived Curated Index**
   - Rewritten, verified, high-trust examples.
   - Best retrieval source for Vela’s own coding.

5. **Rejected/Quarantine Index**
   - Bad examples with reasons.
   - Useful so Vela learns what not to imitate.

## Deterministic tooling Vela needs

### `vela-corpus inspect`

Inspect dataset schemas, licenses, row counts, task distributions, and sample quality.

### `vela-corpus sample`

Sample rows by source, concept, task type, crate, trust level, or risk category.

### `vela-corpus score`

Score rows for:

- parseability
- compileability
- testability
- instruction/code alignment
- explanation/code alignment
- idiom quality
- safety risk
- duplicate likelihood

### `vela-corpus rewrite`

Generate improved candidate entries with explicit rationale.

This should be review-gated. Vela can propose rewrites, but accepted rewrites should pass deterministic verification.

### `vela-corpus verify`

Run harnesses:

- `rustfmt`
- `cargo check`
- `cargo test`
- `clippy`
- optional rust-analyzer diagnostics

### `vela-corpus index`

Build retrieval indexes:

- SQLite metadata tables
- FTS/Tantivy text index
- optional embeddings
- provenance graph

## Integration with Rust Coding Mentor

The assistant-first operating plan lives in [`04-assistant-first-rust-mentor.md`](./04-assistant-first-rust-mentor.md).

The Rust Coding Mentor should use the curated corpus like this:

1. User asks Vela to write or edit Rust.
2. Vela drafts code.
3. Deterministic Rust checks run.
4. Mentor retrieves relevant curated examples.
5. Mentor compares the patch against:
   - local code style
   - compiler diagnostics
   - curated examples
   - known Rust idioms
6. Mentor proposes corrections.
7. Checks run again.
8. Lessons from failures update skills/workflows/corpus quality notes.

## Why this matters

If Vela is written in Rust, she needs to internalize Rust’s actual constraints:

- ownership
- borrowing
- lifetimes
- trait design
- error handling
- async cancellation
- Send/Sync boundaries
- unsafe boundaries
- module/crate organization
- tests and examples
- idiomatic API design

The corpus should make those constraints searchable, testable, and reusable.

## Planning recommendation

Add an early milestone for **Assistant Rust Corpus Lab** before or alongside serious Vela implementation. This is not optional polish; it is how the assistant gets better at writing the Rust that will become Vela.

Milestone goals:

1. Build a dataset inspection CLI.
2. Verify schema and licenses.
3. Sample and score rows.
4. Create the normalized corpus schema.
5. Curate the first 100 high-trust Rust examples.
6. Build retrieval over the curated corpus.
7. Use it in a Rust Coding Mentor workflow.

This ensures Vela’s own implementation is shaped by proper Rust from the beginning.
