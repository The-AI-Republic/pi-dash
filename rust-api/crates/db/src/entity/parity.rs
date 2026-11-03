//! `information_schema` parity test for the generated entities.
//!
//! For each covered table this compares the entity's declared table name,
//! column names, column types and nullability with a database migrated by
//! `apps/api`, and fails on any drift. Django stays the schema owner: after
//! a Django sync the entities are regenerated (see `super`'s module doc and
//! `rust-api/README.md`) and this test proves the regen matches.
//!
//! The test needs a migrated database: like every other live-db test in the
//! workspace it skips when `DATABASE_URL` is unset (plain `cargo test`, and
//! post-merge CI, have no Postgres) and runs for real otherwise. Type
//! lengths are compared only when the entity pins them: `sea-orm-cli` drops
//! `varchar(n)` lengths (bare `String` infers `StringLen::None`), so those
//! compare the `varchar` family only, while a pinned `StringLen::N(n)`
//! compares the length too.

use std::collections::BTreeMap;

use sea_orm::strum::IntoEnumIterator;
use sea_orm::{ColumnTrait, EntityName, EntityTrait, IdenStatic};
use sea_query::{ColumnType, RcOrArc, StringLen};

/// One `information_schema.columns` row: name, udt, char length,
/// numeric precision, numeric scale, `YES`/`NO` nullability.
type InfoSchemaRow = (
    String,
    String,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    String,
);

/// One `information_schema.columns` row for a covered table.
struct DbColumn {
    udt: String,
    char_len: Option<i32>,
    num_prec: Option<i32>,
    num_scale: Option<i32>,
    nullable: bool,
}

/// The Postgres type key an entity [`ColumnType`] maps to.
///
/// Lengths are `Some` only when the entity pins them; `None` skips that
/// comparison (the generator drops `varchar(n)` lengths).
struct ExpectedType {
    udt: String,
    char_len: Option<u32>,
    num_prec: Option<u32>,
    num_scale: Option<u32>,
}

impl ExpectedType {
    fn plain(udt: &str) -> Self {
        Self {
            udt: udt.to_owned(),
            char_len: None,
            num_prec: None,
            num_scale: None,
        }
    }
}

/// Map an entity [`ColumnType`] to its Postgres `udt_name`, following the
/// `sea-query` Postgres backend's DDL table (`table.rs`,
/// `prepare_column_type`). Returns `None` for types Postgres cannot store
/// ([`ColumnType::Year` is `unimplemented!` there) so the test fails loudly
/// instead of silently passing an unmapped type.
fn expected_type(col: &ColumnType) -> Option<ExpectedType> {
    match col {
        // Bare `char` is `character(1)` in Postgres.
        ColumnType::Char(len) => Some(ExpectedType {
            udt: "bpchar".to_owned(),
            char_len: Some(len.unwrap_or(1)),
            num_prec: None,
            num_scale: None,
        }),
        // Bare `varchar` (what `None` and `Max` both render as) carries no
        // length in `information_schema`.
        ColumnType::String(StringLen::N(len)) => Some(ExpectedType {
            udt: "varchar".to_owned(),
            char_len: Some(*len),
            num_prec: None,
            num_scale: None,
        }),
        ColumnType::String(_) => Some(ExpectedType::plain("varchar")),
        ColumnType::Text => Some(ExpectedType::plain("text")),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned => Some(ExpectedType::plain("int2")),
        ColumnType::Integer | ColumnType::Unsigned => Some(ExpectedType::plain("int4")),
        ColumnType::BigInteger | ColumnType::BigUnsigned => Some(ExpectedType::plain("int8")),
        ColumnType::Float => Some(ExpectedType::plain("float4")),
        ColumnType::Double => Some(ExpectedType::plain("float8")),
        ColumnType::Decimal(precision) => Some(ExpectedType {
            udt: "numeric".to_owned(),
            char_len: None,
            num_prec: precision.map(|(prec, _)| prec),
            num_scale: precision.map(|(_, scale)| scale),
        }),
        ColumnType::DateTime | ColumnType::Timestamp => Some(ExpectedType::plain("timestamp")),
        ColumnType::TimestampWithTimeZone => Some(ExpectedType::plain("timestamptz")),
        ColumnType::Time => Some(ExpectedType::plain("time")),
        ColumnType::Date => Some(ExpectedType::plain("date")),
        // Postgres has no `year` column type.
        ColumnType::Year => None,
        ColumnType::Interval(_, _) => Some(ExpectedType::plain("interval")),
        ColumnType::Binary(_) | ColumnType::VarBinary(_) | ColumnType::Blob => {
            Some(ExpectedType::plain("bytea"))
        }
        // `character_maximum_length` covers bit strings too.
        ColumnType::Bit(len) => Some(ExpectedType {
            udt: "bit".to_owned(),
            char_len: Some(len.unwrap_or(1)),
            num_prec: None,
            num_scale: None,
        }),
        ColumnType::VarBit(len) => Some(ExpectedType {
            udt: "varbit".to_owned(),
            char_len: Some(*len),
            num_prec: None,
            num_scale: None,
        }),
        ColumnType::Boolean => Some(ExpectedType::plain("bool")),
        // Postgres `money` takes no precision; only the family compares.
        ColumnType::Money(_) => Some(ExpectedType::plain("money")),
        ColumnType::Json => Some(ExpectedType::plain("json")),
        ColumnType::JsonBinary => Some(ExpectedType::plain("jsonb")),
        ColumnType::Uuid => Some(ExpectedType::plain("uuid")),
        ColumnType::Custom(name) => Some(ExpectedType::plain(&name.to_string())),
        ColumnType::Enum { name, .. } => Some(ExpectedType::plain(&name.to_string())),
        // Postgres names an array type `_<element>`.
        ColumnType::Array(inner) => {
            let inner = expected_type(inner)?;
            if inner.udt.starts_with('_') {
                return None;
            }
            Some(ExpectedType::plain(&format!("_{}", inner.udt)))
        }
        ColumnType::Vector(_) => Some(ExpectedType::plain("vector")),
        ColumnType::Cidr => Some(ExpectedType::plain("cidr")),
        ColumnType::Inet => Some(ExpectedType::plain("inet")),
        ColumnType::MacAddr => Some(ExpectedType::plain("macaddr")),
        ColumnType::LTree => Some(ExpectedType::plain("ltree")),
        // `ColumnType` is non-exhaustive: a future variant fails loudly.
        _ => None,
    }
}

/// Compare entity `E`'s columns with `table` in `information_schema`.
/// Fails with one line per drifted column.
async fn assert_entity_parity<E>(pool: &sqlx::PgPool, table: &str)
where
    E: EntityTrait,
    E::Column: IntoEnumIterator,
{
    let mut expected = BTreeMap::new();
    for col in E::Column::iter() {
        let name = col.as_str().to_owned();
        let def = col.def();
        let col_type = def.get_column_type();
        let exp = expected_type(col_type)
            .unwrap_or_else(|| panic!("{table}.{name}: unmapped entity type {col_type:?}"));
        expected.insert(name, (exp, def.is_null()));
    }

    let rows: Vec<InfoSchemaRow> = sqlx::query_as(
        "SELECT column_name, udt_name, character_maximum_length, \
             numeric_precision, numeric_scale, is_nullable \
             FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 \
             ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("information_schema query");
    assert!(
        !rows.is_empty(),
        "table {table} missing from information_schema on the migrated database"
    );
    let mut db = BTreeMap::new();
    for (name, udt, char_len, num_prec, num_scale, is_nullable) in rows {
        db.insert(
            name,
            DbColumn {
                udt,
                char_len,
                num_prec,
                num_scale,
                nullable: is_nullable == "YES",
            },
        );
    }

    let mut diffs = Vec::new();
    for (name, (exp, nullable)) in &expected {
        match db.get(name) {
            None => diffs.push(format!(
                "column {table}.{name} in entity but missing from database"
            )),
            Some(found) => {
                if found.udt != exp.udt {
                    diffs.push(format!(
                        "column {table}.{name} type drift: entity {} but database {}",
                        exp.udt, found.udt
                    ));
                }
                if let Some(len) = exp.char_len {
                    if found.char_len != Some(len as i32) {
                        diffs.push(format!(
                            "column {table}.{name} length drift: entity {len} but database {:?}",
                            found.char_len
                        ));
                    }
                }
                if let Some(prec) = exp.num_prec {
                    if found.num_prec != Some(prec as i32) {
                        diffs.push(format!(
                            "column {table}.{name} precision drift: entity {prec} but database {:?}",
                            found.num_prec
                        ));
                    }
                }
                if let Some(scale) = exp.num_scale {
                    if found.num_scale != Some(scale as i32) {
                        diffs.push(format!(
                            "column {table}.{name} scale drift: entity {scale} but database {:?}",
                            found.num_scale
                        ));
                    }
                }
                if found.nullable != *nullable {
                    diffs.push(format!(
                        "column {table}.{name} nullability drift: entity {} but database {}",
                        if *nullable { "NULL" } else { "NOT NULL" },
                        if found.nullable { "NULL" } else { "NOT NULL" },
                    ));
                }
            }
        }
    }
    for name in db.keys() {
        if !expected.contains_key(name) {
            diffs.push(format!(
                "column {table}.{name} in database but missing from entity"
            ));
        }
    }
    assert!(
        diffs.is_empty(),
        "schema drift on {table}:\n{}",
        diffs.join("\n")
    );
}

/// One parity test per covered table: pins the entity's declared table
/// name, then compares its columns with `information_schema`.
macro_rules! parity_test {
    ($test:ident, $entity:ident, $table:literal) => {
        #[tokio::test]
        async fn $test() {
            let url = match std::env::var("DATABASE_URL") {
                Ok(url) => url,
                Err(_) => {
                    eprintln!("skipping live-db test: DATABASE_URL is not set");
                    return;
                }
            };
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL");
            let entity = super::$entity::Entity;
            assert_eq!(entity.table_name(), $table, "entity table_name drift");
            assert_entity_parity::<super::$entity::Entity>(&pool, $table).await;
        }
    };
}

parity_test!(issues, issues, "issues");
parity_test!(issue_sequences, issue_sequences, "issue_sequences");
parity_test!(issue_assignees, issue_assignees, "issue_assignees");
parity_test!(issue_comments, issue_comments, "issue_comments");
parity_test!(descriptions, descriptions, "descriptions");
parity_test!(issue_relations, issue_relations, "issue_relations");
parity_test!(users, users, "users");
parity_test!(profiles, profiles, "profiles");
parity_test!(accounts, accounts, "accounts");
parity_test!(
    user_notification_preferences,
    user_notification_preferences,
    "user_notification_preferences"
);
parity_test!(workspaces, workspaces, "workspaces");
parity_test!(workspace_members, workspace_members, "workspace_members");
parity_test!(projects, projects, "projects");
parity_test!(project_members, project_members, "project_members");
parity_test!(states, states, "states");
parity_test!(api_tokens, api_tokens, "api_tokens");
parity_test!(machine_token, machine_token, "machine_token");
parity_test!(runner_session, runner_session, "runner_session");
parity_test!(
    assistant_user_llm_config,
    assistant_user_llm_config,
    "assistant_user_llm_config"
);
parity_test!(assistant_message, assistant_message, "assistant_message");

#[test]
fn inferred_string_compares_varchar_family_only() {
    let exp = expected_type(&ColumnType::String(StringLen::None)).expect("mapped");
    assert_eq!(exp.udt, "varchar");
    assert_eq!(exp.char_len, None);
}

#[test]
fn pinned_string_compares_length() {
    let exp = expected_type(&ColumnType::String(StringLen::N(30))).expect("mapped");
    assert_eq!(exp.udt, "varchar");
    assert_eq!(exp.char_len, Some(30));
}

#[test]
fn array_maps_to_underscore_element() {
    let varchar_array = ColumnType::Array(RcOrArc::new(ColumnType::String(StringLen::None)));
    let exp = expected_type(&varchar_array).expect("mapped");
    assert_eq!(exp.udt, "_varchar");
}

#[test]
fn year_is_unmapped() {
    assert!(expected_type(&ColumnType::Year).is_none());
}
