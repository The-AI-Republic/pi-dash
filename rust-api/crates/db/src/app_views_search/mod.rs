//! App views + search table models (D-29, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/view.py` (`IssueView`) and
//! `apps/api/pi_dash/db/models/favorite.py` (`UserFavorite`), adopting the
//! Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover.
//!
//! * [`models`] — `IssueView`, `UserFavorite` (struct + column/constraint
//!   mapping only) plus the `Issue` / `IssueComment` columns this domain
//!   reads with their FTS index expression text (read-only reference; the
//!   owning domains port the full structs). Reads, serializers, guards,
//!   tasks and handlers belong to PIDASHCONV-269/271/272; the domain gate
//!   is PIDASHCONV-277.

pub mod models;
