//! Re-export: the runner seam providers moved to
//! `pidash_jobs::oss_worker::runner_seams` in PIDASHCONV-918. This shim
//! keeps the binary's inventory mirror test on its paths; `worker()`
//! reaches the providers through `register_oss_handlers`.

pub use pidash_jobs::oss_worker::runner_seams::RunnerSeams;
