#![forbid(unsafe_code)]

//! Project scheduler API surface (`app/views/scheduler/`, D-36).
//!
//! Ports the serializers (`app/serializers/scheduler.py:1-268`): the
//! scheduler + binding field lists, read-only sets, derived-field rules,
//! and every pure validator (color, rrule via an injected verdict
//! closure, rdates/exdates ISO lists, tzid, extra_context, and the
//! cross-field scheduler/project lock, rrule+dtstart re-check, and
//! pod-must-belong-to-project check).
//!
//! Fixture oracles live under
//! `rust-api/fixtures/app_scheduler/serializers/` (`TRACE.md` maps every
//! file to its Python source lines); the shape tests replay F36-01..03
//! byte-identically.
//!
//! The queries layer (`queries.rs`, PIDASHCONV-630) owns every SQL
//! statement plus the `next_run_at` recompute decisions; the guards
//! (`gate.rs`, PIDASHCONV-632) and the handlers (PIDASHCONV-633..635)
//! complete the domain.

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
pub mod shape;

pub use shape::{
    binding_read_only_fields, extra_context_error, field_error_body, iso_item_parse_error,
    iso_item_type_error, iso_list_too_long, json_type_name, last_run_ended_at, last_run_status,
    lock_error, nested_field_error_body, normalize_iso_datetime, pod_display_name,
    resolve_active_binding_count, tzid_unknown_error, validate_color, validate_cross_rrule,
    validate_extra_context, validate_iso_datetime_list, validate_locked_field,
    validate_pod_project, validate_rrule, validate_tzid, IsoListError, TzidError,
    BINDING_DECLARED_READ_ONLY_FIELDS, BINDING_META_READ_ONLY_FIELDS, BINDING_SERIALIZER_FIELDS,
    COLOR_ERROR, EXTRA_CONTEXT_MAX_LENGTH, ISO_LIST_NOT_ARRAY, POD_PROJECT_ERROR,
    RDATE_EXDATE_MAX_LENGTH, SCHEDULER_READ_ONLY_FIELDS, SCHEDULER_SERIALIZER_FIELDS,
};
