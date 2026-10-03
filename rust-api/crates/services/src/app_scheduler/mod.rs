//! App scheduler domain surface (D-36, stage 5).
//!
//! Ports `apps/api/pi_dash/app/views/scheduler/` for the services layer,
//! bottom-up:
//!
//! * [`occurrences`] — the occurrences calendar query units: window
//!   parsing/validation, future-bindings filter + RRULE-expansion
//!   orchestration, past-runs query, merge/sort/cap (PIDASHCONV-631).
//! * [`queries`] — scheduler + binding SQL (list/detail reads with the
//!   `_active_binding_count` annotation and `select_related` JOINs,
//!   create/update/delete writes, install/patch/uninstall, and the
//!   `next_run_at` recompute decisions with the RRULE expansion injected
//!   as a closure).
//!
//! Wiring note: the crate root declares `pub mod app_scheduler;` (seam
//! for this module's new files); every file under this module is new.
//! Sibling layer issues extend this module (on rebase, keep both sides'
//! `pub mod` lines): `shape` (scheduler + binding serializers,
//! PIDASHCONV-629) lands its line here too. The guards live in the api
//! crate (`api/src/app_scheduler/gate.rs`, PIDASHCONV-632), the endpoint
//! handlers in `api/src/app_scheduler/handlers_*.rs` (PIDASHCONV-633…
//! 635).
//!
//! Fixture input for [`occurrences`]: F36-07
//! (`rust-api/fixtures/app_scheduler/queries/occurrences_window.golden.json`)
//! and F36-08
//! (`rust-api/fixtures/app_scheduler/queries/occurrences_merge.golden.json`)
//! plus the joined-column lists in F36-06
//! (`rust-api/fixtures/app_scheduler/queries/pod_lastrun_columns.json`);
//! the goldens are the Done-when oracles. Trace lines live in
//! `rust-api/fixtures/app_scheduler/TRACE.md`.
//!
//! Fixture input for [`queries`]: F36-04, F36-05, F36-06
//! (`rust-api/fixtures/app_scheduler/queries/scheduler_sql.sql` +
//! `.rows.json`, `binding_sql.sql` + `.rows.json`,
//! `pod_lastrun_columns.json`).
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook
//! (updated_at 2026-10-02T23:02:53.761015Z, binding; PIDASHCONV-631 read
//! 2026-10-02T20:58:50.659133Z).
//!
//! Existing quirks ported as-is (translation, don't redesign; also
//! listed in the PR):
//! 1. Double `deleted_at IS NULL` guards (R6 fallback count, BR4 unique
//!    check — default manager plus the explicit filter).
//! 2. `ORDER BY created_at DESC` on single-row detail lookups
//!    (`Meta.ordering`); no `ORDER BY` on the annotated R2 detail.
//! 3. The binding-list `pod` LEFT JOIN has no tombstone filter — a
//!    soft-deleted pod still joins and renders stale `pod_name`.
//! 4. The R5 cascade `QuerySet.update()` bypasses `auto_now` (bindings'
//!    `updated_at` untouched).
//! 5. `delete()` samples `now()` twice (`:now` + `:now2`).
//! 6. Install-vs-patch `next_run_at` write asymmetry (install compares
//!    to stored, patch does not) with patch recomputing on key
//!    presence.

pub mod occurrences;
pub mod queries;
