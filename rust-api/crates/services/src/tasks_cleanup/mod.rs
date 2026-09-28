//! D-09 issue/analytics export + expiry logic (tasks layer).
//!
//! Ports `apps/api/pi_dash/bgtasks/export_task.py`,
//! `exporter_expired_task.py` and `analytic_plot_export.py` for the
//! services layer: CSV sanitising + writing, segmented/non-segmented row
//! builders, ZIP construction, S3 key/branch protocol, expiry planning
//! and the mail-payload/orchestration decisions. Every function is pure
//! over injected snapshots: no database, no S3, no SMTP.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-1 (`analytic_plot_export.py:278-282`): MODULE segment headers
//!   resolve against `label_details` (not `module_details`) with
//!   module-id keys, so they never match and stay raw. Kept as-is;
//!   [`generate_segmented_rows`] takes `label_details` for that branch.
//! * BUG-2 (`:217`): a missing segment cell renders as the STRING `"0"`,
//!   not the number `0`. Kept as-is.
//! * BUG-3 (`exporter_expired_task.py:48-53`): the MINIO/non-MINIO
//!   `delete_object` branches are identical calls, and the row's `url`
//!   is cleared even when `key` was falsy (no S3 call happened).
//!   [`plan_expiry_deletes`] keeps both.
//!
//! Deliberate deviation (documented, not a bug): `segment_zero`
//! (`:200`) is `list(set(...))` in Python — nondeterministic across runs
//! (hash seed). The port keeps first-seen order over the same multiset,
//! which replays every recorded golden and is stable under test.
//!
//! D-09 cleanup retention + mongo flush task logic.
//!
//! Port of the task wiring of `apps/api/pi_dash/bgtasks/cleanup_task.py`
//! (`:92-:163`, `:422-:479`).
//!
//! * [`workspace_seed`] — `bgtasks/workspace_seed_task.py` (PIDASHCONV-190).
//!
//! Business logic for the D-09 cleanup domain (stage 5).
//!
//! Ports the orchestration half of
//! `apps/api/pi_dash/bgtasks/deletion_task.py` (PIDASHCONV-186): the
//! soft-delete relation walk, the hard-delete driver, and the unregistered
//! restore function. SQL shapes and catalog discovery live in `pidash-db`
//! (`tasks_cleanup::deletion_queries`); worker registration lives in
//! `pidash-jobs` (`tasks_cleanup::deletion`).

pub mod assets;
pub mod cleanup;
pub mod deletion;
// D-09 bgtasks: cleanup, versions, exports, deletion (task layer).
// The `dummy_data` submodule below owns one Python task file's counts,
// literals, ordering and payload shape; the jobs-layer `tasks_cleanup`
// module owns execution and worker registration. Wiring note: the crate
// root declares `pub mod tasks_cleanup;` (foundation change, tracked
// separately).

pub mod dummy_data;
pub mod exports;
pub mod workspace_seed;

pub use deletion::{
    hard_delete, parse_soft_delete_call, restore_related_objects, soft_delete_related_objects,
    DeletionError, HardDeleteOutcome, SoftDeleteOutcome, SoftDeleteTarget,
};

// D-09 version-task decisions (stage 5, PIDASHCONV-188): pure logic behind
// `issue_version_sync.py`, `issue_description_version_sync.py`,
// `issue_description_version_task.py` and `page_version_task.py`.
pub mod versions;
