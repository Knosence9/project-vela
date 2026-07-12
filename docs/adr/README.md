# Architecture Decision Records

Project Vela uses lightweight Architecture Decision Records (ADRs) for decisions that materially shape the kernel, security model, persistence formats, extension boundaries, or development process.

## Status values

- `proposed`
- `accepted`
- `superseded`
- `rejected`

## Naming

Use a four-digit sequence and concise slug:

```text
0001-use-small-in-house-kernel.md
0002-use-append-only-event-log.md
```

Copy [`template.md`](template.md), fill every section, and link the related issue and pull request. If a later ADR changes the decision, preserve the original and mark it superseded.
