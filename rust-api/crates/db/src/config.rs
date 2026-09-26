//! Postgres connection configuration.
//!
//! Parsed once at startup from `DATABASE_URL`. The full URL (with password)
//! never reaches logs: use [`DbConfig::redacted_url`] for diagnostics.

use thiserror::Error as ThisError;

/// How to reach Postgres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbConfig {
    url: String,
    max_connections: u32,
}

impl DbConfig {
    /// Name of the environment variable holding the connection URL.
    pub const ENV_VAR: &str = "DATABASE_URL";

    /// Parse and validate a connection URL.
    ///
    /// Only `postgres://` / `postgresql://` URLs are accepted; anything else
    /// is a startup error, not a runtime surprise.
    pub fn new(url: impl Into<String>, max_connections: u32) -> Result<Self, ConfigError> {
        let url = url.into();
        if url.is_empty() {
            return Err(ConfigError::MissingUrl);
        }
        if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
            return Err(ConfigError::UnsupportedScheme);
        }
        if max_connections == 0 {
            return Err(ConfigError::InvalidPoolSize);
        }
        Ok(Self {
            url,
            max_connections,
        })
    }

    /// Read the configuration from the environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        let url = std::env::var(Self::ENV_VAR).map_err(|_| ConfigError::MissingUrl)?;
        Self::new(url, 10)
    }

    /// The connection URL, for the pool builder only.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Maximum pool size.
    pub fn max_connections(&self) -> u32 {
        self.max_connections
    }

    /// The URL with any `user:password@` credentials replaced by `***`.
    /// Safe to print in logs and startup banners.
    pub fn redacted_url(&self) -> String {
        match self.url.split_once("://") {
            Some((scheme, rest)) => match rest.split_once('@') {
                Some(_) => {
                    let host = rest.rsplit('@').next().unwrap_or(rest);
                    format!("{scheme}://***@{host}")
                }
                None => self.url.clone(),
            },
            None => self.url.clone(),
        }
    }
}

/// Why a [`DbConfig`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum ConfigError {
    #[error("DATABASE_URL is missing or empty")]
    MissingUrl,
    #[error("only postgres:// connection URLs are supported")]
    UnsupportedScheme,
    #[error("pool size must be at least 1")]
    InvalidPoolSize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DbConfig {
        DbConfig::new("postgresql://app:s3cret@db:5432/pidash", 10).expect("valid")
    }

    #[test]
    fn accepts_postgres_urls() {
        assert_eq!(config().max_connections(), 10);
        assert!(DbConfig::new("postgres://u@h/db", 1).is_ok());
    }

    #[test]
    fn rejects_empty_unsupported_and_zero_pool() {
        assert_eq!(DbConfig::new("", 5).unwrap_err(), ConfigError::MissingUrl);
        assert_eq!(
            DbConfig::new("sqlite://x.db", 5).unwrap_err(),
            ConfigError::UnsupportedScheme
        );
        assert_eq!(
            DbConfig::new("postgresql://h/db", 0).unwrap_err(),
            ConfigError::InvalidPoolSize
        );
    }

    #[test]
    fn redaction_hides_credentials_but_keeps_host() {
        let redacted = config().redacted_url();
        assert!(!redacted.contains("s3cret"), "{redacted}");
        assert!(redacted.contains("db:5432/pidash"), "{redacted}");
    }

    #[test]
    fn redaction_leaves_credential_free_urls_alone() {
        let plain = DbConfig::new("postgresql:///pidash_contract", 5).expect("valid");
        assert_eq!(plain.redacted_url(), "postgresql:///pidash_contract");
    }
}
