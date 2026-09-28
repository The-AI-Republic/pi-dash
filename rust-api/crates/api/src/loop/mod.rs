#![forbid(unsafe_code)]

//! Loop auto-pm domain module (D-03).
//!
//! [`guards`] ports the toggle-only body guard, the admin write validator,
//! the slug-taken predicates, and the `InstanceAdminPermission` wiring
//! reference (`pi_dash/loop/views.py`, `pi_dash/loop/admin_views.py`,
//! `pi_dash/license/api/permissions/instance.py`). Handlers live in later
//! issues; they own the SQL, the routes, and the HTTP mapping.

pub mod guards;
