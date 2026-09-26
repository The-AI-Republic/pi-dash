#![forbid(unsafe_code)]

//! AMQP transport for Celery-format publishing.
//!
//! During coexistence the Python Celery workers still consume from
//! RabbitMQ, so jobs the Rust worker does not own locally are published
//! here in Celery protocol v2 (see [`crate::celery`]) to the same
//! `celery` direct exchange / `celery` queue the Django setup uses
//! (mirroring `contract-tests/_harness/broker.py`: durable exchange and
//! queue, persistent delivery).
//!
//! Broker selection mirrors `apps/api/pi_dash/settings/common.py`: a full
//! `AMQP_URL` wins, otherwise the `RABBITMQ_USER` / `RABBITMQ_PASSWORD` /
//! `RABBITMQ_HOST` / `RABBITMQ_PORT` / `RABBITMQ_VHOST` parts are composed
//! into `amqp://user:pass@host:port/vhost`.

use amqprs::channel::{
    BasicPublishArguments, ExchangeDeclareArguments, ExchangeType, QueueBindArguments,
    QueueDeclareArguments,
};
use amqprs::connection::{Connection, OpenConnectionArguments};
use amqprs::{BasicProperties, FieldTable, FieldValue};
use thiserror::Error as ThisError;

use crate::celery::CeleryTaskMessage;

/// Exchange tasks are published to (the Celery default).
pub const CELERY_EXCHANGE: &str = "celery";
/// Queue Python workers consume from (the Celery default routing key).
pub const CELERY_ROUTING_KEY: &str = "celery";

/// How to reach the broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmqpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub virtual_host: String,
}

impl AmqpConfig {
    /// Default local broker (guest/guest on localhost, `/` vhost).
    pub fn local() -> Self {
        Self {
            host: "localhost".to_owned(),
            port: 5672,
            username: "guest".to_owned(),
            password: "guest".to_owned(),
            virtual_host: "/".to_owned(),
        }
    }

    /// Parse a full `amqp://user:pass@host:port/vhost` URL.
    /// `amqps://` (TLS) is rejected with [`AmqpError::TlsUnsupported`]:
    /// the deployment terminates TLS before the broker.
    pub fn parse_url(url: &str) -> Result<Self, AmqpError> {
        if url.starts_with("amqps://") {
            return Err(AmqpError::TlsUnsupported);
        }
        let rest = url
            .strip_prefix("amqp://")
            .ok_or_else(|| AmqpError::BadUrl(url.to_owned()))?;
        let (credentials, host_part) = rest
            .split_once('@')
            .ok_or_else(|| AmqpError::BadUrl(url.to_owned()))?;
        let (username, password) = credentials
            .split_once(':')
            .ok_or_else(|| AmqpError::BadUrl(url.to_owned()))?;
        let (host_port, vhost) = match host_part.split_once('/') {
            Some((hp, v)) => (hp, format!("/{v}")),
            None => (host_part, "/".to_owned()),
        };
        let (host, port) = match host_port.split_once(':') {
            Some((h, p)) => (
                h.to_owned(),
                p.parse::<u16>()
                    .map_err(|_| AmqpError::BadUrl(url.to_owned()))?,
            ),
            None => (host_port.to_owned(), 5672),
        };
        if username.is_empty() || host.is_empty() {
            return Err(AmqpError::BadUrl(url.to_owned()));
        }
        Ok(Self {
            host,
            port,
            username: username.to_owned(),
            password: password.to_owned(),
            virtual_host: vhost,
        })
    }

    /// Resolve the broker from the environment, mirroring
    /// `settings/common.py`: `AMQP_URL` first, else the `RABBITMQ_*`
    /// parts, else the local default.
    pub fn from_env() -> Result<Self, AmqpError> {
        if let Ok(url) = std::env::var("AMQP_URL") {
            if !url.is_empty() {
                return Self::parse_url(&url);
            }
        }
        let part = |name: &str, default: &str| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default.to_owned())
        };
        Ok(Self {
            host: part("RABBITMQ_HOST", "localhost"),
            port: part("RABBITMQ_PORT", "5672")
                .parse::<u16>()
                .map_err(|_| AmqpError::BadPort)?,
            username: part("RABBITMQ_USER", "guest"),
            password: part("RABBITMQ_PASSWORD", "guest"),
            virtual_host: part("RABBITMQ_VHOST", "/"),
        })
    }

    fn connection_args(&self) -> OpenConnectionArguments {
        OpenConnectionArguments::new(&self.host, self.port, &self.username, &self.password)
            .virtual_host(&self.virtual_host)
            .connection_name("pidash-rust-jobs")
            .finish()
    }
}

/// Why broker configuration or publishing failed.
#[derive(Debug, ThisError)]
pub enum AmqpError {
    #[error("bad AMQP URL: {0}")]
    BadUrl(String),
    #[error("bad RABBITMQ_PORT")]
    BadPort,
    #[error("amqps:// brokers are not supported (TLS terminates before the broker)")]
    TlsUnsupported,
    #[error("broker error: {0}")]
    Broker(#[from] amqprs::error::Error),
}

/// Convert one JSON header value to its AMQP table equivalent, matching
/// what kombu/pika put on the wire: strings as long-strings, booleans as
/// booleans, small integers as 32-bit, null as void, arrays as field
/// arrays and objects as nested tables.
pub fn header_value(value: &serde_json::Value) -> FieldValue {
    match value {
        serde_json::Value::Null => FieldValue::V,
        serde_json::Value::Bool(b) => FieldValue::t(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if let Ok(small) = i32::try_from(i) {
                    FieldValue::I(small)
                } else {
                    FieldValue::l(i)
                }
            } else if let Some(u) = n.as_u64() {
                if u <= i64::MAX as u64 {
                    FieldValue::l(u as i64)
                } else {
                    FieldValue::from(u.to_string())
                }
            } else {
                FieldValue::from(n.to_string())
            }
        }
        serde_json::Value::String(s) => FieldValue::from(s.clone()),
        serde_json::Value::Array(items) => {
            let values = items.iter().map(header_value).collect::<Vec<_>>();
            FieldValue::A(
                values
                    .try_into()
                    .expect("header arrays are small enough to encode"),
            )
        }
        serde_json::Value::Object(map) => {
            let mut table = FieldTable::new();
            for (k, v) in map {
                table.insert(
                    k.clone().try_into().expect("header keys are short"),
                    header_value(v),
                );
            }
            FieldValue::F(table)
        }
    }
}

/// Build the AMQP headers table from a message's headers map.
pub fn headers_table(message: &CeleryTaskMessage) -> FieldTable {
    let mut table = FieldTable::new();
    for (k, v) in message.headers() {
        table.insert(
            k.try_into().expect("header keys are short"),
            header_value(&v),
        );
    }
    table
}

/// A connected publisher to the Celery exchange.
///
/// Cloneable: the underlying connection multiplexes channels, so every
/// worker task holds its own handle to the same connection.
#[derive(Clone)]
pub struct Publisher {
    connection: Connection,
}

impl Publisher {
    /// Connect and declare the durable `celery` exchange, queue and
    /// binding (the same topology kombu maintains).
    pub async fn connect(config: &AmqpConfig) -> Result<Self, AmqpError> {
        let connection = Connection::open(&config.connection_args()).await?;
        let channel = connection.open_channel(None).await?;
        // Durable like kombu's `Exchange("celery", "direct", durable=True)`:
        // re-declaring the Python workers' exchange with different
        // durability would fail with PRECONDITION_FAILED.
        let mut exchange = ExchangeDeclareArguments::of_type(CELERY_EXCHANGE, ExchangeType::Direct);
        exchange.durable(true);
        channel.exchange_declare(exchange).await?;
        channel
            .queue_declare(QueueDeclareArguments::durable_client_named(
                CELERY_ROUTING_KEY,
            ))
            .await?;
        channel
            .queue_bind(QueueBindArguments::new(
                CELERY_ROUTING_KEY,
                CELERY_EXCHANGE,
                CELERY_ROUTING_KEY,
            ))
            .await?;
        channel.close().await?;
        Ok(Self { connection })
    }

    /// Publish one task in Celery protocol v2. Delivery is persistent;
    /// a broker `nack` or disconnect surfaces as an error and the caller
    /// (the worker loop) requeues rather than dropping the job.
    pub async fn publish(&self, message: &CeleryTaskMessage) -> Result<(), AmqpError> {
        let props = message.properties();
        let channel = self.connection.open_channel(None).await?;
        let mut basic = BasicProperties::default();
        basic
            .with_content_type(&props.content_type)
            .with_content_encoding(&props.content_encoding)
            .with_correlation_id(&props.correlation_id)
            .with_delivery_mode(props.delivery_mode)
            .with_headers(headers_table(message))
            .finish();
        let (_, body) = message.to_wire();
        channel
            .basic_publish(
                basic,
                body,
                BasicPublishArguments::new(CELERY_EXCHANGE, CELERY_ROUTING_KEY),
            )
            .await?;
        channel.close().await?;
        Ok(())
    }

    /// Close the underlying connection.
    pub async fn close(self) -> Result<(), AmqpError> {
        self.connection.close().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map, Value};

    fn message() -> CeleryTaskMessage {
        CeleryTaskMessage {
            id: "task-id-1".to_owned(),
            task: "pi_dash.bgtasks.loop.scan_due_targets".to_owned(),
            args: vec![json!(1)],
            kwargs: Map::new(),
            retries: 0,
            eta: None,
            expires: None,
            timelimit: (None, None),
            root_id: None,
            parent_id: None,
            origin: None,
        }
    }

    #[test]
    fn url_parsing_mirrors_settings_fallback() {
        let cfg = AmqpConfig::parse_url("amqp://user:pass@rabbit:5673/vhost").expect("parse");
        assert_eq!(cfg.host, "rabbit");
        assert_eq!(cfg.port, 5673);
        assert_eq!(cfg.username, "user");
        assert_eq!(cfg.password, "pass");
        assert_eq!(cfg.virtual_host, "/vhost");

        let default_port = AmqpConfig::parse_url("amqp://u:p@h/v").expect("parse");
        assert_eq!(default_port.port, 5672);

        assert!(matches!(
            AmqpConfig::parse_url("amqps://u:p@h/v"),
            Err(AmqpError::TlsUnsupported)
        ));
        assert!(matches!(
            AmqpConfig::parse_url("not-a-url"),
            Err(AmqpError::BadUrl(_))
        ));
    }

    #[test]
    fn header_values_keep_wire_types() {
        assert_eq!(header_value(&Value::Null), FieldValue::V);
        assert_eq!(header_value(&json!(true)), FieldValue::t(true));
        assert_eq!(header_value(&json!(0)), FieldValue::I(0));
        assert_eq!(
            header_value(&json!("celery")),
            FieldValue::from("celery".to_owned())
        );
        match header_value(&Value::Array(vec![Value::Null, Value::Null])) {
            FieldValue::A(_) => {}
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn headers_table_covers_every_header() {
        use amqprs::{FieldName, FieldValue};
        use std::collections::HashMap;

        let table = headers_table(&message());
        let headers = message().headers();
        let map: &HashMap<FieldName, FieldValue> = table.as_ref();
        assert_eq!(map.len(), headers.len());
        let key: FieldName = "task".to_owned().try_into().expect("short");
        assert_eq!(
            map.get(&key),
            Some(&FieldValue::from(
                "pi_dash.bgtasks.loop.scan_due_targets".to_owned()
            ))
        );
        let retries: FieldName = "retries".to_owned().try_into().expect("short");
        assert_eq!(map.get(&retries), Some(&FieldValue::I(0)));
    }
}
