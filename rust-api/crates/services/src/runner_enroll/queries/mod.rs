//! Daemon-enrollment read/write queries (D-13, stage 5).
//!
//! Ports the SQL statement sets Django emits for the daemon-enrollment
//! units in `runner/views/enrollment.py`. Each builder returns the SQL text
//! with Postgres `$N` binds (Django renders `%s`; same binding order) and
//! documents its `$N` params in first-appearance order. Execution belongs
//! to the handlers layer (PIDASHCONV-590/592), which binds the documented
//! params, owns the `BEGIN`/`COMMIT`/`SAVEPOINT` boundaries, and maps row
//! counts onto the `.get()`/`.first()` contracts.
//!
//! * [`enroll_reads`] — enroll tx (E1-E2), refresh row-lock (E3-E5),
//!   create-endpoint reads + insert (E6-E7), token bootstrap/rotate
//!   (B1-B3), dev-machine get-or-create (D1-D4)
//!   (`runner/views/enrollment.py:57-203,284-467,597-735`, PIDASHCONV-582).
//! * [`catalog_reads`] — pod list/detail/create reads (P1-P3), pod
//!   patch/delete + guards + sweep (P4-P5), projects serialize (J1-J2),
//!   desktop enroll/delete (K1-K3)
//!   (`runner/views/pods.py`, `projects.py:31-77,97-124`,
//!   `desktop.py:107-149,168-194`, PIDASHCONV-584).
//!
//! Later D-13 layer issues extend this module; they add files here, never a fork.
pub mod catalog_reads;
pub mod enroll_reads;
