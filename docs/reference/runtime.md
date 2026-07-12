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
- `eval` now bootstraps a durable backend-comparison harness that can execute bounded prompt comparisons across selected backends — including `embedded` when a local GGUF path is configured — and persist per-backend results for later inspection
- the eval harness now publishes multiple bounded provider experiment slots (`ternary-preview`, `sparse-routing-preview`, `local-first-replay`, `adapter-intake-gate`, and `capability-parity-scan`) so broader provider experimentation can happen through durable, named lanes without changing the live kernel route; those published slot definitions include `embedded` in the bounded backend set for the broader shadow-routing, offline-replay, and parity evidence lanes
- slot inspection surfaces now carry the latest attached per-backend outcome evidence (status, transport, response source, and resolved model) so operators can compare bounded experiments without replaying the live route
- a durable model-lab policy now lives alongside eval state so deeper model-core work stays governed by explicit graduation gates, allowed strategies, prohibited behaviors, and required evidence

## Current runtime behavior
- bare `vela` creates an interactive chat session titled `chat interactive`, persists the bounded session runtime state progression (`receive` → `deliberate` → `respond` → `finish` for the startup turn), and appends an interactive runtime-ready assistant message when no explicit resume target is given
- `vela chat --query ...` creates a single-turn session titled from the normalized query text (`chat: <trimmed query preview>`), collapsing repeated whitespace and truncating long fragments before persisting the durable title that `sessions --show` and title-based resume flows inspect
- image-only chat starts title sessions as `chat image: <filename>` when a filename is available, otherwise `chat image`, and that normalized title is the one exposed through durable inspection and title-based resume flows
- `vela chat --query ...` can call a configured runtime provider for text turns, with Ollama, a deterministic mock backend, a bounded llama.cpp backend, and the embedded in-process backend now serving as explicit provider implementations or targets behind that boundary
- the backend API contract is now explicit: Vela exposes stable backend descriptors (API version, backend id, transport kind, model requirement, response-source mapping, and capability matrix) so future adapters and experiments can target a bounded kernel-owned interface instead of implicit provider wiring; the new `embedded` backend reserves the in-process local model path without making it the default yet
- provider capabilities are now treated as an explicit matrix rather than implicit backend quirks: Ollama and mock advertise text, bounded tool-loop, reflection/retry support, and first-pass image support; the bounded llama.cpp backend advertises text/tool-loop/reflection support without direct image attachments; and the embedded backend now supports first-pass text-only in-process generation plus bounded tool-loop/reflection compatibility for text turns while keeping direct image support deferred to later slices
- configured provider-backed turns, including image-backed turns for the supported providers, can run a bounded iterative local tool loop with approved read-only runtime tools before producing the final assistant reply
- approved read-only runtime tools now include targeted internal context retrieval for memory, session history, and skill content (`view_memory`, `search_session_history`, `view_skill`) in addition to the earlier snapshot/list tools
- provider-backed turns, including supported image-backed turns, now perform bounded reflection/retry when they see invalid tool continuations, empty provider replies, or unusable intermediate tool results
- retrieved tool context is injected back into the provider continuation path as durable tool-result artifacts, keeping context-aware reasoning auditable without allowing live mutation
- each live runtime turn now persists ordered lifecycle phases (`receive`, `deliberate`, `tool-request`, `tool-result`, `reflect`, `retry`, `respond`, `finish`, plus `failed` on error paths)
- session inspection now surfaces the current bounded session runtime state (`ready`, `receive`, `deliberate`, `tool-request`, `tool-result`, `reflect`, `retry`, `respond`, `finish`, or `failed`) alongside parsed runtime lifecycle records, explicit branch lineage, and persisted compression summaries, so operators do not need raw event inspection to see the latest durable session state
- runtime CLI turn output now reports the durable `turn_id`, lifecycle phase count, and final phase, while `vela status` and `vela sessions --show ...` expose the persisted session runtime state for the active or inspected session
- `vela sessions --branch <session> --title <new-title> [--note ...]` can fork a durable child session with explicit parent lineage and copied continuity
- `vela sessions --compress <session> --summary ...` can persist compressed continuity summaries without mutating durable memory directly; summaries are trimmed, must be non-empty, must not reuse the latest or any prior persisted summary for that session, must capture new durable messages since the latest persisted summary, and are capped to a bounded length
- `vela sessions --show <session-or-title>` first prints the branch-selection decision (`exact-session-id`, `exact-anchor-title`, `latest-descendant-of-anchor-title`, or `not-found`) and then exposes branch parentage, lineage path, immediate children, descendant navigation entries, and compression counts through the session inspection surface, including bounded per-compression delta message/event counts
- `vela sessions --list` surfaces recent durable sessions with branch depth and parent context, while `vela sessions --browse` groups durable sessions by root so operators can navigate multi-branch trees without raw state inspection
- `--continue <session-id>` now resumes that exact durable session directly, while title-based `--continue <title>` resolves either to the exact anchor title or the latest descendant in that anchor's branch subtree; CLI output reports whether resolution was `exact-session-id`, `exact-anchor-title`, `latest-descendant-of-anchor-title`, or `latest-global`, alongside the resolved id/title
- Vela now discovers extension manifests from `~/.vela/extensions/` (or `extensions.manifests_dir` in config), applies config-driven enable/disable overrides, and surfaces lifecycle-aware extension entries through `vela status`
- extension lifecycle now distinguishes `discovered`, `validated`, `activated`, `disabled`, and `failed` states, with metadata-only vs on-boot activation boundaries surfaced per entry
- manifests may now declare bounded lifecycle hooks (`on-activate`, `on-reload`) alongside activation policy, and status/reload output surfaces those hook declarations per entry
- tool, skill, and workflow extensions may activate on boot when they provide a non-empty entry path; `on-activate` hooks require that on-boot boundary plus an explicit `activate` capability, `on-reload` hooks remain metadata-visible, and service extensions remain metadata-only in this slice with unsupported `on-boot` or `on-activate` requests failing explicitly in status/reload output
- `vela extensions --reload` now re-reads extension config + manifest files, recomputes lifecycle transitions, refreshes extension state without resetting durable session state, and distinguishes two drift classes in operator output: kernel-owned restart-required drift (which blocks reload) and extension-owned reload drift (which is surfaced explicitly and, when no kernel drift exists, is applied through the reload path); embedded model asset path changes (`runtime.embedded_model_path`) remain restart-only
- the embedded backend currently expects `runtime.embedded_model_path` to point at an existing non-empty local `.gguf` file and loads that model in-process for bounded text generation without requiring LM Studio, Ollama, or another model server
- embedded lifecycle guardrails are now operator-visible through `status`: the runtime surfaces `embedded lifecycle` state (`fixture-ready`, `not-yet-loaded`, `load-failed`, or `invalid-config`), the durable state-file path, and the last persisted load error for the currently configured model path when a previous load failed
- embedded compatibility shims used by deterministic tests are gated to explicit stub-model fixtures only; real embedded sessions continue through model-backed completions for tool-loop/reflection continuations
- extension reload ownership checks now compare against a durable last-applied ownership baseline (`~/.vela/runtime/reload-ownership-baseline.json`) that carries both kernel-owned runtime settings and extension-owned reload inputs, so restart-only drift remains enforceable across fresh CLI processes while extension-manifest / enablement drift can surface as `reload-available` before a reload is attempted; extension override drift details now distinguish added overrides, removed overrides, and value changes
- `vela status` now surfaces that same ownership baseline ahead of reload, including whether the currently loaded runtime config is already restart-required relative to the durable baseline and bounded per-setting current drift lines for operator diagnostics
- `vela chat --image ...` and mixed `vela chat --query ... --image ...` turns can call a configured runtime provider for first-pass image execution: Ollama and the deterministic mock backend serve that path directly with attached image bytes, while text-only backends such as llama.cpp and embedded now answer through a provider-backed image scaffold path that passes operator-visible image metadata without direct attachments
- runtime turn CLI output now prints the resolved response route (`source`, optional `provider`, optional `model`, and advertised provider capabilities) so backend differences stay visible even when execution falls back to the kernel path
- `vela eval --run <prompt> --backend <id>...` now records one durable eval run in `~/.vela/evals/runs.json`, capturing per-backend status, duration, response routing, provider capabilities, bounded previews/errors, and a bounded parity summary so backend comparisons are repeatable without reading raw logs
- `vela eval --run-slot ternary-preview ...` now drives one of the published bounded architecture experiment slots from `~/.vela/evals/slots.json`, recording which slot was used so future routing experiments stay inspectable and reversible; slot inspection surfaces now also show the latest durable eval id/time/result-count/parity summary plus the latest passed backends, failed backends, and capability-group evidence for each slot, and the current published slots carry `embedded` alongside the existing bounded providers so sparse-routing previews, adapter-intake replays, and local-first/parity evidence remain visible without mutating the live route
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
- scheduled jobs now surface explicit progression states in CLI status (`registered`, `started-attempt`, `recovered-for-retry`, `completed-rescheduled`, `failed-rescheduled`) plus explicit delivery progression semantics (`delivery-configured`, `delivery-unconfigured`, `delivery-pending`, `delivery-delivered`, `delivery-failed`, `delivery-skipped`) and bounded delivery evidence (`delivery_webhook_url`, `delivery_event_type`, `last_delivery_outcome`, `last_delivery_status_code`, `delivery_attempt_count`, `last_delivery_error`) so recurring behavior is visible without reading raw timestamps alone
- `vela cron --report` now aggregates durable scheduler visibility into one bounded summary (job counts, running/overdue/lease-expired health counts, outcomes, delivery failures, delivered deliveries, skipped deliveries, recoveries, total delivery attempts, and next due job) and prints per-job schedule/source plus lease/session ownership context (`updated_at`, `last_recovered_at`, `last_session_id`, `execution_token`, `lease_expires_at`), due state / health lag seconds, last-run / last-failure details, delivery timestamps, delivery event types, delivery progression, delivery attempt counts, delivery status codes, and bounded delivery/error excerpts so operators can inspect recurring health without scanning every job row or raw JSON

## Current provider-selection and runtime-contract surface
- `runtime.provider` currently accepts `ollama`, `mock`, `llamacpp` / `llama.cpp`, and `embedded`.
- When no provider is configured, query/image turns stay on the deterministic kernel fallback path.
- The same configured `runtime.provider` now also acts as the default backend selector for bounded eval/model-lab execution when `vela eval --run ...` or `vela eval --run-slot ...` omits explicit `--backend` flags.
- Provider defaults and guardrails currently exposed by the runtime boundary are:
  - `ollama` -> default base URL `http://127.0.0.1:11434`, local-only unless `VELA_ALLOW_REMOTE_OLLAMA` is set
  - `llamacpp` -> default base URL `http://127.0.0.1:8080`, local-only unless `VELA_ALLOW_REMOTE_LLAMACPP` is set
  - `mock` -> in-process deterministic fixture backend with no network transport
  - `embedded` -> in-process local backend that requires `runtime.embedded_model_path` to point at an existing non-empty `.gguf` file
- Response routing is intentionally surfaced as part of the runtime contract:
  - direct provider turns emit provider-specific sources such as `runtime-ollama`, `runtime-llamacpp`, or `runtime-embedded`
  - bounded tool-loop completions emit the corresponding `*-tool-loop` source
  - kernel fallbacks remain explicitly visible as `runtime-kernel`
- Capability differences are part of the documented contract rather than hidden implementation details, and `vela status` / response-route output now distinguishes direct image support from the bounded text-image scaffold path:
  - Ollama and mock support text, bounded tool-loop, reflection/retry, and direct first-pass image handling (`image_scaffold=false`, `images=true`)
  - llama.cpp supports text, bounded tool-loop, reflection/retry, and provider-backed text-only image scaffolds (`image_scaffold=true`), but not direct image attachments in this slice (`images=false`)
  - embedded supports text plus the bounded tool-loop / reflection path for text turns and provider-backed text-only image scaffolds (`image_scaffold=true`), but not direct image attachments in this slice (`images=false`)

## Kernel vs provider boundary
- keep runtime orchestration, lifecycle persistence, tool approvals, retry/fallback rules, and deterministic kernel responses in-kernel
- keep provider-specific request transport, response decoding, and provider-local safety validation behind a provider backend boundary
- treat Ollama as the first provider implementation rather than the runtime’s permanent hard-coded execution path
- preserve local-only provider safety (`VELA_ALLOW_REMOTE_OLLAMA` and `VELA_ALLOW_REMOTE_LLAMACPP` opt-ins for remote endpoints) inside the provider implementations

## Kernel vs extension boundary
- keep durable session/state ownership in-kernel (`vela-state`, runtime lifecycle, approvals, persistence, scheduler continuity, and scheduler execution recovery)
- keep policy-bearing memory/review/session mutation paths in-kernel until stronger trust boundaries exist
- allow extensions to describe discoverable capabilities, tool/skill/workflow metadata, and bounded activation hooks
- treat extension reload as an extensions-owned surface only; provider, network, interface, security, hooks, and other kernel-owned runtime settings remain restart-only even when config drift is detected during reload, surface as explicit ownership drift records, and block the reload from succeeding, while extension-owned manifest-dir and enable/disable drift surfaces explicitly as reload-available or reload-applied instead of remaining silent
- keep service-style extensions metadata-only in this slice while tool/skill/workflow entries may activate when they provide valid entrypoints and declare `activate` capability for `on-activate` hooks; explicit manifest `metadata-only` requests validate without activation, bounded lifecycle hooks stay declarative (`on-activate`, `on-reload`), and unsupported service `on-boot` / `on-activate` requests fail clearly
- treat the current registry as metadata-first scaffolding with bounded activation semantics, not arbitrary third-party code execution

## Longer-horizon runtime roadmap notes
These items are intentionally kept as roadmap themes rather than current execution slices after the current milestone work:
- deeper provider capability parity beyond the current explicit matrix, while preserving the documented bounded tool loop, reflection/retry, and explicit response-route contract
- deeper branch-selection behavior beyond the current branch-aware list/browse/show navigation surfaces
- deeper compression policy behavior beyond the current bounded contract (trimmed non-empty summaries, duplicate latest/prior rejection, require-new-message gating, bounded summary length, touched-session updates, and explicit persisted operator-visible delta summaries)
- deeper extension lifecycle hooks and capability activation beyond the current explicit activation matrix and declarative hook surface (`on-activate`/`on-reload`, explicit `activate` capability for activation hooks, tool/skill/workflow on-boot with entry path, service metadata-only, unsupported service activation requests failing clearly)
- deeper reload ownership enforcement beyond the current restart-required vs reload-owned drift split, per-setting ownership records, added/removed/value-specific extension override drift, and explicit reload-available / reload-applied handling
- richer recurring scheduling semantics beyond the current explicit progression/delivery progression states, delivery-attempt evidence, and next-run rescheduling model
