#![forbid(unsafe_code)]

//! App workspace guard seam (D-24, stage 5).
//!
//! [`gates`] ports the `@allow_permission` role matrix, the workspace
//! permission classes, the throttle wiring, and the cache/header
//! decorators (F-W24-13, PIDASHCONV-613) over the F-06 kernel for the
//! D-24 handler issues (PIDASHCONV-615…624) to decide through.
//!
//! Route registration and handlers belong to those handler issues;
//! this module only declares the guard layer they wire through, not a
//! stub — sibling issues extend it; merges keep both sides.

pub mod gates;
pub mod handlers_prefs;
