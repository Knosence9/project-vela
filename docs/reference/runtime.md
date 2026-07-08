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
- `status` reports the latest active session identity, explicit backend API contracts, resolved backend selection, and first-pass extension registry state
- `gateway` now bootstraps durable gateway directories/config, can resume a gateway-specific runtime session, and can deliver a bounded outbound webhook payload with a persisted outbox record
- `agents` now persists bounded subagent delegation requests through a dedicated command-scoped runtime surface
- `mcp` now persists bounded MCP bridge requests through a dedicated command-scoped runtime surface
- `cron` now bootstraps durable scheduler config/job state, can resume a scheduler-specific runtime session, can execute due jobs through the kernel scheduler path, and can deliver job outcomes through the gateway webhook path
- `eval` now bootstraps a durable backend-comparison harness that can execute bounded prompt comparisons across selected backends and persist per-backend results for later inspection
- the eval harness now publishes multiple bounded provider experiment slots (`ternary-preview`, `local-first-replay`, and `capability-parity-scan`) so broader provider experimentation can happen through durable, named lanes without changing the live kernel route
- a durable model-lab policy now lives alongside eval state so deeper model-core work stays governed by explicit graduation gates, allowed strategies, prohibited behaviors, and required evidence

## Current runtime behavior
- bare `vela` creates an interactive chat session and appends an interactive runtime-ready assistant message when no explicit resume target is given
- `vela chat --query ...` creates a single-turn session and can call a configured runtime provider for text turns, with Ollama, a deterministic mock backend, a bounded llama.cpp backend, and the embedded in-process backend now serving as explicit provider implementations or targets behind that boundary
- the backend API contract is now explicit: Vela exposes stable backend descriptors (API version, backend id, transport kind, model requirement, response-source mapping, and capability matrix) so future adapters and experiments can target a bounded kernel-owned interface instead of implicit provider wiring; the new `embedded` backend reserves the in-process local model path without making it the default yet
- provider capabilities are now treated as an explicit matrix rather than implicit backend quirks: Ollama and mock advertise text, bounded tool-loop, reflection/retry support, and first-pass image support; the bounded llama.cpp backend advertises text/tool-loop/reflection support without direct image attachments; and the embedded backend now supports first-pass text-only in-process generation while keeping tool-loop/reflection/image support deferred to later slices
- configured provider-backed turns, including image-backed turns for the supported providers, can run a bounded iterative local tool loop with approved read-only runtime tools before producing the final assistant reply
- approved read-only runtime tools now include targeted internal context retrieval for memory, session history, and skill content (`view_memory`, `search_session_history`, `view_skill`) in addition to the earlier snapshot/list tools
- provider-backed turns, including supported image-backed turns, now perform bounded reflection/retry when they see invalid tool continuations, empty provider replies, or unusable intermediate tool results
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
- `vela extensions --reload` now re-reads extension config + manifest files, recomputes lifecycle transitions, refreshes extension state without resetting durable session state, and fails explicitly when kernel-owned runtime drift is detected, surfacing owned config boundaries such as `runtime.provider@kernel-runtime`, the exact ownership-baseline path/source used for the comparison, and bounded previous/reloaded value diffs for each blocked setting; embedded model asset path changes (`runtime.embedded_model_path`) remain restart-only as well
- the embedded backend currently expects `runtime.embedded_model_path` to point at a local GGUF file and loads that model in-process for bounded text generation without requiring LM Studio, Ollama, or another model server
- extension reload ownership checks now compare against a durable last-applied kernel-runtime baseline (`~/.vela/runtime/reload-ownership-baseline.json`) so restart-only drift remains enforceable across fresh CLI processes instead of only within one in-memory bootstrap, and blocked reloads now direct the operator to restart Vela with the updated config to refresh that baseline intentionally
- `vela chat --image ...` and mixed `vela chat --query ... --image ...` turns can call a configured runtime provider for first-pass image execution, with Ollama and the deterministic mock backend serving that path directly while llama.cpp falls back to the deterministic kernel image scaffold because its bounded contract is text-only
- runtime turn CLI output now prints the resolved response route (`source`, optional `provider`, optional `model`, and advertised provider capabilities) so backend differences stay visible even when execution falls back to the kernel path
- `vela eval --run <prompt> --backend <id>...` now records one durable eval run in `~/.vela/evals/runs.json`, capturing per-backend status, duration, response routing, provider capabilities, bounded previews/errors, and a bounded parity summary so backend comparisons are repeatable without reading raw logs
- `vela eval --run-slot ternary-preview ...` now drives the first bounded architecture experiment slot from `~/.vela/evals/slots.json`, recording which slot was used so future routing experiments stay inspectable and reversible; slot inspection surfaces now also show the latest durable eval id/time/result-count/parity summary for each slot, and `capability-parity-scan` continues to make backend capability differences operator-visible
- `vela eval --show-policy` now reads the durable model-lab policy from `~/.vela/evals/policy.json`, making model-core experimentation criteria operator-visible without mutating the live runtime path
- `vela chat --query ... --checkpoints` can emit review signals and generate review candidates during live execution
- when no provider is configured, or a request cannot use provider-backed execution, query/image turns fall back to deterministic local-kernel scaffold responses
- repeated resume/continue paths update `updated_at` on the matching session row
- active-session reporting currently resolves to the latest `updated_at` row in `sessions`
- `vela gateway --start` resumes the latest `gateway` command session when one already exists
- `vela gateway --webhook-url <url> --payload <text> [--event-type <name>]` posts one JSON payload through the gateway surface, appends durable gateway delivery events/messages, and writes a delivery record into `~/.vela/gateway/outbox/`
- `vela agents --delegate <task> --role <role> [--note <text>]` records one durable bounded subagent delegation request in `~/.vela/agents/delegations.json`, appends a delegation event/message, and rejects duplicate pending requests for the same role/task pair
- `vela mcp --bridge <server> --tool <tool> --payload <json> [--note <text>]` records one durable bounded MCP bridge request in `~/.vela/mcp/requests.json`, appends an MCP bridge event/message, and rejects duplicate pending server/tool/payload requests for the same command surface
- `vela cron --start` resumes the latest `cron` command session when one already exists, executes due durable jobs, recovers stale in-flight jobs before retrying them, and can forward completed/failed job outcomes through the gateway webhook surface when a job carries a delivery target
- `vela cron --add <task> --schedule <expr> [--delivery-webhook-url <url>] [--delivery-event-type <name>]` stores optional automated delivery metadata directly on the durable scheduled job record in `~/.vela/scheduler/jobs.json`
- scheduled jobs now surface explicit progression states in CLI status (`registered`, `started-attempt`, `recovered-for-retry`, `completed-rescheduled`, `failed-rescheduled`) plus explicit delivery state (`delivery_webhook_url`, `delivery_event_type`, `last_delivery_outcome`, `last_delivery_error`) so recurring behavior is visible without reading raw timestamps alone
- `vela cron --report` now aggregates durable scheduler visibility into one bounded summary (job counts, running/overdue/lease-expired health counts, outcomes, delivery failures, delivered deliveries, recoveries, and next due job) and prints per-job due state / health lag seconds, last-run / last-failure details, delivery timestamps, delivery event types, and bounded delivery/error excerpts so operators can inspect recurring health without scanning every job row

## Kernel vs provider boundary
- keep runtime orchestration, lifecycle persistence, tool approvals, retry/fallback rules, and deterministic kernel responses in-kernel
- keep provider-specific request transport, response decoding, and provider-local safety validation behind a provider backend boundary
- treat Ollama as the first provider implementation rather than the runtime’s permanent hard-coded execution path
- preserve local-only provider safety (`VELA_ALLOW_REMOTE_OLLAMA` and `VELA_ALLOW_REMOTE_LLAMACPP` opt-ins for remote endpoints) inside the provider implementations

## Kernel vs extension boundary
- keep durable session/state ownership in-kernel (`vela-state`, runtime lifecycle, approvals, persistence, scheduler continuity, and scheduler execution recovery)
- keep policy-bearing memory/review/session mutation paths in-kernel until stronger trust boundaries exist
- allow extensions to describe discoverable capabilities, tool/skill/workflow metadata, and bounded activation hooks
- treat extension reload as an extensions-owned surface only; provider, network, interface, security, hooks, and other kernel-owned runtime settings remain restart-only even when config drift is detected during reload, surface as explicit ownership drift records, and block the reload from succeeding
- keep service-style extensions metadata-only in this slice while tool/skill/workflow entries may activate when they provide valid entrypoints; explicit manifest `metadata-only` requests validate without activation, while unsupported service `on-boot` requests fail clearly
- treat the current registry as metadata-first scaffolding with bounded activation semantics, not arbitrary third-party code execution

## Still needed
- richer runtime state transitions beyond created/resumed shell states at the session level
- deeper provider capability parity beyond the current explicit matrix (shared text/tool-loop/reflection support, but intentionally different image and transport behavior), while preserving the bounded iterative tool loop, bounded reflection/retry rules, first-pass internal context retrieval tools, the persisted backend eval harness, the first bounded architecture experiment slot, and the explicit model-lab criteria/boundaries policy
- session titles/naming behavior closer to upstream truth
- explicit continue semantics matching upstream lineage behavior
- richer branch-selection behavior and multi-branch navigation beyond the first durable branch/fork model
- more advanced compression policies beyond the current bounded contract (trimmed non-empty summaries, duplicate-latest rejection, bounded summary length, touched-session updates, and explicit persisted operator summaries)
- deeper extension lifecycle hooks and capability activation beyond the current explicit activation matrix (tool/skill/workflow on-boot with entry path, service metadata-only, unsupported service on-boot failure)
- deeper reload ownership enforcement beyond the current block-on-kernel-drift rule and explicit restart-only ownership drift records
- richer recurring scheduling semantics beyond the current explicit progression states and next-run rescheduling model
