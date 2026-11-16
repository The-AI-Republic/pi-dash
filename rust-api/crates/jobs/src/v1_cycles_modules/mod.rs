//! api-v1 cycles + modules task publishers (D-20, jobs layer).
//!
//! Port of the eleven `.delay()` call sites the cycle/module views trigger:
//! [`publish`] owns the Celery wire surface for the four `model_activity`
//! sites and the seven `issue_activity` sites — the task names, the
//! `(args, kwargs)` binding in exact Python call order, and the
//! [`NewJob`] constructors the D-20 route handlers (PIDASHCONV-362/406)
//! enqueue transactionally (`enqueue_in`). Task bodies live with their
//! existing owners (D-08 `tasks_webhooks`); nothing here re-implements
//! or re-registers them.
//!
//! Wiring note: the crate root declares `pub mod v1_cycles_modules;` (seam
//! for this issue's new files); every file under this module is new.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! [`NewJob`]: crate::queue::NewJob

pub mod publish;
