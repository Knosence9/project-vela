# Kernel slice — State directory bootstrap and continuity foundation

## Behavior target
The Rust runtime creates and uses state directories with semantics that support a persistent agentic OS core.

## Reference input
- source: startup/bootstrap and persistence setup in the prior system
- flow: first run, repeated run, missing directory, partial state cases
- notes: this slice is now about continuity readiness, not only directory compatibility

## Rust target
- crate/module: state bootstrap layer
- state surface: directory creation, initialization, and handoff into persistence services

## Checklist
- [ ] contract understood
- [ ] first-run behavior captured
- [ ] repeat-run behavior captured
- [x] initial directory bootstrap scaffold exists
- [ ] verify handoff into session/event persistence
- [ ] verify behavior under partial or damaged state cases
- [ ] add proof notes

## Next proof target
Show that state bootstrap is good enough to support:
- session continuity
- reloadable runtime metadata
- future scheduler/memory/plugin persistence
