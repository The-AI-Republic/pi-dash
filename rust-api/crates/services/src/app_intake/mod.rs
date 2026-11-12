//! App intake domain surface (D-32, stage 5).
//!
//! Ports `apps/api/pi_dash/app/views/intake/base.py` for the services
//! layer, bottom-up:
//!
//! * [`permissions`] — role matrices, guest scoping, creator gates,
//!   default-intake delete guard and destroy cascade (the guard units).
//! * [`shape`] — `IntakeSerializer` (17-24) and `IntakeIssueSerializer`
//!   (27-90) from `apps/api/pi_dash/app/serializers/intake.py:17-90`
//!   (PIDASHCONV-281). Sibling issue PIDASHCONV-282 appends the remaining
//!   three units (`intake.py:93-139`) to [`shape`].
//! * [`tasks`] — the five `issue_activity` / `issue_description_version_task`
//!   `.delay()` emits from `app/views/intake/base.py` (PIDASHCONV-345):
//!   Celery-format kwargs in call-site order plus the migration-update
//!   silent-path predicate. Handlers (PIDASHCONV-385/395) wrap them into
//!   queue rows; no worker `Registry` handler is needed (Python-owned
//!   names forward to the broker).
//!
//! Wiring note: the crate root declares `pub mod app_intake;` (seam for
//! this issue's new files); every file under this module is new.
//!
//! Ported bugs (also listed in the PR): none in this range. The
//! `validate` None-instance path (`self.instance` is `None` on create)
//! is unreachable: the create view builds the row with
//! `IntakeIssue.objects.create(...)` directly (`base.py:267-273`) and
//! never runs this serializer, so only `partial_update` (instance always
//! set) reaches `validate`/`update`.
pub mod permissions;
pub mod shape;
pub mod tasks;

pub use shape::{
    accepted_issue_transition, apply_label_ids_annotation, no_default_state_error_body,
    validate_status_transition, IntakeIssueRecord, IntakeRecord, INTAKE_ISSUE_FIELDS,
    INTAKE_ISSUE_READ_ONLY_FIELDS, INTAKE_ISSUE_STATUS_ACCEPTED, INTAKE_ISSUE_STATUS_DUPLICATE,
    INTAKE_ISSUE_STATUS_PENDING, INTAKE_ISSUE_STATUS_REJECTED, INTAKE_ISSUE_STATUS_SNOOZED,
    INTAKE_KEY_ORDER, INTAKE_READ_ONLY_FIELDS, ISSUE_STATE_GROUP_TRIAGE, NO_DEFAULT_STATE_MESSAGE,
};
