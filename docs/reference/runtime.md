# Runtime compatibility notes

## Implemented in the Rust scaffold
- bare `vela` now bootstraps a runtime session instead of only printing help
- `chat` now executes a runtime turn instead of only bootstrapping a session
- runtime session bootstrap persists into `state.db`
- session rows currently store:
  - `id`
  - `title`
  - `command_name`
  - `interaction_mode`
  - `created_at`
  - `updated_at`
- `--resume <session-id-or-title>` resumes a stored session
- `--continue <name>` now resumes the latest session in the matched branch subtree, so continuing a parent or branch title prefers the freshest related descendant
- bare `--continue` resumes the latest session
- interactive vs single-turn mode is derived from whether query/image input is present
- `status` reports the latest active session identity and first-pass extension registry state
- `gateway` now bootstraps durable gateway directories/config and can resume a gateway-specific runtime session
- `cron` now bootstraps durable scheduler config/job state, can resume a scheduler-specific runtime session, and can execute due jobs through the kernel scheduler path

## Current runtime behavior
- bare `vela` creates an interactive chat session and appends an interactive runtime-ready assistant message when no explicit resume target is given
- `vela chat --query ...` creates a single-turn session and can call a configured runtime provider for text turns, with Ollama currently serving as the first provider implementation behind that boundary
- configured provider-backed turns can run a bounded iterative local tool loop with approved read-only runtime tools before producing the final assistant reply
- approved read-only runtime tools now include targeted internal context retrieval for memory, session history, and skill content (`view_memory`, `search_session_history`, `view_skill`) in addition to the earlier snapshot/list tools
- provider-backed turns now perform bounded reflection/retry when they see invalid tool continuations, empty provider replies, or unusable intermediate tool results
- retrieved tool context is injected back into the provider continuation path as durable tool-result artifacts, keeping context-aware reasoning auditable without allowing live mutation
- each live runtime turn now persists ordered lifecycle phases (`receive`, `deliberate`, `tool-request`, `tool-result`, `reflect`, `retry`, `respond`, `finish`, plus `failed` on error paths)
- session inspection now surfaces parsed runtime lifecycle records, explicit branch lineage, and persisted compression summaries alongside raw messages and events
- runtime CLI turn output now reports the durable `turn_id`, lifecycle phase count, and final phase
- `vela sessions --branch <session> --title <new-title> [--note ...]` can fork a durable child session with explicit parent lineage and copied continuity
- `vela sessions --compress <session> --summary ...` can persist compressed continuity summaries without mutating durable memory directly; summaries are trimmed, must be non-empty, must differ from the latest persisted summary for that session, and are capped to a bounded length
- `vela sessions --show <session>` exposes branch parentage, immediate child sessions, and compression counts through the session inspection surface
- title-based `--continue <title>` now resolves to either the exact anchor session or the latest descendant in that anchor's branch subtree, and CLI output reports whether resolution was `exact-anchor`, `latest-in-subtree`, or `latest-global`
- Vela now discovers extension manifests from `~/.vela/extensions/` (or `extensions.manifests_dir` in config), applies config-driven enable/disable overrides, and surfaces lifecycle-aware extension entries through `vela status`
- extension lifecycle now distinguishes `discovered`, `validated`, `activated`, `disabled`, and `failed` states, with metadata-only vs on-boot activation boundaries surfaced per entry
- tool, skill, and workflow extensions may activate on boot when they provide a non-empty entry path; service extensions remain metadata-only in this slice, and unsupported service `on-boot` requests fail explicitly in status/reload output
- `vela extensions --reload` now re-reads extension config + manifest files, recomputes lifecycle transitions, refreshes extension state without resetting durable session state, and reports restart-only runtime drift as explicit owned config boundaries (for example `runtime.provider@kernel-runtime`)
- `vela chat --image ...` can call a configured runtime provider for first-pass image turns, with Ollama currently serving as the first image-capable provider behind the kernel boundary
- `vela chat --query ... --checkpoints` can emit review signals and generate review candidates during live execution
- when no provider is configured, or a request cannot use provider-backed execution, query/image turns fall back to deterministic local-kernel scaffold responses
- repeated resume/continue paths update `updated_at` on the matching session row
- active-session reporting currently resolves to the latest `updated_at` row in `sessions`
- `vela gateway --start` resumes the latest `gateway` command session when one already exists
- `vela cron --start` resumes the latest `cron` command session when one already exists, executes due durable jobs, and recovers stale in-flight jobs before retrying them

## Kernel vs provider boundary
- keep runtime orchestration, lifecycle persistence, tool approvals, retry/fallback rules, and deterministic kernel responses in-kernel
- keep provider-specific request transport, response decoding, and provider-local safety validation behind a provider backend boundary
- treat Ollama as the first provider implementation rather than the runtime’s permanent hard-coded execution path
- preserve local-only Ollama safety (`VELA_ALLOW_REMOTE_OLLAMA` opt-in for remote endpoints) inside the Ollama provider implementation

## Kernel vs extension boundary
- keep durable session/state ownership in-kernel (`vela-state`, runtime lifecycle, approvals, persistence, scheduler continuity, and scheduler execution recovery)
- keep policy-bearing memory/review/session mutation paths in-kernel until stronger trust boundaries exist
- allow extensions to describe discoverable capabilities, tool/skill/workflow metadata, and bounded activation hooks
- treat extension reload as an extensions-owned surface only; provider, network, interface, security, hooks, and other kernel-owned runtime settings remain restart-only even when config drift is detected during reload, and surface as explicit ownership drift records in reload output
- keep service-style extensions metadata-only in this slice while tool/skill/workflow entries may activate when they provide valid entrypoints; explicit manifest `metadata-only` requests validate without activation, while unsupported service `on-boot` requests fail clearly
- treat the current registry as metadata-first scaffolding with bounded activation semantics, not arbitrary third-party code execution

## Still needed
- richer runtime state transitions beyond created/resumed shell states at the session level
- additional provider implementations beyond the first Ollama-backed provider boundary, while preserving the bounded iterative tool loop, bounded reflection/retry rules, and first-pass internal context retrieval tools
- session titles/naming behavior closer to upstream truth
- explicit continue semantics matching upstream lineage behavior
- richer branch-selection behavior and multi-branch navigation beyond the first durable branch/fork model
- more advanced compression policies beyond the current bounded contract (trimmed non-empty summaries, duplicate-latest rejection, bounded summary length, touched-session updates, and explicit persisted operator summaries)
- deeper extension lifecycle hooks and capability activation beyond the current explicit activation matrix (tool/skill/workflow on-boot with entry path, service metadata-only, unsupported service on-boot failure)
- richer reload ownership reporting beyond the first explicit restart-only ownership drift records
- richer recurring scheduling semantics beyond the first durable execution/recovery sweep and next-run rescheduling model
