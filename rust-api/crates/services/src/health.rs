//! Liveness logic: the computation behind `GET /healthz`.

use pidash_auth::TenantScope;
use pidash_db::DbConfig;
use pidash_types::HealthStatus;

/// Build the liveness report. The version is the serving binary's version.
pub fn health_report(version: impl Into<String>) -> HealthStatus {
    HealthStatus::ok(version)
}

/// One-line, credential-free summary of the database configuration for the
/// health payload's `db` field. Takes the tenant scope so callers thread the
/// request context through every service call from day one.
pub fn db_summary(_scope: &TenantScope, config: &DbConfig) -> String {
    format!(
        "postgres max_connections={} url={}",
        config.max_connections(),
        config.redacted_url()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::WorkspaceId;

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("ws-1"))
    }

    fn config() -> DbConfig {
        DbConfig::new("postgresql://app:s3cret@db:5432/pidash", 10).expect("valid")
    }

    #[test]
    fn health_report_is_ok_with_binary_version() {
        assert_eq!(health_report("0.1.0"), HealthStatus::ok("0.1.0"));
    }

    #[test]
    fn db_summary_never_leaks_credentials() {
        let summary = db_summary(&scope(), &config());
        assert!(!summary.contains("s3cret"), "{summary}");
        assert!(summary.contains("max_connections=10"), "{summary}");
    }
}
