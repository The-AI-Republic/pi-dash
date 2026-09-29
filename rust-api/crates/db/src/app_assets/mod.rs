//! File-asset app domain surface (D-31, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/asset.py` for the db layer:
//!
//! * [`columns`] — `FileAsset` table/column/index consts, the
//!   `EntityTypeContext` value list, and the pure helpers
//!   (`get_upload_path`, `file_size`, `asset_url`, `__str__`) as a
//!   read-only reference for query builders (PIDASHCONV-333).
//!   Reads, serializers, guards, tasks and handlers belong to the
//!   sibling D-31 issues; the domain gate is PIDASHCONV-420.

pub mod columns;
