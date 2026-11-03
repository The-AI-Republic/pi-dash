//! Pull-request operation: attach a GitHub PR link to an issue.
//!
//! The serializer step of the v1 PR-link create endpoint, moved from
//! `pidash_api::v1_work_items::handlers_pr_links` (OVL-14,
//! PIDASHCONV-1649): `GithubPullRequestLinkSerializer`
//! (`github_pr.py:53-95`).

use chrono::Utc;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::app_issues::pr_links::{attach_pull_request, AttachError, StoreError};
use crate::domain::WriteCtx;
use crate::integrations::github_client::{
    LiveGithubClient, PoolPrStore, PrLinksSurface, TransportContext, TransportGuard,
};

use super::common::{activate_timezone, db_error};
use super::deps::Deps;
use super::error::{Error, Outcome};

/// Python `bool()` over a JSON value (the `(raw_url or "")` collapse).
fn is_python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `(request.data.get("url") or "")` (`github_pr.py:61`,
/// `git_code_review.py:59`, `code_reviews.py:168`): missing/null and falsy
/// non-strings collapse to `""` (the invalid-URL 400 below); truthy
/// non-strings 500 on `.strip()`; strings strip inside the port's parser.
pub fn extract_raw_url(body: &Map<String, Value>) -> Result<String, Error> {
    match body.get("url").unwrap_or(&Value::Null) {
        Value::Null => Ok(String::new()),
        Value::String(text) => Ok(text.clone()),
        other if !is_python_truthy(other) => Ok(String::new()),
        _ => Err(Error::ServerError),
    }
}

/// Render one PR-link row through `GithubPullRequestLinkSerializer`
/// (S3 [`render_pr_link`](crate::v1_work_items::shape_links::render_pr_link)):
/// full field set, no `fields=`/`expand=` (the view passes neither).
pub fn render_pr_row(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<Value, Error> {
    use crate::v1_work_items::shape_links as shape;
    use sqlx::Row as _;
    let dt = |key: &str| -> Result<String, Error> {
        row.try_get::<chrono::DateTime<chrono::Utc>, _>(key)
            .map(|stamp| pidash_compat::datetime::render_datetime_in(&stamp, tz))
            .map_err(|error| db_error(error, "pr row datetime"))
    };
    let dt_opt = |key: &str| -> Result<Option<String>, Error> {
        row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(key)
            .map(|stamp| stamp.map(|stamp| pidash_compat::datetime::render_datetime_in(&stamp, tz)))
            .map_err(|error| db_error(error, "pr row datetime"))
    };
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| db_error(error, "pr row map"))?;
    let issue_id: Uuid = row
        .try_get("issue_id")
        .map_err(|error| db_error(error, "pr row map"))?;
    let created_by: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|error| db_error(error, "pr row map"))?;
    let id = id.to_string();
    let issue = issue_id.to_string();
    let created_by = created_by.map(|id| id.to_string());
    let pr_updated_at = dt_opt("pr_updated_at")?;
    let created_at = dt("created_at")?;
    let updated_at = dt("updated_at")?;
    let repo_owner: String = row
        .try_get("repo_owner")
        .map_err(|error| db_error(error, "pr row map"))?;
    let repo_name: String = row
        .try_get("repo_name")
        .map_err(|error| db_error(error, "pr row map"))?;
    let url: String = row
        .try_get("url")
        .map_err(|error| db_error(error, "pr row map"))?;
    let title: String = row
        .try_get("title")
        .map_err(|error| db_error(error, "pr row map"))?;
    let state: String = row
        .try_get("state")
        .map_err(|error| db_error(error, "pr row map"))?;
    let shape_row = shape::PrLinkRow {
        id: &id,
        issue: &issue,
        repo_owner: &repo_owner,
        repo_name: &repo_name,
        pr_number: row
            .try_get("pr_number")
            .map_err(|error| db_error(error, "pr row map"))?,
        url: &url,
        title: &title,
        state: &state,
        merged: row
            .try_get("merged")
            .map_err(|error| db_error(error, "pr row map"))?,
        draft: row
            .try_get("draft")
            .map_err(|error| db_error(error, "pr row map"))?,
        pr_updated_at: pr_updated_at.as_deref(),
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
    };
    let map = shape::render_pr_link(&shape::PrLinkShowInput {
        row: &shape_row,
        fields: None,
        expand: &[],
        expansions: &[],
    })
    .map_err(|error| db_error(error, "pr row render"))?;
    Ok(Value::Object(map))
}

/// Sentinel for the deleted-project lookup miss inside PR attach: the
/// handler maps it to the 404 `DoesNotExist` body (Python's
/// `self.project` fetch on `save()`).
const PROJECT_WORKSPACE_MISS: &str = "pr-links: project workspace not found";

/// Sentinel for the lost create race when the re-lookup also misses:
/// Python re-raises the `IntegrityError` → the payload 400.
const PR_RACE_MISS: &str = "pr link lost the create race";

/// Marker prefix for mid-attach integrity failures (FK races on the
/// mirror writes): Python's uncaught `IntegrityError` → the payload
/// 400, so the handler maps these to 400, not 500.
const PR_PAYLOAD_INVALID: &str = "pr-links: payload not valid";

/// Re-read one PR link by id for the create response (the serializer
/// reads the saved instance, whose audit columns the store stamped).
const PR_OWNED_SQL: &str = r#"SELECT id, issue_id, repo_owner, repo_name, pr_number, url, title, state,
              merged, draft, pr_updated_at, created_at, updated_at, created_by_id
       FROM github_pull_request_links WHERE id = $1 LIMIT 1"#;

/// `POST .../github/pull-requests/` (`GithubPullRequestLinkSerializer`,
/// `github_pr.py:53-95`): validate the URL, attach through the store,
/// re-read and render the saved row (201 on create, 200 when the link
/// already covered this issue).
///
/// The caller already ran auth, the gate and body negotiation; `write`
/// carries what it resolved, including the settings-derived transport
/// inputs (token-cache handle, instance secret) the GitHub client needs.
pub async fn attach(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &PullRequestWrite<'_>,
) -> Result<Outcome, Error> {
    let pool = &deps.pool;
    let slug = write.slug;
    let project_id = write.project_id;
    let issue_id = write.issue_id;
    let actor_id = ctx.actor.user_id;
    let tz = activate_timezone(ctx.actor.timezone.as_deref())?;
    let body = write.body;
    let raw_url = extract_raw_url(body)?;
    let transport = TransportContext {
        pool: deps.pool.clone(),
        redis: write.redis.clone(),
        secret: write.secret.clone(),
    };
    let _guard = TransportGuard::install(transport);
    let store = PoolPrStore::new(deps.pool.clone(), PrLinksSurface::PrLinks);
    let now = Utc::now();
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(attach_pull_request::<
            PoolPrStore,
            LiveGithubClient,
        >(
            &store,
            project_id,
            issue_id,
            slug,
            &raw_url,
            &now,
            Some(actor_id),
        ))
    });
    drop(_guard);
    let (link, created) = match outcome {
        Ok(attached) => (attached.link, attached.created),
        Err(AttachError::InvalidUrl(_)) => {
            return Err(Error::BadError(
                "A valid github.com pull request URL is required.".to_owned(),
            ))
        }
        Err(AttachError::IssueNotFound(_)) => {
            return Err(Error::Rejection {
                status: 404,
                message: "Work item not found.".to_owned(),
            })
        }
        Err(AttachError::AlreadyLinked(conflict)) => {
            return Err(Error::Rejection {
                status: 409,
                message: format!(
                    "This pull request is already linked to issue {}.",
                    conflict.issue_id
                ),
            })
        }
        Err(AttachError::Store(StoreError(message))) => {
            if message == PROJECT_WORKSPACE_MISS {
                return Err(Error::NotFound);
            }
            if message.starts_with(PR_PAYLOAD_INVALID) || message.contains(PR_RACE_MISS) {
                return Err(Error::BadError("The payload is not valid".to_owned()));
            }
            return Err(Error::ServerError);
        }
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(PR_OWNED_SQL)
        .bind(link.id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "pr create reread"))?;
    let Some(row) = row else {
        return Err(Error::ServerError);
    };
    let shape = render_pr_row(&row, &tz)?;
    let text = serde_json::to_string(&shape).map_err(|_| Error::ServerError)?;
    if created {
        Ok(Outcome::created(text))
    } else {
        Ok(Outcome::ok(text))
    }
}

/// The pre-resolved PR-attach scope: auth, the gate and body negotiation
/// already ran; `body` is the negotiated JSON object.
pub struct PullRequestWrite<'a> {
    /// The workspace slug from the route.
    pub slug: &'a str,
    /// The gated project id (after identifier rewrite).
    pub project_id: Uuid,
    /// The linked issue id.
    pub issue_id: Uuid,
    /// The negotiated JSON object body.
    pub body: &'a Map<String, Value>,
    /// The installation-token cache handle, if configured (`None`
    /// mints through, like the view).
    pub redis: Option<pidash_db::redis::RedisHandle>,
    /// The instance secret (the config-decrypt + PAT keyring source).
    pub secret: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_server_error(error: Error) -> bool {
        error.status_and_body().0 == 500
    }

    #[test]
    fn raw_url_extraction_collapses_falsy_and_rejects_truthy_non_strings() {
        let url_of = |value: Value| {
            let mut map = Map::new();
            map.insert("url".to_owned(), value);
            extract_raw_url(&map)
        };
        // Missing / null / falsy → "" (the invalid-URL 400 downstream).
        assert_eq!(
            extract_raw_url(&Map::new()).expect("missing"),
            String::new()
        );
        for value in [
            Value::Null,
            Value::Bool(false),
            Value::Number(0.into()),
            Value::Number(serde_json::Number::from_f64(0.0).expect("zero")),
            Value::String(String::new()),
            Value::Array(vec![]),
            Value::Object(Map::new()),
        ] {
            assert_eq!(
                url_of(value.clone()).expect("falsy"),
                String::new(),
                "{value}"
            );
        }
        // Strings pass through (the parser strips).
        assert_eq!(
            url_of(Value::String(
                "  https://github.com/a/b/pull/1  ".to_owned()
            ))
            .expect("string"),
            "  https://github.com/a/b/pull/1  "
        );
        // Truthy non-strings 500 on `.strip()`.
        for value in [
            Value::Bool(true),
            Value::Number(42.into()),
            Value::Array(vec![Value::Null]),
            Value::Object({
                let mut map = Map::new();
                map.insert("a".to_owned(), Value::Null);
                map
            }),
        ] {
            assert!(
                is_server_error(url_of(value.clone()).expect_err("truthy non-string")),
                "{value}"
            );
        }
    }
}
