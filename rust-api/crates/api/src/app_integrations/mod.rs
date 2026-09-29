//! D-33 app-integrations permission layer (stage 5, PIDASHCONV-436).
//!
//! [`gates`] ports the `allow_permission` role matrix, the two `AllowAny`
//! endpoints, and the manual admin checks; [`hmac`] ports the
//! `verify_webhook_signature` HMAC-SHA256 guard. Both modules are pure:
//! handlers fetch membership rows through the workspace-scoped handle and
//! decide here.

pub mod gates;
pub mod hmac;
