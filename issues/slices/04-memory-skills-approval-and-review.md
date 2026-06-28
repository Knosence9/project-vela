# Kernel slice — Memory/skills approval staging and background review

## Behavior target
The Rust runtime can propose durable memory and procedural skill changes without applying them immediately.

## Reference input
- source: Hermes-style gated self-modification and learning control
- flow: candidate suggestion -> pending approval -> approve/reject
- notes: background analyzers should emit structured signals instead of writing directly

## Rust target
- crates/modules: `vela-memory`, `vela-skills`, `vela-review`, `vela-state`, `vela-runtime`, `apps/vela`
- state surface: `pending/`, `reviews/candidates/`, `session_events`, CLI review flows

## Checklist
- [x] memory writes can be staged before apply
- [x] skill writes can be staged before apply
- [x] pending items can be listed, shown, approved, and rejected
- [x] background review candidates can be stored separately from pending approvals
- [x] latest-session transcript/event inspection can drive review candidate generation
- [x] structured `memory_signal` and `skill_signal` events can be emitted
- [x] end-to-end auto pass can emit signals and derive review candidates
- [ ] verify richer dedupe and conflict handling across repeated sessions
- [ ] add proof notes for safe autonomous learning loops

## Next proof target
Show that Vela can:
- learn candidate preferences without mutating memory immediately
- capture candidate procedures as reviewable skills
- preserve operator control over every durable change
