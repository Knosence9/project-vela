# ADR-0005: Require caller-owned skill registration and explicit prompt composition

- **Status:** accepted
- **Date:** 2026-07-27
- **Owners:** Project Vela maintainers
- **Related issue:** [#654](https://github.com/Knosence9/project-vela/issues/654)
- **First execution issue:** [#655](https://github.com/Knosence9/project-vela/issues/655)

## Context

ADR-0004 keeps prepared skill instructions inert because instruction text can influence model behavior even though it is not executable code. Discovery and preparation prove package identity and recover exact text; neither operation expresses ongoing caller enablement or permission to include that text in a model request.

The project plans place separate capability registries in the small kernel and require deterministic validation, explicit authority, and auditable improvement. Vela also needs a provider-neutral composition contract so provider adapters do not accidentally decide skill authority, ordering, or enablement.

## Decision

Vela uses a caller-owned, process-local `SkillRegistry` containing exact stable IDs and immutable UTF-8 instruction text. Registration is explicit, deterministic, and atomic. A collision with an existing exact ID or another member of the batch rejects the complete batch without replacing any registered instructions. Registry inspection is ordered by exact ID, and debug representations expose instruction lengths rather than bodies.

Extension registration first rejects the lexicographically first selected non-skill before filesystem access, then completes descriptor-anchored preparation for the full batch before calling the registry once. An empty selection is a filesystem-free no-op. Registration remains inert: it does not select instructions for a turn, call a provider, persist enablement, grant tools, or compose a prompt.

Before registered instructions may influence a model, a caller must explicitly select registered exact IDs for each provider request. Selection will reject duplicate and absent IDs and return immutable borrowed skill blocks in exact-ID order. Merely discovering, preparing, or registering a skill will never select it automatically.

The provider-neutral composed request will preserve four distinct fields, in descending authority order:

1. caller-owned system policy;
2. caller-owned developer policy;
3. explicitly selected skill blocks in deterministic exact-ID order;
4. the durable conversation transcript.

Provider adapters may lower those typed fields into provider-native roles, but may not concatenate them behind the caller's back or promote skill instructions above caller policy. Existing runtime methods remain skill-free. A later explicit composed-turn method will accept a caller-owned selection per request. Tool-capable turns require a separate contract that preserves the same composition across every continuation; this decision does not inject skills into them.

## Alternatives considered

### Treat prepared artifacts as the registry

A temporary artifact vector has no collision policy or stable caller-owned lifecycle. Reusing it would blur filesystem preparation with process-local enablement and make accidental prompt insertion easier.

### Automatically include every registered skill

Registration expresses availability, not authority for every turn. Automatic inclusion would make unrelated skills influence requests, obscure prompt growth, and eliminate explicit auditability.

### Concatenate instruction strings in the runtime

Magic delimiters would erase authority levels and force provider adapters to reverse-engineer roles. Typed provider-neutral fields retain policy boundaries and permit provider-specific lowering later.

### Add persistence, precedence, and token budgeting now

Those policies require product evidence that the first process-local registry does not yet provide. They remain additive decisions and are not needed to preserve the authority boundary.

## Consequences

### Positive

- Availability, per-request selection, and model influence remain separate explicit operations.
- Atomic registration preserves a deterministic process-local registry on every failure.
- Provider adapters receive a stable authority order without owning enablement policy.
- Exact instruction text remains provider-neutral and inspectable without debug-log leakage.

### Negative

- Registered skills still cannot affect a model until the explicit selection/composition slice exists.
- Process restart loses registrations.
- Exact-ID ordering is deterministic but does not express dependencies or author-selected precedence.
- Tool-capable turns cannot use skills until continuation semantics are defined.

## Verification

The first execution slice must use RED→GREEN tests proving exact text and ID ordering, redacted debug output, atomic internal/existing collision failures, wrong-kind rejection before filesystem access, preparation-failure atomicity, registry-collision atomicity, and filesystem-free empty registration. The complete repository quality gate must remain green.

A later composition slice must prove duplicate/missing selection rejection, skill-free defaults, exact typed provider fields and authority order, exclusion of registered-but-unselected skills, and unchanged durable failure semantics.

## Revisit when

Reconsider this decision before persisted enablement, precedence or dependency rules, token budgeting, provider-specific serialization, automatic skill routing, skill-authored tool grants, tool-capable continuation composition, replacement or hot reload, workflows, remote packages, or content-addressed identity.
