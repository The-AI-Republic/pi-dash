//! Re-export: the execute seams moved to
//! `pidash_jobs::oss_worker::execute_seams` in PIDASHCONV-918. This shim
//! keeps the binary's inventory mirror test on its paths; `worker()`
//! reaches the seams through `register_oss_handlers`.

pub use pidash_jobs::oss_worker::execute_seams::LiveExecuteSeams;
