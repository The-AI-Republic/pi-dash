//! App asset surface (D-31): S3/MinIO presigned flows.
//!
//! [`guards`] ports the permission gates and the asset throttle key
//! (`apps/api/pi_dash/app/views/asset/v2.py` gate lines,
//! `apps/api/pi_dash/app/views/asset/base.py` default auth,
//! `apps/api/pi_dash/throttles/asset.py`). Handler routes (issues
//! 394/400/412) call into it; this module owns no routes yet.

pub mod guards;
