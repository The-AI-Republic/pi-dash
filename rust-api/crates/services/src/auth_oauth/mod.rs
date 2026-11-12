//! D-17 authentication (OAuth providers + error codes), shapes layer.
//!
//! Ports the pure constructor/mapping parts of
//! `apps/api/pi_dash/authentication/provider/oauth/` (`google.py:115`,
//! `github.py:183`, `gitlab.py:124`, `gitea.py:173` lines) and the oauth
//! rows + exception of `authentication/adapter/error.py:41-50,77-92`
//! (PIDASHCONV-325):
//!
//! * [`providers`] — auth-url builders, token POST bodies, stored
//!   token/user-data mappers, github/gitea email selectors (AUTHOAUTH-F3/F4).
//! * [`error`] — oauth error-code table, provider error selector,
//!   `AuthenticationException` + error-dict renderer (AUTHOAUTH-F5).
//! * [`exchange`] — token/userinfo fetch error mapping per provider,
//!   github email + org-gate branches (PIDASHCONV-327, AUTHOAUTH-F4/F5).
//!
//! Out of scope (sibling issues): account upsert + device helpers (db
//! queries), views (handlers), models.

pub mod error;
pub mod exchange;
pub mod providers;
