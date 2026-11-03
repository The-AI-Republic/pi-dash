//! Database parity helpers: the exhaustive `sea_query::Value` binder and
//! Postgres error classifiers.
//!
//! Moved verbatim from `pidash-api` (`app_issues/mod.rs`, D-26) by OVL-04
//! (PIDASHCONV-1542); the only change is the error type (`Denial` is an
//! `api`-local type, so the canonical functions return [`BindError`] and
//! callers map it to their own server-error variant — the same bytes as
//! before, since every previous copy failed only to its 500 variant).
//!
//! The match below must stay exhaustive with no wildcard arm: the
//! rust-v2026.10.26 release broke because hand-copied binders did not
//! cover the variants a newly enabled sea-query feature added. A future
//! variant addition must fail to compile here, loudly, in exactly one
//! place.

use std::fmt;

use sqlx::postgres::{PgArguments, Postgres};

/// Binding failed: the compiler emitted a value kind the binder never
/// binds (`Json` / `Array` / the chrono time variants). Every OSS caller
/// maps this to its 500 variant, exactly as the previous local copies did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindError;

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unsupported sea_query::Value variant for binding")
    }
}

impl std::error::Error for BindError {}

/// Bind one `sea_query::Value` to a `sqlx` query (moved from
/// `api/src/app_issues/mod.rs::bind_sea_value`).
pub fn bind_sea_value<'q>(
    query: sqlx::query::Query<'q, Postgres, PgArguments>,
    value: &sea_query::Value,
) -> Result<sqlx::query::Query<'q, Postgres, PgArguments>, BindError> {
    use sea_query::Value as SeaValue;
    match value {
        SeaValue::Bool(value) => Ok(query.bind(*value)),
        SeaValue::TinyInt(value) => Ok(query.bind(*value)),
        SeaValue::SmallInt(value) => Ok(query.bind(*value)),
        SeaValue::Int(value) => Ok(query.bind(*value)),
        SeaValue::BigInt(value) => Ok(query.bind(*value)),
        SeaValue::TinyUnsigned(value) => Ok(query.bind(value.map(i16::from))),
        SeaValue::SmallUnsigned(value) => Ok(query.bind(value.map(i32::from))),
        SeaValue::Unsigned(value) => Ok(query.bind(value.map(|v| v as i64))),
        SeaValue::BigUnsigned(value) => Ok(query.bind(value.map(|v| v as i64))),
        SeaValue::Float(value) => Ok(query.bind(*value)),
        SeaValue::Double(value) => Ok(query.bind(*value)),
        SeaValue::String(value) => Ok(query.bind(value.clone().map(|text| *text))),
        SeaValue::Char(value) => Ok(query.bind(value.map(|char| char.to_string()))),
        SeaValue::Bytes(value) => Ok(query.bind(value.clone().map(|bytes| (*bytes).clone()))),
        SeaValue::Uuid(value) => Ok(query.bind(value.clone().map(|id| *id))),
        // Rich date / datetime filters bind typed params (`DATE` /
        // `timestamptz`), like the ORM. The remaining chrono variants are
        // never emitted; they only keep the match exhaustive.
        SeaValue::ChronoDate(value) => Ok(query.bind(value.clone().map(|date| *date))),
        SeaValue::ChronoDateTimeUtc(value) => Ok(query.bind(value.clone().map(|moment| *moment))),
        SeaValue::ChronoTime(_)
        | SeaValue::ChronoDateTime(_)
        | SeaValue::ChronoDateTimeLocal(_)
        | SeaValue::ChronoDateTimeWithTimeZone(_)
        // `Json` / `Array` exist only because ORM-01 enables
        // `sea-query/with-json` workspace-wide (via sea-orm); these
        // compilers emit scalars, never them.
        | SeaValue::Json(_)
        | SeaValue::Array(_, _) => Err(BindError),
    }
}

/// Bind a `sea_query::Value` vec to raw SQL (moved from
/// `api/src/app_issues/mod.rs::bind_all`; the `bind_values` copies in
/// `app_cycles/handlers_cycle_issues.rs` and
/// `app_modules/handlers_module_issues.rs` carried the same mapping).
pub fn bind_all<'a>(
    sql: &'a str,
    values: Vec<sea_query::Value>,
) -> Result<sqlx::query::Query<'a, Postgres, PgArguments>, BindError> {
    let mut query = sqlx::query(sql);
    for value in &values {
        query = bind_sea_value(query, value)?;
    }
    Ok(query)
}

/// True when the failure is a unique violation (SQLSTATE 23505).
/// Canonicalizes 16 identical OSS copies (one used sqlx's own
/// `is_unique_violation`, which checks the same code on Postgres).
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code == "23505"))
}

/// True when the failure is any integrity-constraint violation
/// (SQLSTATE class 23 — Django's `IntegrityError`, which psycopg raises
/// for the whole class).
///
/// Canonicalizes 12 OSS copies: 9 already matched the `23` prefix, 3
/// matched only `23502`/`23503`/`23505`/`23514` explicitly (or via
/// sqlx's `ErrorKind`). The forms are identical on every reachable
/// input: Postgres class 23 is exactly
/// {23000, 23001, 23502, 23503, 23505, 23514, 23P01}, and the schema has
/// no `RESTRICT` deletes (which produce 23000/23001) and no `EXCLUDE`
/// constraints (which produce 23P01) — verified by grep over
/// `apps/api` on rust-dev 2a068127b — so only the four codes can occur.
pub fn is_integrity_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use sea_query::Value as SeaValue;

    #[test]
    fn bind_all_binds_scalars_in_order() {
        // sqlx::Query exposes no offline accessor for bound values, so
        // this pins acceptance (every arm type-checks and binds without
        // panicking); bind order/bytes stay pinned by the contract suites
        // over the migrated callers.
        let _bound = bind_all(
            "SELECT $1, $2, $3, $4, $5",
            vec![
                SeaValue::Bool(Some(true)),
                SeaValue::Int(Some(7)),
                SeaValue::String(Some(Box::new("seven".to_string()))),
                SeaValue::Uuid(Some(Box::new(
                    uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap(),
                ))),
                SeaValue::Double(Some(1.5)),
            ],
        )
        .expect("scalars bind");
    }

    #[test]
    fn bind_all_covers_every_scalar_arm() {
        // One value per bindable arm, including NULLs and the chrono
        // date/datetime arms; building the query is offline (no DB).
        let values = vec![
            SeaValue::Bool(None),
            SeaValue::TinyInt(Some(1)),
            SeaValue::SmallInt(Some(2)),
            SeaValue::Int(None),
            SeaValue::BigInt(Some(4)),
            SeaValue::TinyUnsigned(Some(5)),
            SeaValue::SmallUnsigned(Some(6)),
            SeaValue::Unsigned(Some(7)),
            SeaValue::BigUnsigned(Some(8)),
            SeaValue::Float(Some(1.0)),
            SeaValue::Double(None),
            SeaValue::String(None),
            SeaValue::Char(Some('c')),
            SeaValue::Bytes(Some(Box::default())),
            SeaValue::Uuid(None),
            SeaValue::ChronoDate(Some(Box::new(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
            ))),
            SeaValue::ChronoDateTimeUtc(Some(Box::new(
                chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
            ))),
        ];
        let placeholders = (1..=values.len())
            .map(|n| format!("${n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT {placeholders}");
        let _bound = bind_all(&sql, values).expect("every scalar arm binds");
    }

    #[test]
    fn bind_all_rejects_non_scalar_variants() {
        for value in [
            SeaValue::Json(Some(Box::new(serde_json::Value::Null))),
            SeaValue::Array(sea_query::ArrayType::Int, Some(Box::default())),
            SeaValue::ChronoTime(None),
            SeaValue::ChronoDateTime(None),
            SeaValue::ChronoDateTimeLocal(None),
            SeaValue::ChronoDateTimeWithTimeZone(None),
        ] {
            assert!(matches!(bind_all("SELECT $1", vec![value]), Err(BindError)));
        }
    }

    #[derive(Debug)]
    struct StubDbError {
        code: Option<&'static str>,
    }

    impl fmt::Display for StubDbError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("stub database error")
        }
    }

    impl std::error::Error for StubDbError {}

    impl sqlx::error::DatabaseError for StubDbError {
        fn message(&self) -> &str {
            "stub"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            self.code.map(Cow::Borrowed)
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
    }

    fn db_error(code: Option<&'static str>) -> sqlx::Error {
        sqlx::Error::Database(Box::new(StubDbError { code }))
    }

    #[test]
    fn classifiers_match_django_integrity_semantics() {
        // (code, is_unique_violation, is_integrity_error)
        for (code, unique, integrity) in [
            (Some("23505"), true, true),
            (Some("23503"), false, true),
            (Some("23502"), false, true),
            (Some("23514"), false, true),
            (Some("23P01"), false, true),
            (Some("42601"), false, false),
            (None, false, false),
        ] {
            let error = db_error(code);
            assert_eq!(is_unique_violation(&error), unique, "unique {code:?}");
            assert_eq!(is_integrity_error(&error), integrity, "integrity {code:?}");
        }
        assert!(!is_unique_violation(&sqlx::Error::RowNotFound));
        assert!(!is_integrity_error(&sqlx::Error::RowNotFound));
    }
}
