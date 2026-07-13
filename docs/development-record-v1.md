# Development record schema version 1

A Vela development record captures one implementation episode as deterministic, reviewable JSON. The Rust types in `vela_dev::record` are the schema authority. Unknown fields, unsupported schema versions, and values outside the bounded enums are rejected during deserialization; cross-field rules are reported separately by semantic validation.

## Shape

```json
{
  "schema_version": 1,
  "task": {
    "title": "Validate records",
    "objective": "Reject invalid evidence",
    "acceptance_criteria": ["valid records pass"]
  },
  "attempts": [{
    "summary": "Implemented validation",
    "outcome": "success",
    "diagnostic": null,
    "patch": "crates/vela-dev/src/record.rs"
  }],
  "outcome": {
    "summary": "Validation works",
    "verified": true,
    "verification": [{"command": "just verify", "status": "passed"}]
  },
  "lessons": ["Keep validation deterministic"],
  "provenance": {
    "repository_path": "crates/vela-dev/src/record.rs",
    "url": "https://github.com/Knosence9/project-vela"
  },
  "sanitation": {"passed": true, "blockers": []},
  "trust": "curated",
  "example": {"type": "positive", "rejection_rationale": null}
}
```

Bounded values are:

- `attempts[].outcome`: `success`, `failure`, or `blocked`
- `outcome.verification[].status`: `passed`, `failed`, or `not_run`
- `trust`: `untrusted`, `reviewed`, or `curated`
- `example.type`: `positive` or `negative`

Version 1 requires non-empty task title, objective, and acceptance criteria. A verified outcome needs a passing verification. Curated evidence cannot end with a failed verification. Negative examples need an attempt diagnostic or rejection rationale. Repository paths are relative and cannot traverse parent directories, provenance URLs use HTTPS, and obvious secrets and absolute home-directory paths are rejected. Sanitation cannot pass with unresolved blockers.

## Validation CLI

```bash
nix develop --command cargo run --locked -p vela-dev -- record validate path/to/record.json
```

Exit status is `0` for valid records, `1` for records with semantic issues, and `2` for unreadable or malformed records. Semantic diagnostics are written to stderr as `<field path>: <stable code>: <message>` and all detected issues are emitted in deterministic traversal order.

## Corpus storage and inspection

Curated, project-native records are stored as JSON below `corpus/development/`. The corpus inspector recursively discovers `.json` files and processes them in sorted relative-path order:

```bash
nix develop --command cargo run --locked -p vela-dev -- corpus inspect corpus/development
```

Valid entries are listed on stdout before a final count. Record-level read, deserialization, and semantic failures are prefixed with the record's relative path on stderr; inspection continues so the summary covers the whole corpus. Exit status is `0` when every discovered record is valid, `1` when any record is invalid, and `2` when the corpus root cannot be traversed.
