# Verification notes

## Goal
Provide executable proof for Vela kernel behaviors so PRs do not rely mainly on manual CLI transcripts.

## Current automated coverage
- `crates/vela-config`
  - invalid/unreadable user config falls back to project config
  - runtime provider/model/base-url settings load from config
- `crates/vela-memory`
  - durable memory format round-trip
  - safe legacy migration detection
  - duplicate staged add rejection
  - stale replace/remove approval diagnostics
- `crates/vela-skills`
  - conflicting staged skill actions are rejected
- `crates/vela-review`
  - duplicate candidate generation is deduplicated across repeat passes
- `crates/vela-state`
  - command session reuse is durable
  - explicit event targeting stays attached to the requested session
  - session inspection parses ordered runtime lifecycle records from persisted events
  - branch lineage and compression summaries remain inspectable after branching/compression flows
- `crates/vela-runtime`
  - scheduler job persistence and dedupe
  - gateway restart continuity avoids duplicate bootstrap messages
  - scheduler restart continuity reuses the same command session and preserves registered jobs
  - extension reload compares against a durable ownership baseline across fresh bootstraps so restart-only runtime drift stays explicit
  - scheduler run state records last outcome, progression, and webhook delivery evidence for durable reporting
  - chat turn execution appends an assistant response and can generate checkpoint artifacts
  - configured Ollama execution is used for text turns when provider/model settings are present
  - configured Ollama execution is used for image turns when provider/model settings are present
  - configured provider turns can request approved local tools across a bounded multi-step loop and continue with each tool result
  - configured provider turns can retrieve targeted memory, session history, and skill context through read-only runtime tools
  - latest persisted session compression summaries are injected back into provider prompts for continuity
  - runtime turns persist ordered lifecycle phases for local, provider-backed, and tool-loop execution paths
  - provider-backed turns can reflect on invalid intermediate continuations, retry within strict limits, and fall back deterministically when recovery fails
  - invalid final provider continuations after max-step tool loops recover or fall back without dropping ordered lifecycle evidence through `finish`
  - bounded eval slots publish durable experiment lanes and latest per-backend evidence without mutating the live runtime route
- `apps/vela/tests/cli_verification.rs`
  - bare `vela` creates a runtime session visible in `vela status`
  - bare `vela` emits an interactive runtime-ready message on first creation
  - `vela chat --query ... --checkpoints` executes a runtime turn, reports lifecycle state, and produces review candidates
  - `vela chat --query ...` uses a configured Ollama provider when present
  - configured provider turns can complete a bounded multi-step runtime tool loop through the CLI while reporting lifecycle phase counts
  - configured provider turns can retrieve targeted skill context through the CLI tool loop
  - sessions can branch and persist compression summaries through the CLI sessions surface
  - configured provider turns can recover from an invalid tool request through bounded reflection/retry in the CLI path
  - configured provider turns can fall back after exhausting bounded reflection retries during mixed text+image turns
  - `vela chat --image ...` uses a configured Ollama provider when present
  - `vela chat --image ...` falls back to a deterministic local-kernel scaffold response when no provider is configured
  - `vela gateway --start` resumes the same gateway session
  - `vela extensions --reload` surfaces the durable ownership baseline, restart-required kernel-runtime drift, and session preservation
  - `vela cron` registration persists across `--show`/`--list`
  - `vela cron --report` surfaces last outcome, progression, and webhook delivery evidence for scheduler jobs
  - `vela eval --list-slots`, `vela eval --show-slot <id>`, and `vela eval --run-slot <id>` expose the published bounded experiment lanes plus their latest durable backend evidence
  - review candidates can be promoted through CLI-only approval and rejection flows while preserving structured inspection output
  - invalid `vela cron --schedule ...` usage is rejected at CLI parse time

## Milestone 11 verification bundle (`#246`)

### Commands / scenarios run
- prerequisite: install libclang and set `LIBCLANG_PATH` appropriately for your local environment before running the Rust verification commands below if your toolchain does not already discover libclang automatically
- `cargo test -p vela-runtime reload_extensions_uses_durable_ownership_baseline_across_bootstraps -- --nocapture`
- `cargo test -p vela-runtime execute_chat_turn_recovers_from_invalid_final_provider_continuation_after_max_tool_steps -- --nocapture`
- `cargo test -p vela-runtime execute_chat_turn_falls_back_after_exhausting_reflection_retries -- --nocapture`
- `cargo test -p vela cron_report_summarizes_scheduler_state -- --nocapture`
- `cargo test -p vela extensions_status_and_reload_are_visible_via_cli -- --nocapture`
- `cargo test -p vela backend_experiment_slot_is_visible_and_runnable -- --nocapture`

### Evidence summary
- End-to-end CLI verification proves scheduler reports surface last outcome, progression, and webhook delivery evidence while eval slots remain published, inspectable, and runnable through the durable `vela eval` surface.
- State continuity and reload verification prove extension reload compares against a durable ownership baseline across fresh bootstraps, preserves the active session, and keeps kernel-owned runtime drift restart-only and operator-visible.
- Failure-path verification proves provider continuation recovery still succeeds after invalid final continuations and falls back deterministically after bounded reflection exhaustion without losing ordered lifecycle evidence.
- Side-by-side comparison was not needed beyond the bounded eval-slot parity surface because the provider experiment lanes themselves already preserve per-backend evidence under one durable comparison harness.

### Result
- Pass

### Follow-up required
- none inside milestone 11; remaining reference/review verification work continues under `#250`

## Milestone 12 verification bundle (`#250`)

### Commands / scenarios run
- prerequisite: install libclang and set `LIBCLANG_PATH` appropriately for your local environment before running the Rust verification commands below if your toolchain does not already discover libclang automatically
- `cargo test -p vela review_candidate_can_be_promoted_and_approved_via_cli -- --nocapture`
- `cargo test -p vela review_candidate_can_be_promoted_and_rejected_via_cli -- --nocapture`
- `cargo build -p vela`
- `./target/debug/vela --help`
- `./target/debug/vela chat --help`
- `./target/debug/vela sessions --help`
- `./target/debug/vela review --help`
- `VELA_HOME=/tmp/vela-verify-home ./target/debug/vela --ignore-user-config status`
- `VELA_HOME=/tmp/vela-verify-home ./target/debug/vela --ignore-user-config chat --provider mock --query "verification smoke" --yolo`
- `VELA_HOME=/tmp/vela-verify-home ./target/debug/vela --ignore-user-config review --list`

### Evidence summary
- End-to-end CLI verification proves the transcript -> review-candidate -> promote -> approve/reject pipeline works entirely through the CLI, with structured `review --show` and `memory --show` output preserved through both approval and rejection paths.
- Reference/docs verification against live behavior confirms the grouped CLI surface advertised in `vela --help`, `vela chat --help`, `vela sessions --help`, and `vela review --help`, while `vela status` exposes the runtime ownership baseline, backend contract matrix, and zero-candidate review state described in the current reference corpus.
- Failure-path verification confirms the documentation now carries a portable libclang prerequisite rather than a machine-specific path; in this environment, `cargo build -p vela` succeeded once `LIBCLANG_PATH` was configured appropriately for the local toolchain.
- State continuity / restart verification was already covered where applicable by the milestone 11 bundle; this milestone focused on the remaining CLI review-pipeline proof and reference-surface checks after the docs slices landed.

### Result
- Pass

### Follow-up required
- none inside milestone 12; remaining work shifts to tracker cleanup / translation-friendly normalization once open verification issues are exhausted

## Remaining gaps
- end-to-end live runtime loop behavior beyond the current local Ollama text/image-turn paths, bounded iterative tool loop, bounded reflection/retry, first-pass lifecycle persistence, internal context retrieval tools, and first-pass branching/compression semantics
- broader review-pipeline integration from transcript -> candidate -> pending -> approval via CLI-only tests
- model/tool execution verification once runtime behavior moves beyond the current shell scaffolding
