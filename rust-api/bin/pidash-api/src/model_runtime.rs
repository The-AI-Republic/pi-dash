//! Re-export: the model runtime moved to
//! `pidash_jobs::oss_worker::model_runtime` in PIDASHCONV-918. This shim
//! re-exports the submodules the inventory mirror test constructs;
//! `worker()` reaches the runtime through `register_oss_handlers`.

pub use pidash_jobs::oss_worker::model_runtime::{github, invoke, keys, tools_exec};
