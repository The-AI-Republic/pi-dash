# Dev-machine run queue: implementation handoff

**Status: specification ready; feature not implemented.** Baseline: `pi-dash` main `24b546d2d`.

Read in this order:

1. [design.md](design.md) — behavior, architecture, current-code evidence and acceptance matrix.
2. [implementation.md](implementation.md) — fixed storage, wire, lifecycle, recovery and compatibility contracts. This governs concrete implementation choices if the architectural overview is less specific.
3. [tasks.md](tasks.md) — dependency-ordered implementation checklist and completion criteria.
4. [review.md](review.md) — review findings and why the required safeguards exist.

The feature preserves cloud runner selection and adds a durable FIFO execution queue in the shared dev-machine daemon. It defaults to three concurrent issue runs, configurable live in the TUI. Assigned waiting runs keep their runners busy. Interactive chats on other runners, automatic resource sizing and cross-daemon scheduling are outside this version.

## Prompt for the implementing agent

> Implement the dev-machine AgentRun queue end to end using the specification in `.ai_design/dev_machine_run_queue/`. Read README.md, design.md, implementation.md, tasks.md and review.md before editing code. Reconcile entry points with the current branch, then execute T1–T8 in dependency order. Preserve cloud assignment and the default limit of three. Implement both Python and Rust backend contracts, daemon durability and process guards, shared IPC/TUI live configuration, and cloud waiting presentation. Add and run the required contract, crash/race, UI and native-platform tests. Update tasks.md only with work actually completed and include validation evidence. Do not replace specified recovery or authorization checks with optimistic fallbacks. Ordinary library/module choices are yours; report any discovered contradiction that changes product behavior or ownership guarantees. Do not stop after a local queue prototype or mark missing platform coverage as complete. No deployment or publishing is included.

The design has no pending product-choice questions. Platform validation, migration tests and fault-injection checks are implementation obligations; their results are not claimed in this documentation.
