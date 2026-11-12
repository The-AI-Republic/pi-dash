//! App pages domain surface (D-30, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/page.py` for the services
//! layer, bottom-up:
//!
//! * [`shape`] — field-name consts in DRF wire order, the create/update
//!   row contracts, the binary/HTML validation rules with byte-identical
//!   error bodies, and the partial-update helper.
//! * [`queries`] — page read query builders (`get_queryset`, `summary`,
//!   archive CTE, `PageLog` lookup, duplicate re-fetch, version lookups).
//!
//! Wiring note: the crate root declares `pub mod app_pages;` (seam for
//! this issue's new files); every file under this module is new.
//!
//! Fixture input: F30-01, F30-02, F30-03
//! (`rust-api/fixtures/app_pages/serializers/*.golden.json` + `TRACE.md`);
//! the goldens are the Done-when oracles for the shape layer.
//! Fixture input for [`queries`]: F30-06, F30-07, F30-08
//! (`rust-api/fixtures/app_pages/queries/*.sql` + `.rows.json`) plus the
//! F30-12 version branches
//! (`rust-api/fixtures/app_pages/handlers/versions.golden.json`).
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook
//! (updated_at 2026-09-30T04:04:40.144056Z, binding).
//!
//! Existing bugs ported as-is (translation, don't redesign; also listed
//! in the PR):
//! 1. `label_ids` / `project_ids` carry no `read_only` flag — supplying
//!    them on create reaches `Page.objects.create(**validated_data)` and
//!    raises `TypeError`; on update they are a transient setattr no-op.
//! 2. `update()` pops `labels` then calls `super().update` with the rest
//!    only — labels never reach the model layer.
//! 3. `create()` reads `description_*` from the serializer context, not
//!    from `validated_data`.
//! 4. The `project_ids` annotation `~Q(projects__id=True)` UUID-vs-bool
//!    no-op lives in the queries layer but surfaces on this shape —
//!    noted, not fixed here.

pub mod queries;
pub mod shape;
