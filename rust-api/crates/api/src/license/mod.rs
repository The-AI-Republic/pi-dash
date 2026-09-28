//! License / instance-console form handlers (D-01, stage 3).
//!
//! Ports `apps/api/pi_dash/license/api/views/` for the api layer. Sibling
//! handler issues own their files and consume shared pieces read-only:
//!
//! * [`handlers_auth_forms`] — admin sign-up + sign-in form-POST redirect
//!   flows (PIDASHCONV-122; `admin.py:89-358`).
//! * `handlers_instance` — instance + signup-visited (PIDASHCONV-120; also
//!   owns the shared `base.py` handler base).
//! * `handlers_admin` — admins CRUD + me/session/sign-out (PIDASHCONV-121;
//!   `admin.py:44-86` admins CRUD and `admin.py:361-398` me/session/sign-out).
//! * `handlers_config_workspace` — configuration + workspace (PIDASHCONV-123).
//!
//! This module is new-files-only: the `pub mod license;` wiring in
//! `crate::lib`, route registration in the License route group, and the
//! CSRF strategy for unsafe methods land in a separate wiring issue (the
//! F-10 seam), exactly like the auth-license wiring tracked separately for
//! PIDASHCONV-118. Until then every path here still proxies to Django, so
//! the token-less POST contract (`test_auth_forms.py`: HTTP 200 with the
//! CSRF-failure page) is preserved byte for byte.

pub mod handlers_admin;
pub mod handlers_auth_forms;
