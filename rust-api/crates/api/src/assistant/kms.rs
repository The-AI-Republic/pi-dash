//! Re-export: the production KMS transport moved to
//! `pidash_jobs::oss_worker::kms_transport` in PIDASHCONV-918 (the
//! worker's key resolver is its second caller, and `pidash-jobs`
//! cannot depend on `pidash-api`). This path stays so the assistant
//! handlers are untouched.

pub use pidash_jobs::oss_worker::kms_transport::HttpKmsTransport;
