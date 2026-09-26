#![forbid(unsafe_code)]

//! Authentication and authorization primitives (F-05).
//!
//! Django-parity layer over the mechanisms every ported domain needs:
//!
//! - [`signing`]: Django's TimestampSigner codec, shared by sessions.
//! - [`session`]: DB-session reader (cookie routing, key screening,
//!   `session_data` decoding, expiry predicate) plus the F-08 response
//!   half's pure pieces: `Set-Cookie` rendering, `http_date`, cookie
//!   parsing, and session-key generation.
//! - [`password`]: PBKDF2 password-hash verification.
//! - [`token`]: `X-Api-Key` routing plus `APIToken` / `MachineToken` row
//!   predicates, with the runner pepper hash and fingerprint helpers.
//! - [`jwt`]: runner access-token (JWT) verification.
//! - [`csrf`]: CSRF mask / match / format semantics plus the
//!   `get-csrf-token` endpoint behaviour.
//! - [`scope`]: tenant scope both auth and the F-06 permission kernel deny on.
//! - [`permissions`]: the F-06 permission kernel — every access rule the
//!   Django backend enforces (`app`/`utils`/`core` permissions, runner,
//!   managed-runner desktop gate, license console gate, `@allow_permission`).
//!
//! The modules are pure: row fetching stays the caller's SQL, so the crate
//! performs no I/O and every check is unit-testable against Django-issued
//! vectors.

pub mod csrf;
pub mod jwt;
pub mod password;
pub mod permissions;
pub mod scope;
pub mod session;
pub mod signing;
pub mod token;

pub use scope::TenantScope;
