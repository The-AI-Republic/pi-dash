//! App asset surface (D-31): S3/MinIO presigned flows.
//!
//! [`guards`] ports the permission gates and the asset throttle key
//! (`apps/api/pi_dash/app/views/asset/v2.py` gate lines,
//! `apps/api/pi_dash/app/views/asset/base.py` default auth,
//! `apps/api/pi_dash/throttles/asset.py`). [`handlers_v1`] serves the
//! legacy v1 routes (PIDASHCONV-394);
//! [`handlers_v2_user_workspace`] (PIDASHCONV-400) serves the v2
//! user/workspace/static/restore family;
//! [`handlers_v2_project`] serves the v2 project-side routes
//! (`app/urls/asset.py:79-113`, PIDASHCONV-412); sibling handler issues
//! extend [`routes`] with their own routers, keeping both sides.

pub mod guards;
pub mod handlers_v1;
pub mod handlers_v2_project;
pub mod handlers_v2_user_workspace;

use axum::Router;

use crate::state::AppState;

/// `urllib.parse.quote(filename)` (default `safe='/'`) — the encoder
/// `S3Storage._get_content_disposition` uses (`storage.py:115-124`).
/// It is the RFC 3986 unreserved set (`-_.~`) plus `/`, so a stored
/// `attributes.name` that contains a slash keeps it: the disposition
/// signs `attachment; filename*=UTF-8''a/b.png`, not `…a%2Fb.png`.
/// SigV4 canonical/query encoding must NOT use this (it stays strict,
/// see each handler's `uri_encode`).
pub(crate) fn quote_filename(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0x0f) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

/// Merge the app-asset route groups (v1 first, then v2
/// user/workspace/static/restore, then v2 project/bulk/check/duplicate/
/// downloads; sibling handler issues extend the merge; merges keep both
/// sides).
pub fn routes() -> Router<AppState> {
    handlers_v1::routes()
        .merge(handlers_v2_user_workspace::routes())
        .merge(handlers_v2_project::routes())
}
