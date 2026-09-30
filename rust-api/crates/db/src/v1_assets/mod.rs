#![forbid(unsafe_code)]

//! api-v1 assets / stickies / intake model layer (D-21, stage 5).
//!
//! Ports the column lists, defaults, constraints, manager scopes, and
//! save-rule helpers for the four tables the D-21 views touch, adopting
//! the Django-owned schema column-for-column. Migrations are not ported;
//! Django stays schema owner until switchover.
//!
//! * [`model`] — `FileAsset` / `Sticky` / `Intake` / `IntakeIssue` columns,
//!   enums, row structs, and the `Sticky.save` helpers (PIDASHCONV-404).
//! * [`entities`] — generated HTML5 reference table backing
//!   [`model::sticky::unescape`]; do not hand-edit.
//! * [`intake_queries`] — intake queryset reads (PIDASHCONV-411,
//!   fixture fx-q-intake).
//!
//! Sources (`apps/api/`): `pi_dash/db/models/asset.py:28-110`
//! (`FileAsset`, `EntityTypeContext`, `asset_url`); `pi_dash/db/models`
//! `/sticky.py:16-60` (`Sticky`, `save`); `pi_dash/db/models/intake.py:12-80`
//! (`Intake`, `SourceType`, `IntakeIssueStatus`, `IntakeIssue`); audit
//! columns `pi_dash/db/mixins.py:16-89`, UUID pk
//! `pi_dash/db/models/base.py:17-21`.
//!
//! Fixtures: `rust-api/fixtures/v1_assets/fx-model-{fileasset,sticky,
//! intake}.json`. Column order in each `COLUMNS` const is Django `_meta`
//! field order: `id`, audit cols, `project_id`/`workspace_id` for
//! `ProjectBaseModel` children, then declaration order with Django
//! attnames for FKs (`user_id`, `workspace_id`, …).
//!
//! Reads serve from the per-table soft-delete views (`<table>_active`,
//! [`crate::soft_delete::active_view_ddl`]) wherever Django uses its
//! default managers; writes hit the tables so the partial unique indexes
//! keep working. Every application-level default below must be supplied
//! explicitly on insert — the live tables carry no `column_default`.
//!
//! Plane note: the same physical tables are also ported under the `app`
//! plane (`file_assets` in [`crate::app_assets`], `intakes` /
//! `intake_issues` in [`crate::app_intake`]). The `app` modules serve the
//! Plane-fork views; this module serves the api-v1 views. Both describe
//! the same Django-owned schema; if the schema drifts, both need the same
//! update (the D-21 domain gate owns drift).
//!
//! Wiring note: the crate root declares `pub mod v1_assets;` (seam added
//! by PIDASHCONV-404); every file under this module is new. Sibling issues
//! add their own layers under sibling modules (PIDASHCONV-409/411
//! queries); on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod entities;
pub mod intake_queries;
pub mod model;
