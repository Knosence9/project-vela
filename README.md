<div align="center">

# Project Vela

**A Rust-native, self-improving AI assistant operating system.**

Part practical assistant. Part adversarial research partner. Part honest best friend.

[Vision](plans/00-north-star.md) · [Architecture](plans/01-architecture-research.md) · [System map](docs/project-vela-visual.html)

</div>

> [!IMPORTANT]
> Vela is at the specification and bootstrap stage. The repository does not yet contain a runnable assistant.

## North star

Vela is not intended to be a passive chatbot. She should notice conflict, challenge weak assumptions, remember useful lessons, improve her procedures, and build missing tools within explicit safety and review boundaries.

The system is guided by one engineering rule:

> Models reason, decide, synthesize, and communicate. Code parses, validates, schedules, checks, records, and enforces contracts.

## Bootstrap loop

Project Vela develops through an evidence-producing loop:

```text
External Rust references
        ↓
Retrieve relevant patterns
        ↓
Implement a focused Vela change
        ↓
Format · compile · lint · test · review
        ↓
Correct and verify
        ↓
Capture the development episode
        ↓
Grow the Vela-native corpus
        ↺
```

External Rust datasets help the assistant write better Rust. At the same time, Vela's own implementation produces higher-value project-native examples: tasks, context, patches, diagnostics, corrections, rationale, tests, and verified outcomes. Vela will eventually inherit this evidence from her own development.

## Intended architecture

Vela's small, inspectable Rust kernel will eventually own:

- identity and behavioral policy
- task and session lifecycles
- durable memory and event replay
- tools, skills, workflows, and extensions
- permissions and isolation
- observability and deterministic validation
- controlled, auditable self-improvement

Existing Rust frameworks may be studied or integrated, but Vela should not be a thin wrapper around one framework.

## Development environment

Nix is the supported development entry point. The flake pins the Rust toolchain, Rust quality tools, GitHub tooling, formatters, native libraries, and CI utilities used by the project:

```bash
nix develop
```

Run a single command without entering an interactive shell with:

```bash
nix develop --command <command>
```

The committed `flake.lock` keeps local development and CI reproducible across x86-64 Linux, ARM64 Linux, and Apple Silicon macOS.

Run the complete local quality gate with:

```bash
nix develop --command just verify
```

## Secret management

Vela commits secret **declarations** in [`secretspec.toml`](secretspec.toml), but
secret values must remain in an external SecretSpec provider such as the system
keyring. Set up your preferred provider once, verify the required values, and
then run commands with only their declared secrets injected:

```bash
nix develop --command secretspec config init
nix develop --command secretspec check
nix develop --command just with-secrets cargo run --locked -p vela-dev -- --help
```

CI and the local quality gate use an ephemeral, permission-restricted dotenv
fixture containing a disposable test value. They remove it on exit and never
require or print a developer credential. See
[`docs/adr/0001-declarative-secret-management.md`](docs/adr/0001-declarative-secret-management.md)
for the trust boundary and rationale.

## Developer CLI

The initial Rust workspace provides `vela-dev`, the command-line home for corpus and development-evidence tooling:

```bash
nix develop --command cargo run --locked -p vela-dev -- --help
nix develop --command cargo run --locked -p vela-dev -- record --help
```

The workspace includes schema-versioned development-record validation:

```bash
nix develop --command cargo run --locked -p vela-dev -- record validate path/to/record.json
```

Verified project-native records live under `corpus/development/`. Inspect every JSON record recursively, in deterministic relative-path order, with:

```bash
nix develop --command cargo run --locked -p vela-dev -- corpus inspect corpus/development
```

Inspection prints each valid record and an aggregate summary. It continues past malformed, unreadable, or semantically invalid records, emits path-prefixed diagnostics, and exits non-zero when any record is invalid.

See [`docs/development-record-v1.md`](docs/development-record-v1.md) for the version 1 shape, invariants, stable diagnostics, and exit statuses.

## Development status

The first milestone is the **evidence loop**:

1. Establish the public GitHub workflow and Rust quality gates. ✅
2. Build a minimal `vela-dev` CLI. ✅
3. Define and validate development records. ✅
4. Store and inspect a small Vela-native corpus. ✅
5. Use the creation of that tooling as the first real corpus episode. ✅
6. Begin the kernel with a typed append-only event log and replay. ✅
7. Start the persisted task lifecycle with durable start, output-bearing completion, reason-bearing cancellation, diagnosed failure, and load. ✅
8. Start the persisted session lifecycle with durable creation, close, reopen, and load. ✅

## Project documents

- [`plans/00-north-star.md`](plans/00-north-star.md) — identity and operating principles
- [`plans/01-architecture-research.md`](plans/01-architecture-research.md) — Rust ecosystem and kernel boundaries
- [`plans/02-rust-dataset-understanding.md`](plans/02-rust-dataset-understanding.md) — external dataset findings
- [`plans/03-rust-corpus-strategy.md`](plans/03-rust-corpus-strategy.md) — corpus design and quality strategy
- [`plans/04-assistant-first-rust-mentor.md`](plans/04-assistant-first-rust-mentor.md) — assistant-first Rust feedback loop
- [`docs/project-vela-visual.html`](docs/project-vela-visual.html) — standalone visual system map
- [`docs/event-log.md`](docs/event-log.md) — typed append/replay behavior and stable errors
- [`docs/task-lifecycle.md`](docs/task-lifecycle.md) — persisted task start/completion/cancellation/load behavior
- [`docs/session-lifecycle.md`](docs/session-lifecycle.md) — persisted session lifecycle behavior
- [`docs/adr/`](docs/adr/) — architecture decision records

## Contributing

Project Vela is being built in public. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for branch, review, verification, and corpus-safety rules.

## License

No project license has been selected yet. Until one is added, copyright law reserves all rights to the project owner.
