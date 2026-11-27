# Dev-machine run queue: implementation handoff

**Status: Rust-only backend specification ready; feature not implemented by this documentation.** Scope updated 2026-11-15 against OSS `5852f02e7` and private `d3ad2a3`.

Read in this order:

1. [design.md](design.md) — behavior, architecture, current-code evidence and acceptance matrix.
2. [implementation.md](implementation.md) — fixed storage, wire, lifecycle, recovery and compatibility contracts. This governs concrete implementation choices if the architectural overview is less specific.
3. [tasks.md](tasks.md) — dependency-ordered implementation checklist and completion criteria.
4. [review.md](review.md) — review findings and why the required safeguards exist.

The feature preserves cloud runner selection and adds a durable FIFO execution queue in the shared dev-machine daemon. It defaults to three concurrent issue runs, configurable live in the TUI. Assigned waiting runs keep their runners busy. Interactive chats on other runners, automatic resource sizing and cross-daemon scheduling are outside this version.

**Backend scope:** Rust only. Production schema changes go through the native SeaORM migrator in `private-pi-dash/rust-cloud/crates/cloud-migration/`; runtime changes go in OSS `rust-api/`. The daemon/IPC/TUI and `apps/web_new` remain included. Python runtime, Django models/migration files and old-web presentation are excluded. Historical Django baseline/source checks stay intact; they are not a requirement to build this feature in Python. This revision supersedes the earlier dual-backend checklist and the Python-first issue split; issue descriptions/statuses must be reconciled separately and have not been changed by this documentation update.

## Prompt for the implementing agent

> Implement the dev-machine AgentRun queue end to end using the specification in `.ai_design/dev_machine_run_queue/`. Read README.md, design.md, implementation.md, tasks.md and review.md before editing code. Reconcile entry points with the current branch, then execute T1–T8 in dependency order. Preserve cloud assignment and the default limit of three. Implement the Rust backend contract, native cloud migration(s) in private-pi-dash/rust-cloud, daemon durability and process guards, shared IPC/TUI live configuration, and new-frontend waiting presentation. Do not implement Python runtime behavior, Django model/migration files, or old-web UI for this feature. Add and run the required contract, crash/race, UI and native-platform tests. Update tasks.md only with work actually completed and include validation evidence. Do not replace specified recovery or authorization checks with optimistic fallbacks. Ordinary library/module choices are yours; report any discovered contradiction that changes product behavior or ownership guarantees. Do not stop after a local queue prototype or mark missing platform coverage as complete. No deployment or publishing is included.

The design has no pending product-choice questions. Platform validation, migration tests and fault-injection checks are implementation obligations; their results are not claimed in this documentation.
