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
//!   (PIDASHCONV-387). Later layer issues append their units here.
//!
//! Wiring note: the crate root declares `pub mod app_assets;` (seam for
//! this issue's new files); every file under this module is new.
pub mod shape;
pub mod tasks;

pub use shape::{
    is_read_only_field, row_to_json, strip_read_only_fields, FileAssetRecord, FILEASSET_KEY_ORDER,
    FILEASSET_READ_ONLY_FIELDS,
};
