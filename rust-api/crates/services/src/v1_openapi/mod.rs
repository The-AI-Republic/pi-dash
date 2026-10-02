#![forbid(unsafe_code)]

//! api-v1 OpenAPI schema doc vocabulary (D-23, stage 5).
//!
//! Ports `apps/api/pi_dash/settings/openapi.py` (`SPECTACULAR_SETTINGS`) and
//! `apps/api/pi_dash/utils/openapi/{auth,hooks}.py` for the services layer:
//!
//! * [`meta`] — doc-meta consts + API-key auth scheme (PIDASHCONV-530).
//! * [`hooks`] — the 3 schema hook functions (PIDASHCONV-530).
//!
//! Wiring note: the crate root declares `pub mod v1_openapi;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! issues add their own siblings to this `mod.rs`; on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod hooks;
pub mod meta;
