//! api-v1 asset/sticky/intake background tasks (D-21, PIDASHCONV-417).
//!
//! Port of the four `.delay()` units named by fixtures `fx-task-asset`
//! and `fx-task-intake` (`rust-api/fixtures/v1_assets/`). [`tasks`] owns
//! the Celery wire surface for the api-v1 call sites — task names, kwarg
//! order, the `storage_metadata` falsy guard — plus the `NewJob`
//! constructors the D-21 route handlers (PIDASHCONV-419/421/426) enqueue
//! transactionally. Task bodies live with their existing owners (D-09 for
//! the metadata task, D-08 for the activity dispatcher); nothing here
//! re-implements or re-registers them.

pub mod tasks;
