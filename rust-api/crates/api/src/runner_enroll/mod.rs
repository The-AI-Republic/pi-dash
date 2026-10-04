//! Runner enrollment/auth/machine API layer (D-13, `api/v1/runner/`).
//!
//! This module is the authentication/throttle shelf the D-13 handler ports
//! build on (PIDASHCONV-589):
//!
//! - [`auth`]: the `runner/authentication.py` extractor ports (access-token,
//!   refresh-token, machine-token) plus the consumed `APIKeyAuthentication`
//!   behaviour — fixture id D13-F4.
//! - [`throttle`]: the inherited DRF `AnonRateThrottle` (30/minute) for the
//!   enroll, redeem, and health endpoints — fixture ids D13-F4 + D13-F7.
//!
//! Kernel reuse only (`pidash_auth`, `pidash_types`), plus this module's own
//! SQL: no cross-domain code dependency.
//!
//! - [`manage`]: the web runners/machines/pods endpoints
//!   (`runner/views/runners.py:62-125,289-425`, `runner/views/pods.py`,
//!   PIDASHCONV-591) — fixture ids D13-F2 + D13-F5 + D13-F6 + D13-F7.

pub mod auth;
pub mod manage;
pub mod throttle;
