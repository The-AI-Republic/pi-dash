#![forbid(unsafe_code)]

//! api-v1 assets / stickies / intake permission + throttle layer (D-21, stage 5).
//!
//! Ports the `permission_classes` / authentication / throttle lines for the
//! api-v1 surface. See [`permissions`] for the gates; sibling issues own the
//! handlers that call them.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod permissions;
