# Rust Dataset Understanding for Vela

## Purpose

These datasets are not primarily a model-training plan. Their first purpose is to help Vela write better Rust.

Vela should use them as:

- reference material for idiomatic Rust examples
- retrieval material for Rust concepts and crate patterns
- evaluation material for generated Rust patches
- test-case inspiration for ownership, borrowing, async, error handling, and API design
- seed material for deterministic Rust-code review checklists
- later, optional training/fine-tuning material only after license and quality review

The goal is not “use datasets because they exist.” The goal is: **make Vela better at writing proper Rust.**

## Important principle: do not trust dataset labels blindly

A dataset can be useful without being uniformly high quality.

Example: the first inspected `Convence/Rust-Coder` row asks about “Calling C functions (FFI)” but its code sample is a simple `HashMap` insertion. That mismatch means Vela must not treat every row as authoritative idiomatic Rust.

Therefore Vela needs a Rust dataset quality gate before using examples as guidance.

Quality checks should include:

1. Does the code compile?
2. Does the code actually answer the instruction?
3. Is the explanation technically correct?
4. Does the example demonstrate the named Rust concept?
5. Is it idiomatic for modern Rust?
6. Are unsafe/FFI/concurrency examples especially scrutinized?
7. Is license compatible with the intended use?
8. Does the sample come from real crate context, synthetic generation, semantic extraction, or educational prompt data?

## Dataset: `Convence/Rust-Coder`

Verified path:

- <https://huggingface.co/datasets/Convence/Rust-Coder>

Metadata observed:

- Author: Convence
- License: Apache-2.0
- Size category: 10K–100K
- Claimed card count: 12,000 unique samples
- Tasks: text generation, question answering
- Language: English
- Format: parquet
- Splits: train, validation

Schema:

- `id`: UUID
- `instruction`: prompt/question about Rust
- `code`: Rust code snippet
- `explanation`: explanation of concept/code
- `category`: high-level Rust category
- `topic`: specific topic
- `metadata`: adjective/verb/context/length-style metadata

Covered topics from card:

- Ownership & Borrowing
- Types & Data Structures
- Control Flow & Logic
- Functions & Methods
- Error Handling
- Standard Library & Collections
- Concurrency & Parallelism
- Macros & Metaprogramming
- Unsafe & FFI

Potential use for Vela:

- Rust concept retrieval.
- Prompt-to-code/code-to-explanation examples.
- Building a Rust learning map.
- Generating initial Rust review checklist categories.

Risks:

- At least one inspected sample looked semantically weak: FFI instruction paired with generic `HashMap` code.
- May contain synthetic/template-like examples.
- Should not be used as an unquestioned source of idiomatic Rust.

Recommended Vela treatment:

- Use as low-to-medium trust educational material.
- Require compile checks and semantic relevance scoring before examples influence code generation.
- Prefer rows that compile and whose code/explanation directly match the topic.

## Dataset: `gubernac/Rust-Coder`

Verified path:

- <https://huggingface.co/datasets/gubernac/Rust-Coder>

Observed relationship:

- Hugging Face page says it is duplicated from `Convence/Rust-Coder`.

Metadata observed:

- Author: gubernac
- License: Apache-2.0
- Size category: 10K–100K
- Tasks: text generation, question answering
- Language: English
- Format: parquet
- Splits: train, validation
- Same apparent schema as `Convence/Rust-Coder`

Recommended Vela treatment:

- Treat as a mirror/duplicate unless deeper checksum comparison proves differences.
- Do not index both by default; avoid duplicate retrieval pollution.
- Canonical candidate: `Convence/Rust-Coder`.

## Dataset: `introspector/rust-analyser`

Verified path:

- <https://huggingface.co/datasets/introspector/rust-analyser>

Metadata observed:

- Author: introspector
- License: AGPL-3.0
- Size category: 100K–1M
- Tasks: text classification, feature extraction, text retrieval
- Format: parquet
- Modality: tabular/text
- Built from rust-analyzer semantic analysis extraction

Schema observed:

- `id`
- `file_path`
- `line`
- `column`
- `phase`
- `processing_order`
- `element_type`
- `element_name`
- `element_signature`
- `syntax_data`
- `symbol_data`
- `type_data`
- `diagnostic_data`
- `processing_time_ms`
- `timestamp`
- `rust_version`
- `analyzer_version`
- `source_snippet`
- `context_before`
- `context_after`

Observed phase example:

- `name_resolution`

Potential use for Vela:

- Understanding how rust-analyzer sees Rust code.
- Building semantic-code-review heuristics.
- Designing Vela’s own Rust analysis pipeline.
- Inspiration for deterministic code intelligence features.

Risks:

- AGPL-3.0 means usage must be carefully constrained.
- Do not embed data or derivative artifacts into Vela without license review.
- Better as research/reference for architecture than as shipped training data.

Recommended Vela treatment:

- High conceptual value.
- License-sensitive.
- Use to design local analysis tools that run rust-analyzer/cargo directly on Vela code, rather than copying dataset content into Vela.

## Dataset: `Fortytwo-Network/Strandset-Rust-v1`

Verified path:

- <https://huggingface.co/datasets/Fortytwo-Network/Strandset-Rust-v1>

Metadata observed:

- Author: Fortytwo-Network
- License: Apache-2.0
- Size category: 100K–1M
- Format: parquet
- Splits: train, test
- Related model/report: Strand-Rust-Coder-v1 / Strand-Rust-Coder-14B-v1

Schema:

- `crate_name`
- `input_data`
- `output_data`
- `task_category`
- `test`

Observed task categories/examples:

- `comment_generation`
- `bug_detection`

Observed sample patterns:

- Real crate context such as `datafusion-datasource-csv`.
- Input can include code and broader `code_context`.
- Output can include transformed/commented/fixed code.
- Bug detection examples may include tests.

Potential use for Vela:

- Strong candidate for Rust coding assistance.
- Patch generation evaluation.
- Bug-fix examples with crate context.
- Comment/doc generation checks.
- Test-aware code repair examples.
- Retrieval over real crate idioms.

Risks:

- Still likely synthetic or generated/validated material, so quality gates remain necessary.
- Need inspect distribution across `task_category` before weighting use.
- Need verify whether `test` fields are executable and how often they are present.

Recommended Vela treatment:

- Medium-to-high value candidate.
- Prioritize for Vela’s Rust coding mentor subsystem after building a local inspection tool.
- Use examples with tests and crate context preferentially.

## Vela subsystem proposal: Rust Coding Mentor

Vela should have a Rust Coding Mentor subsystem whose job is to improve Rust output before code is committed.

Inputs:

- Vela-generated patch
- Rust compiler output
- `cargo fmt`, `cargo clippy`, and test output
- rust-analyzer diagnostics where available
- retrieved high-quality dataset examples
- crate docs and local codebase patterns

Outputs:

- corrected Rust patch
- explanation of ownership/lifetime/API changes
- risk notes
- deterministic verification results
- lessons for skill/workflow improvement

Core deterministic checks:

1. `cargo fmt --check`
2. `cargo check`
3. targeted `cargo test`
4. `cargo clippy` where practical
5. rust-analyzer diagnostics if available
6. patch-local API surface comparison
7. dataset-inspired idiom review only after sample quality scoring

## Vela dataset ingestion plan

Before relying on these datasets, build a local inspection tool. The broader rewrite/combine strategy is defined in [`03-rust-corpus-strategy.md`](./03-rust-corpus-strategy.md).

Minimum tool capabilities:

1. Download or stream selected Hugging Face parquet files.
2. Read schema and row counts.
3. Sample rows by category/task.
4. Compile code snippets when possible.
5. Score instruction-code-explanation alignment.
6. Detect duplicate or near-duplicate rows.
7. Flag unsafe/FFI/concurrency examples for stricter review.
8. Export a curated local index of only high-trust examples.
9. Record license and provenance per row/source.

Possible implementation stack:

- Rust CLI using `hf-hub` or HTTP download.
- `parquet` / `arrow` crates for parquet reading.
- `syn` for parsing Rust snippets.
- `cargo` subprocess for compile checks in temp crates.
- SQLite/FTS or Tantivy for local retrieval.

## How this shapes Vela

Vela should be specific to proper Rust by building the following into her own development process:

- Rust-first architecture decisions.
- Compiler-backed validation.
- Dataset-assisted but not dataset-blinded review.
- Strong ownership/lifetime/error-handling patterns.
- Async cancellation and structured concurrency discipline.
- Idiomatic crate usage learned from real examples and local docs.
- A feedback loop from failed Rust patches into better skills, workflows, and tools.
