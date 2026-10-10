//! D-37 ops command groups (stage 7, PIDASHCONV-74).
//!
//! One module per command issue; groups stay in the order boot, users,
//! repair, instance, prompting so sibling rebases stay mechanical.
//!
//! * [`users`] — users + membership SQL + JSON model defaults
//!   (PIDASHCONV-807; shared with the PIDASHCONV-816 task fix).
//! * [`repair`] — data-repair SQL + row mapping (PIDASHCONV-808).
//! * [`prompting`] — `PromptTemplate` / `PromptSectionOverride` statements
//!   behind the prompting reseed + revalidate commands (PIDASHCONV-810).

pub mod prompting;
pub mod repair;
pub mod users;
