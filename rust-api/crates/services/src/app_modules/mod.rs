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
//! * [`models`] — models A (PIDASHCONV-355): `ModuleStatus`, `Module`
//!   and `ModuleMember` (`db/models/module.py:58-151`) as structs +
//!   column defs, replaying FX-MOD-01. Sibling issue PIDASHCONV-363
//!   appends models B (`:152-217`) in its own sections.
//!
//! Wiring note: the crate root declares `pub mod app_modules;` (seam for
//! this issue's new files); every file under this module is new.
//!
//! Ported bugs (also listed in the PR): PUT has no custom action so it
//! renders the bare write shape, not the annotated row; member replace on
//! update soft-deletes instead of hard-deleting; duplicate-name checks
//! race outside a transaction; `validate` only sees input dates, so a
//! partial update carrying one date skips the start/target check.
pub mod models;
pub mod shape;

pub use shape::{
    date_violation_body, duplicate_link_body, duplicate_link_update_body, duplicate_name_body,
    init_expand_append, invalid_url_body, link_field_error_body, member_ids_representation,
    normalize_link_url, resolve_expand_render, should_replace_members, userprops_patch_value,
    validate_link_url_present, validate_module_dates, ExpandRender, DRF_INVALID_URL_MESSAGE,
    DRF_URL_REQUIRED_MESSAGE, DUPLICATE_LINK_MESSAGE, DUPLICATE_LINK_UPDATE_MESSAGE,
    DUPLICATE_NAME_MESSAGE, DYNAMIC_FIELDS_KWARG_HONORED, FILTER_EXPANSION_KEYS,
    FILTER_EXPANSION_MANY, FLAT_KEY_ORDER, FLAT_READ_ONLY_FIELDS, INVALID_URL_MESSAGE,
    LINK_KEY_ORDER, LINK_READ_ONLY_FIELDS, MEMBER_BULK_BATCH_SIZE, MEMBER_BULK_IGNORE_CONFLICTS,
    MODULE_DETAIL_EXTRA_FIELDS, MODULE_ISSUE_KEY_ORDER, MODULE_ISSUE_NESTED_FIELDS,
    MODULE_ISSUE_READ_ONLY_FIELDS, MODULE_LIST_FIELD_ORDER, MODULE_WRITABLE_FIELDS,
    REPRESENTATION_EXPANSION_KEYS, START_AFTER_TARGET_MESSAGE, USERPROPS_KEY_ORDER,
    USERPROPS_PATCH_STATUS, USERPROPS_READ_ONLY_FIELDS, WRITE_FIELD_ORDER, WRITE_READ_ONLY_FIELDS,
    WRITE_RESPONSE_ORDER,
};
