//! Authentication session/email/magic/password/CSRF model semantics (D-16, stage 5).
//!
//! Ports the model layer named by PIDASHCONV-370:
//!
//! * [`models`] — `Session` + `SessionStore` (`db/models/session.py`) and
//!   the `User` auth columns (`db/models/user.py:56-137`), as pure
//!   functions over row/JSON types. No tables are created here and no
//!   database handle is held: Django stays the schema owner (no
//!   migrations), and the SQL read/write half lives with the queries
//!   layer (PIDASHCONV-382).

pub mod models;
