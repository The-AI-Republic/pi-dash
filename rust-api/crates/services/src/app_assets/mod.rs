//! App assets domain surface (D-31, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/asset.py` for the services
//! layer, bottom-up:
//!
//! * [`shape`] — `FileAssetSerializer` (`asset.py:9-13`): the 24-key golden
//!   shape over `FileAsset` (`db/models/asset.py:45-62` plus audit columns),
//!   the `Meta.read_only_fields` set, and the `row_to_json` renderer
//!   (PIDASHCONV-319).
//! * [`tasks`] — unuploaded-asset sweep query + metadata publisher payloads
//!   (`bgtasks/file_asset_task.py`, `bgtasks/storage_metadata_task.py`
//!   publisher call sites at `app/views/asset/v2.py:177,386,587`)
//!   (PIDASHCONV-387).
//! * [`queries_v1`] — legacy v1 asset reads/writes
//!   (`app/views/asset/base.py:1-86`): workspace/user filter/get builders,
//!   the workspace-slug lookup, the `is_deleted` flip, and the
//!   status/body contract per outcome (PIDASHCONV-346). Later layer issues
//!   append their units here.
//! * [`queries_v2_project`] — v2 project-side query closure
//!   (`app/views/asset/v2.py:432-835`): entity-field maps, scoped SQL,
//!   bulk dispatch, duplicate/download shapes (PIDASHCONV-361).
//! * [`queries_v2_user_workspace`] — v2 user + workspace asset queries
//!   (`UserAssetsV2Endpoint`, `v2.py:29-198`; `WorkspaceFileAssetEndpoint`,
//!   `v2.py:201-429`): entity-field mapping, lookups, create sets,
//!   confirm/delete updates, entity-link actions, post-commit
//!   invalidations, metadata predicate (PIDASHCONV-356). Later layer
//!   issues append their units here.
//!
//! Wiring note: the crate root declares `pub mod app_assets;` (seam for
//! this issue's new files); every file under this module is new.
pub mod queries_v1;
pub mod queries_v2_project;
pub mod queries_v2_user_workspace;
pub mod shape;
pub mod tasks;

pub use queries_v1::{
    miss_body, not_found_body, set_deleted_sql, unhandled_body, user_filter_sql, user_get_sql,
    workspace_asset_key, workspace_filter_sql, workspace_get_sql, workspace_lookup_sql,
    DELETE_STATUS, FOUND_STATUS, MISS_STATUS, NOT_FOUND_STATUS, POST_STATUS, RESTORE_STATUS,
    USER_GET_FOUND_STATUS,
};
pub use shape::{
    is_read_only_field, row_to_json, strip_read_only_fields, FileAssetRecord, FILEASSET_KEY_ORDER,
    FILEASSET_READ_ONLY_FIELDS,
};
