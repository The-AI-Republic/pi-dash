//! App modules domain surface (D-28, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/module.py` for the services
//! layer, bottom-up:
//!
//! * [`shape`] — part A (PIDASHCONV-330): `ModuleWriteSerializer`
//!   (`module.py:26-120`), `ModuleFlatSerializer` (`:123-134`) and
//!   `ModuleIssueSerializer` (`:137-153`). Sibling issue PIDASHCONV-313
//!   appends part B (`module.py:156-280`: link / detail / userprops) to
//!   [`shape`].
//!
//! Wiring note: the crate root declares `pub mod app_modules;` (seam for
//! this issue's new files); every file under this module is new.
//!
//! Ported bugs (also listed in the PR): PUT has no custom action so it
//! renders the bare write shape, not the annotated row; member replace on
//! update soft-deletes instead of hard-deleting; duplicate-name checks
//! race outside a transaction; `validate` only sees input dates, so a
//! partial update carrying one date skips the start/target check.
pub mod shape;

pub use shape::{
    date_violation_body, duplicate_name_body, member_ids_representation, should_replace_members,
    validate_module_dates, DUPLICATE_NAME_MESSAGE, FLAT_KEY_ORDER, FLAT_READ_ONLY_FIELDS,
    MEMBER_BULK_BATCH_SIZE, MEMBER_BULK_IGNORE_CONFLICTS, MODULE_ISSUE_KEY_ORDER,
    MODULE_ISSUE_NESTED_FIELDS, MODULE_ISSUE_READ_ONLY_FIELDS, START_AFTER_TARGET_MESSAGE,
    WRITE_FIELD_ORDER, WRITE_READ_ONLY_FIELDS, WRITE_RESPONSE_ORDER,
};
