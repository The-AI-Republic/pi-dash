//! Public operation services: the serializer step of the v1 write
//! endpoints (OVL-14, PIDASHCONV-1649).
//!
//! The Python cloud overlay calls OSS serializers directly
//! (`IssueSerializer(...).is_valid()` → `.save()` → render); the Rust
//! overlay re-ported them because the step lived inside private v1 handler
//! functions. Each operation here is that step, lifted verbatim: validate
//! the payload, apply defaults, call the [`domain`](crate::domain) write,
//! render the response. Writes go through the existing `domain::*`
//! services; no new SQL lives here.
//!
//! The v1 handler stays responsible for HTTP and auth (preamble, gate,
//! route params, settings) and becomes parse request → call operation →
//! wrap response. Each operation takes the actor and scope ([`WriteCtx`]),
//! the request payload as the v1 endpoint receives it (raw bytes plus the
//! content type), and the options its callers need as explicit parameters.
//! It returns the rendered body or the DRF-shaped field errors, so callers
//! produce byte-identical responses without re-implementing validation.
//!
//! [`WriteCtx`]: crate::domain::WriteCtx

pub mod comments;
mod common;
pub mod deps;
pub mod error;
pub mod issues;
pub mod pull_requests;
pub mod relations;
pub mod workpad;

pub use common::{requested_data_text, requested_data_value};
pub use deps::Deps;
pub use error::{Error, Outcome};
