//! `delete_old_s3_link` handler: exporter S3-link expiry (PIDASHCONV-858).
//!
//! Port of `apps/api/pi_dash/bgtasks/exporter_expired_task.py:22-53`.
//! The scan SQL ([`scan_sql_for`]) and the per-row plan
//! ([`pidash_services::tasks_cleanup::exports::plan_expiry_deletes`])
//! are reused, never re-implemented; this module owns the task name,
//! the store traits, the live S3 deleter and [`register_expiry`].
//!
//! Celery name, exactly as beat calls it (twice daily):
//!
//! * `pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link`
//!
//! # Fail-loud verdict mapping (translate, don't redesign)
//!
//! Python has no `try`: an S3 error, a scan error or a per-row `update`
//! error escapes the task, which Celery records as a failure (the bare
//! `@shared_task` carries no autoretry). Rows before the failure keep
//! their progress (each row's `url` clears right after its delete).
//! [`run_expiry`] mirrors that: sequential, fail-fast, partial progress
//! kept, and the handler maps every error to `Ok(Verdict::Fail)` —
//! never `Err`, which would retry.
//!
//! In particular an S3 outage or a misconfigured backend FAILS the
//! task; it never ack-and-clears (that silent divergence is what kept
//! this task Python-owned in PIDASHCONV-835).
//!
//! # S3 arms (two, not three)
//!
//! The task builds one client per run: MinIO iff `USE_MINIO`
//! (`:28-35`), else regional (`:36-42`). The non-MinIO arm passes
//! `region_name` only — it IGNORES `AWS_S3_ENDPOINT_URL`, unlike
//! `upload_to_s3`, whose three arms
//! ([`pidash_services::tasks_cleanup::exports::select_s3_branch`])
//! include an explicit-endpoint branch. So this deleter branches on
//! `USE_MINIO` alone: MinIO signs path-style against the verbatim
//! endpoint URL, regional signs virtual-hosted against the derived AWS
//! endpoint ([`LiveS3Deleter::signing_storage`]).
//!
//! # Region handling (probed, boto3 1.34.110 / botocore 1.34.162)
//!
//! * MinIO arm: no region is passed, so botocore's chain applies —
//!   `AWS_DEFAULT_REGION` verbatim when set (even `""`), the
//!   `AWS_REGION` env IGNORED, else the shared-config tail, else
//!   `us-east-1`. [`minio_signing_region`] implements exactly that
//!   over the resolved settings. Requests are path-style
//!   (`DELETE /{bucket}/{key}`).
//! * Regional arm: `region_name=settings.AWS_REGION` verbatim (`""`
//!   when unset, i.e. explicitly passed empty) — `""` fails client
//!   construction (`ValueError: Invalid endpoint:
//!   https://s3..amazonaws.com`), which fails the task here too.
//!   `us-east-1` signs against the global endpoint, any other region
//!   against its regional virtual-hosted endpoint.
//!
//! # Deliberate deviations (documented, interop-safe)
//!
//! * Single attempt, fail fast: botocore follows 301 region redirects
//!   and retries 5xx/transport errors; this client attempts every
//!   delete once (the `ReqwestS3` precedent in
//!   `pidash-services::ops::storage`). Correct-region configs never
//!   redirect; every other status fails the task on both backends.
//! * `x-amz-content-sha256` carries the real payload hash (the
//!   `pidash-storage` precedent; botocore sends `UNSIGNED-PAYLOAD`).
//!   S3 and MinIO accept either.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-3 (`exporter_expired_task.py:48-53`): the MINIO/non-MINIO
//!   `delete_object` branches are identical calls, and the row's `url`
//!   is cleared even when `key` was falsy (no S3 call happened).
//!   [`run_expiry`] keeps both via `plan_expiry_deletes`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use pidash_db::config::{Settings, StorageSettings};
use pidash_services::tasks_cleanup::exports as logic;
use pidash_types::tasks_cleanup::exports_dto::DELETE_OLD_S3_LINK_TASK_NAME;

use crate::worker::{Handler, Registry, Verdict};

/// `pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link`
/// (`exporter_expired_task.py:22-23`, bare `@shared_task`, no arguments).
pub const TASK_DELETE_OLD_S3_LINK: &str = DELETE_OLD_S3_LINK_TASK_NAME;

/// `filter(id).update(url=None)` (`:53`): the `SoftDeletionManager`
/// base filter rides along, so the write keeps its `deleted_at` guard;
/// no `updated_at` bump (`QuerySet.update` never touches `auto_now`).
pub const CLEAR_EXPIRED_URL_SQL: &str =
    "UPDATE \"exporters\" SET \"url\" = NULL WHERE \"id\" = $1 AND \"deleted_at\" IS NULL";

/// Render the expiry scan for one run: the fixture-pinned
/// [`logic::delete_old_s3_link_sql`] over the Django-rendered cutoff
/// (`timezone.now() - 8d`, evaluated once per run like the Python
/// `filter()` call).
///
/// The recorded fixture text is Django's `str(query)` rendering, which
/// leaves the datetime literal unquoted — not executable SQL. The
/// cutoff is quoted here so the composed scan executes; it is an
/// internally-generated fixed-charset timestamp (`%Y-%m-%d
/// %H:%M:%S[.ffffff]+00:00`), never caller input, so interpolation is
/// safe. The unit test below pins the composed statement to the
/// fixture shape (cutoff quoted for execution).
pub fn scan_sql_for(now: DateTime<Utc>) -> String {
    let cutoff = super::format_expiry_cutoff(now);
    logic::delete_old_s3_link_sql(&format!("'{cutoff}'"))
}

/// The database surface the task needs, mirroring the Django calls.
/// Errors are plain text; [`run_expiry`] fails the task on any of them.
pub trait ExpiryStore: Send + Sync {
    /// The expiry scan (`:25-27`), in scan order.
    fn expired_exporters(
        &self,
        now: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<Vec<(Option<String>, Uuid)>, String>> + Send;
    /// `filter(id).update(url=None)` (`:53`), unconditionally.
    fn clear_url(&self, id: Uuid) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

/// One S3 object delete, fail-loud. Separate from
/// [`super::assets::ObjectStore`]: that seam degrades inside the impl
/// (log + ignore, the `S3Storage` swallow contract — its tasks never
/// fail on S3), while this task has no `try`, so an S3 failure fails
/// the task. Sharing the trait would carry two opposing contracts.
pub trait ObjectDeleter: Send + Sync {
    /// The eager `boto3.client(...)` build (`:28-43`), before the scan:
    /// a region boto3 would reject at build fails the task with zero
    /// rows touched (probed `ValueError`/`InvalidRegionError`). The
    /// default never fails; deleters whose client construction can
    /// fail override it.
    fn check_client(&self) -> Result<(), String> {
        Ok(())
    }
    /// Delete one object. `Err` carries the S3 failure text, which
    /// parks the task (never retries).
    fn delete_object(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

/// MinIO-arm signing region: botocore's chain when no region is passed
/// (probed — see the module docs). `aws_region` is the `AWS_REGION`
/// env (only its presence matters: the value is ignored in this arm);
/// `aws_default_region` is the `AWS_DEFAULT_REGION` env verbatim (even
/// `""`); `chained` is the resolved `storage.region`, which already
/// holds exactly this chain — tail included — when `AWS_REGION` is
/// unset, so it is reused as-is there.
///
/// When `AWS_REGION` IS set, the value is ignored and the region is
/// `AWS_DEFAULT_REGION` verbatim when set (even `""`), else
/// `us-east-1`. The shared-config tail is not consulted in that cell
/// (it would need the foundation parser duplicated, for a machine
/// with `AWS_REGION` set, `AWS_DEFAULT_REGION` unset AND a live
/// shared-config file).
pub fn minio_signing_region(
    aws_region: Option<&str>,
    aws_default_region: Option<&str>,
    chained: &str,
) -> String {
    match aws_region {
        None => chained.to_owned(),
        Some(_) => aws_default_region.unwrap_or("us-east-1").to_owned(),
    }
}

/// Build the bounded S3 HTTP client: botocore's defaults apply (the
/// Python task passes no timeout `Config`), 60s to connect and 60s to
/// read, so a wedged object store fails the task instead of hanging
/// the worker slot forever (PIDASHCONV-1291; the PIDASHCONV-1156
/// precedent for the app_assets copy).
fn build_s3_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(60))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("s3 misconfigured: cannot build s3 client: {error}"))
}

/// Live S3 deleter over [`pidash_storage`] SigV4 + reqwest (the one
/// shared signer — no AWS SDK): one signed `DELETE` per key, executed
/// for real. Construction does no I/O (the client connects lazily),
/// so registration stays provable with no network; every failure —
/// config, transport, error status — surfaces per call and fails the
/// task, exactly like the boto3 client built inside the Python task.
#[derive(Debug, Clone)]
pub struct LiveS3Deleter {
    client: Result<reqwest::Client, String>,
    storage: StorageSettings,
    minio_region: String,
    region_raw: String,
}

impl LiveS3Deleter {
    /// Build from the resolved worker settings. `AWS_REGION` /
    /// `AWS_DEFAULT_REGION` are read from the process environment for
    /// the unchained values `Settings` does not retain (both are
    /// env-tier keys, and the worker resolves `Settings` from the same
    /// environment first — a garbage `USE_MINIO` already failed boot
    /// there, like Django's import-time `int()`). Construction never
    /// fails: the client build outcome is stored and the eager client
    /// check runs per task run ([`ObjectDeleter::check_client`]), like
    /// the `boto3.client(...)` build inside the Python task.
    pub fn from_settings(settings: &Settings) -> Self {
        let aws_region = std::env::var("AWS_REGION").ok();
        let aws_default_region = std::env::var("AWS_DEFAULT_REGION").ok();
        Self {
            client: build_s3_client(),
            storage: settings.storage.clone(),
            minio_region: minio_signing_region(
                aws_region.as_deref(),
                aws_default_region.as_deref(),
                &settings.storage.region,
            ),
            region_raw: aws_region.unwrap_or_default(),
        }
    }

    /// The arm-adjusted storage the signer resolves: both arms ride
    /// the shared [`pidash_storage::resolve_server_endpoint`] branches
    /// with task-correct inputs, so no URL building is duplicated here.
    fn signing_storage(&self) -> Result<StorageSettings, String> {
        let mut storage = self.storage.clone();
        if storage.use_minio {
            let endpoint = storage
                .endpoint_url
                .clone()
                .filter(|url| !url.is_empty())
                .ok_or_else(|| {
                    "s3 misconfigured: USE_MINIO is set but neither AWS_S3_ENDPOINT_URL nor \
                     MINIO_ENDPOINT_URL is"
                        .to_owned()
                })?;
            // The MinIO arm signs path-style against the verbatim
            // endpoint (probed `DELETE /{bucket}/{key}`); the shared
            // EndpointUrl branch is exactly that shape, path included.
            // Region is the default chain (no region is passed).
            storage.use_minio = false;
            storage.endpoint_url = Some(endpoint);
            storage.region = self.minio_region.clone();
        } else {
            // The non-MinIO arm ignores the endpoint URL (`:36-42` —
            // unlike `upload_to_s3`'s EndpointUrl arm): region only,
            // verbatim-or-empty. `""` fails endpoint derivation
            // (`Config`, like boto3's `ValueError: Invalid endpoint`).
            storage.endpoint_url = None;
            storage.region = self.region_raw.clone();
        }
        Ok(storage)
    }

    /// Sign one delete. Pure given the timestamps (unit-testable
    /// without I/O); [`ObjectDeleter::delete_object`] executes it.
    /// Scheme/host are dummy: both arms resolve without them (the
    /// MinIO arm rides the EndpointUrl branch, the regional arm the
    /// Region branch — see [`Self::signing_storage`]).
    fn signed_request_for(
        &self,
        key: &str,
        amz_date: &str,
        date_stamp: &str,
    ) -> Result<pidash_storage::SignedRequest, String> {
        let storage = self.signing_storage()?;
        pidash_storage::sign_delete(&storage, "", "", key, amz_date, date_stamp)
            .map_err(|error| format!("s3 delete {key}: {error}"))
    }
}

impl ObjectDeleter for LiveS3Deleter {
    fn check_client(&self) -> Result<(), String> {
        // Pure (no I/O): the stored client build outcome first — a
        // build failure fails the run here, like the `boto3.client(...)`
        // build inside the Python task, never a silent unbounded client.
        self.client
            .as_ref()
            .map(|_| ())
            .map_err(|error| error.clone())?;
        // Pure (no I/O): botocore validates the region at client
        // construction, before the loop. The regional arm fails on an
        // invalid name (`InvalidRegionError`) and on `""` (`ValueError:
        // Invalid endpoint`); the MinIO arm fails only on an invalid
        // chained name — a missing endpoint builds fine there.
        let region = if self.storage.use_minio {
            &self.minio_region
        } else {
            &self.region_raw
        };
        if !pidash_db::config::is_valid_region_name(region) {
            return Err(format!(
                "s3 misconfigured: region {region:?} is not a valid host label"
            ));
        }
        if !self.storage.use_minio && region.is_empty() {
            return Err(
                "s3 misconfigured: empty region with no endpoint URL to derive from".to_owned(),
            );
        }
        Ok(())
    }

    async fn delete_object(&self, key: &str) -> Result<(), String> {
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();
        let signed = self.signed_request_for(key, &amz_date, &date_stamp)?;
        // Unreachable in production: [`run_expiry`] runs `check_client`
        // before the loop, so a stored build error already failed the
        // run; direct callers get the same failure text per call.
        let client = self
            .client
            .as_ref()
            .map_err(|error| format!("s3 delete {key}: {error}"))?;
        let response = client
            .delete(signed.url)
            .header("Authorization", signed.authorization)
            .header("x-amz-date", signed.amz_date)
            .header("x-amz-content-sha256", signed.content_sha256)
            .send()
            .await
            .map_err(|error| format!("s3 delete {key}: transport failure: {error}"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        // boto3 raises `ClientError` on any error status (including a
        // missing bucket); the task fails either way.
        let body = response.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(200).collect();
        Err(format!(
            "s3 delete {key}: service error {status}: {snippet}"
        ))
    }
}

/// What one run did. Counts exist for tests; Django logs nothing, so a
/// successful run stays silent here too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExpiryOutcome {
    pub deleted: u64,
    pub cleared: u64,
}

/// Run the expiry: scan once, then per row (scan order) delete the S3
/// object iff the plan says so and clear `url` unconditionally
/// (`:45-53`, BUG-3 kept). Fail-fast with partial progress kept: the
/// first S3 or DB error aborts the run and the caller parks the task.
pub async fn run_expiry<S: ExpiryStore, D: ObjectDeleter>(
    store: &S,
    deleter: &D,
    now: DateTime<Utc>,
) -> Result<ExpiryOutcome, String> {
    // Eager client build first (`:28-43` runs before the loop): a bad
    // region fails here, with zero rows touched.
    deleter.check_client()?;
    let rows = store.expired_exporters(now).await?;
    let actions = logic::plan_expiry_deletes(
        &rows
            .iter()
            .map(|(key, id)| (key.clone(), id.to_string()))
            .collect::<Vec<_>>(),
    );
    let mut outcome = ExpiryOutcome::default();
    // `plan_expiry_deletes` is a positional map, so zipping reattaches
    // the typed ids without re-parsing them.
    for ((_, id), action) in rows.iter().zip(actions.iter()) {
        if action.delete_object {
            // `delete_object` is true only for `Some(non-empty)` keys,
            // so this always binds; `url` clears either way (BUG-3).
            if let Some(key) = action.key.as_deref() {
                deleter.delete_object(key).await?;
                outcome.deleted += 1;
            }
        }
        store.clear_url(*id).await?;
        outcome.cleared += 1;
    }
    Ok(outcome)
}

/// Postgres [`ExpiryStore`]: the fixture-pinned scan plus the per-row
/// `url` clear. Every table name is a literal, never caller input.
#[derive(Debug, Clone)]
pub struct PgExpiryStore {
    pool: sqlx::PgPool,
}

impl PgExpiryStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

fn db_error(context: &str, error: sqlx::Error) -> String {
    format!("{context}: {error}")
}

impl ExpiryStore for PgExpiryStore {
    async fn expired_exporters(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<(Option<String>, Uuid)>, String> {
        sqlx::query_as::<_, (Option<String>, Uuid)>(&scan_sql_for(now))
            .fetch_all(&self.pool)
            .await
            .map_err(|error| db_error("expired_exporters", error))
    }

    async fn clear_url(&self, id: Uuid) -> Result<(), String> {
        sqlx::query(CLEAR_EXPIRED_URL_SQL)
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| db_error("clear_url", error))
    }
}

/// Install the expiry handler. Every error path parks (`Ok(Fail)`):
/// Python has no autoretry on this task, so retries never trigger.
pub fn register_expiry<D: ObjectDeleter + 'static>(
    registry: &mut Registry,
    pool: sqlx::PgPool,
    deleter: Arc<D>,
) {
    let handler: Handler = Arc::new(move |_job: crate::queue::JobRow| {
        let pool = pool.clone();
        let deleter = deleter.clone();
        Box::pin(async move {
            let store = PgExpiryStore::new(pool);
            match run_expiry(&store, deleter.as_ref(), Utc::now()).await {
                Ok(_) => Ok(Verdict::Ack),
                Err(error) => Ok(Verdict::Fail { error }),
            }
        })
    });
    registry.register(TASK_DELETE_OLD_S3_LINK, handler);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn fixture_filter_sql() -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/tasks_cleanup/exports.json");
        let fx: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("exports fixture exists"))
                .expect("exports fixture is valid JSON");
        fx["delete_old_s3_link"]["filter_sql"]
            .as_str()
            .expect("filter_sql")
            .to_owned()
    }

    #[test]
    fn wire_name_matches_django() {
        assert_eq!(
            TASK_DELETE_OLD_S3_LINK,
            "pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link"
        );
        assert_eq!(TASK_DELETE_OLD_S3_LINK, DELETE_OLD_S3_LINK_TASK_NAME);
    }

    #[test]
    fn scan_sql_chains_fixture_text() {
        // Frozen fixture instant 2026-09-28T06:00:00Z: the composed scan
        // (cutoff render + SQL builder) replays the recorded SQL with
        // the cutoff quoted for execution (the fixture is Django's
        // `str(query)` rendering, which leaves it bare — executing the
        // bare form fails with `syntax error at or near "09"`,
        // PIDASHCONV-858 oracle evidence).
        let now = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 9, 28, 6, 0, 0).unwrap();
        let cutoff = super::super::format_expiry_cutoff(now);
        let quoted = format!("'{cutoff}'");
        let expected = fixture_filter_sql().replacen(&cutoff, &quoted, 1);
        assert!(
            expected.contains(&quoted),
            "fixture must carry the bare cutoff"
        );
        assert_eq!(scan_sql_for(now), expected);
    }

    #[test]
    fn clear_sql_pins_write() {
        assert_eq!(
            CLEAR_EXPIRED_URL_SQL,
            "UPDATE \"exporters\" SET \"url\" = NULL WHERE \"id\" = $1 AND \"deleted_at\" IS NULL"
        );
    }

    #[test]
    fn minio_region_follows_probed_chain() {
        // AWS_REGION unset: the resolved chain (tail included) is reused.
        assert_eq!(minio_signing_region(None, None, "us-east-1"), "us-east-1");
        assert_eq!(
            minio_signing_region(None, Some("eu-west-1"), "eu-west-1"),
            "eu-west-1"
        );
        // AWS_REGION set (even ""): ignored; DEFAULT verbatim (even ""),
        // else us-east-1.
        assert_eq!(
            minio_signing_region(Some("eu-west-1"), None, "eu-west-1"),
            "us-east-1"
        );
        assert_eq!(minio_signing_region(Some(""), None, ""), "us-east-1");
        assert_eq!(
            minio_signing_region(Some("eu-west-1"), Some(""), "eu-west-1"),
            ""
        );
        assert_eq!(
            minio_signing_region(Some(""), Some("eu-west-1"), ""),
            "eu-west-1"
        );
    }

    fn test_storage(use_minio: bool, endpoint_url: Option<&str>, region: &str) -> StorageSettings {
        StorageSettings {
            use_minio,
            minio_endpoint_ssl: false,
            access_key_id: "minioadmin".to_owned(),
            secret_access_key: "minioadmin".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: region.to_owned(),
            endpoint_url: endpoint_url.map(str::to_owned),
            signed_url_expiration_secs: 3600,
        }
    }

    fn deleter_with(
        use_minio: bool,
        endpoint_url: Option<&str>,
        chained_region: &str,
        minio_region: &str,
        region_raw: &str,
    ) -> LiveS3Deleter {
        LiveS3Deleter {
            client: build_s3_client(),
            storage: test_storage(use_minio, endpoint_url, chained_region),
            minio_region: minio_region.to_owned(),
            region_raw: region_raw.to_owned(),
        }
    }

    fn auth_scope(auth: &str) -> String {
        // "AWS4-HMAC-SHA256 Credential=KEY/{scope}, SignedHeaders=..., ..."
        let cred = auth.split(' ').nth(1).expect("credential");
        let scope = cred.split(',').next().expect("scope");
        scope.split('/').skip(1).collect::<Vec<_>>().join("/")
    }

    #[test]
    fn minio_arm_signs_path_style_against_endpoint() {
        // Contract env: USE_MINIO=1, endpoint LocalStack, AWS_REGION
        // unset (chain us-east-1) — probed `DELETE /{bucket}/{key}`.
        let deleter = deleter_with(
            true,
            Some("http://localhost:4566"),
            "us-east-1",
            "us-east-1",
            "",
        );
        let signed = deleter
            .signed_request_for("contract-key.zip", "20261017T000000Z", "20261017")
            .expect("signs");
        assert_eq!(signed.method, "DELETE");
        assert_eq!(signed.url, "http://localhost:4566/uploads/contract-key.zip");
        assert!(auth_scope(&signed.authorization).starts_with("20261017/us-east-1/s3/"));
    }

    #[test]
    fn minio_arm_requires_endpoint() {
        let deleter = deleter_with(true, None, "us-east-1", "us-east-1", "");
        let error = deleter
            .signed_request_for("k.zip", "20261017T000000Z", "20261017")
            .expect_err("no endpoint fails");
        assert!(error.contains("USE_MINIO"), "{error}");
    }

    #[test]
    fn regional_arm_ignores_endpoint() {
        // USE_MINIO=0 with an endpoint set: Django ignores it (`:36-42`)
        // and goes regional — so does the deleter.
        let deleter = deleter_with(
            false,
            Some("http://localhost:4566"),
            "eu-west-1",
            "us-east-1",
            "eu-west-1",
        );
        let signed = deleter
            .signed_request_for("k.zip", "20261017T000000Z", "20261017")
            .expect("signs");
        assert_eq!(
            signed.url,
            "https://uploads.s3.eu-west-1.amazonaws.com/k.zip"
        );
        assert!(auth_scope(&signed.authorization).starts_with("20261017/eu-west-1/s3/"));
    }

    #[test]
    fn regional_arm_uses_global_endpoint_for_us_east_1() {
        let deleter = deleter_with(false, None, "us-east-1", "us-east-1", "us-east-1");
        let signed = deleter
            .signed_request_for("k.zip", "20261017T000000Z", "20261017")
            .expect("signs");
        assert_eq!(signed.url, "https://uploads.s3.amazonaws.com/k.zip");
    }

    #[test]
    fn regional_arm_empty_region_fails_like_boto3() {
        // region_name="" explicitly passed: boto3 fails client
        // construction (`ValueError: Invalid endpoint`) — the task fails.
        let deleter = deleter_with(false, None, "us-east-1", "us-east-1", "");
        deleter
            .signed_request_for("k.zip", "20261017T000000Z", "20261017")
            .expect_err("empty region fails");
    }

    // --- run_expiry fakes ------------------------------------------------

    struct FakeStore {
        rows: Vec<(Option<String>, Uuid)>,
        cleared: Mutex<Vec<Uuid>>,
        fail_scan: Option<String>,
        fail_clear: HashMap<Uuid, String>,
    }

    impl ExpiryStore for FakeStore {
        async fn expired_exporters(
            &self,
            _now: DateTime<Utc>,
        ) -> Result<Vec<(Option<String>, Uuid)>, String> {
            if let Some(error) = &self.fail_scan {
                return Err(error.clone());
            }
            Ok(self.rows.clone())
        }

        async fn clear_url(&self, id: Uuid) -> Result<(), String> {
            if let Some(error) = self.fail_clear.get(&id) {
                return Err(error.clone());
            }
            self.cleared.lock().unwrap().push(id);
            Ok(())
        }
    }

    struct FakeDeleter {
        deleted: Mutex<Vec<String>>,
        fail_on: HashMap<String, String>,
        fail_client: Option<String>,
    }

    impl ObjectDeleter for FakeDeleter {
        fn check_client(&self) -> Result<(), String> {
            match &self.fail_client {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }

        async fn delete_object(&self, key: &str) -> Result<(), String> {
            if let Some(error) = self.fail_on.get(key) {
                return Err(error.clone());
            }
            self.deleted.lock().unwrap().push(key.to_owned());
            Ok(())
        }
    }

    fn store_with(rows: Vec<(Option<String>, Uuid)>) -> FakeStore {
        FakeStore {
            rows,
            cleared: Mutex::new(Vec::new()),
            fail_scan: None,
            fail_clear: HashMap::new(),
        }
    }

    fn deleter() -> FakeDeleter {
        FakeDeleter {
            deleted: Mutex::new(Vec::new()),
            fail_on: HashMap::new(),
            fail_client: None,
        }
    }

    #[test]
    fn check_client_mirrors_boto3_build_rules() {
        // Regional arm: "" fails (ValueError), garbage fails
        // (InvalidRegionError), real regions pass.
        assert!(deleter_with(false, None, "us-east-1", "us-east-1", "")
            .check_client()
            .is_err());
        assert!(deleter_with(false, None, "us-east-1", "us-east-1", "!!!")
            .check_client()
            .is_err());
        assert!(
            deleter_with(false, None, "us-east-1", "us-east-1", "eu-west-1")
                .check_client()
                .is_ok()
        );
        // MinIO arm: garbage chained region fails, "" passes, and a
        // missing endpoint builds fine (boto3 only fails per call).
        assert!(
            deleter_with(true, Some("http://localhost:4566"), "x", "!!!", "")
                .check_client()
                .is_err()
        );
        assert!(
            deleter_with(true, Some("http://localhost:4566"), "x", "", "")
                .check_client()
                .is_ok()
        );
        assert!(deleter_with(true, None, "us-east-1", "us-east-1", "")
            .check_client()
            .is_ok());
    }

    #[test]
    fn client_build_error_fails_like_boto3_construction() {
        // A stored client build failure fails the run in `check_client`
        // (zero rows touched), like the `boto3.client(...)` build inside
        // the Python task — never a silent unbounded client.
        let mut deleter = deleter_with(false, None, "us-east-1", "us-east-1", "eu-west-1");
        deleter.client = Err("s3 misconfigured: cannot build s3 client: probe".to_owned());
        let error = deleter
            .check_client()
            .expect_err("build error fails the run");
        assert!(error.contains("cannot build s3 client"), "{error}");
    }

    #[tokio::test]
    async fn delete_object_surfaces_client_build_error() {
        // Direct callers (no `check_client`) get the same failure text
        // per call instead of a panic.
        let mut deleter = deleter_with(false, None, "us-east-1", "us-east-1", "eu-west-1");
        deleter.client = Err("s3 misconfigured: cannot build s3 client: probe".to_owned());
        let error = deleter
            .delete_object("k.zip")
            .await
            .expect_err("build error fails the call");
        assert!(
            error.contains("s3 delete k.zip: s3 misconfigured"),
            "{error}"
        );
    }

    /// A wedged object store must not hang the expiry delete forever
    /// (PIDASHCONV-1291). The deleter client is bounded like botocore's
    /// defaults (60s connect / 60s read), so a black-hole peer — one that
    /// accepts the connection and never answers — fails the delete
    /// instead of holding the worker slot. Before the fix this never
    /// returns (the test would time out at 90s).
    #[tokio::test]
    async fn delete_object_is_bounded_against_a_black_hole() {
        use std::time::Duration;
        // Accept every connection and hold it open without ever replying.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind black hole");
        let addr = listener.local_addr().expect("black-hole addr");
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                held.push(sock);
            }
        });
        // MinIO arm against the black hole (path-style
        // `DELETE /{bucket}/{key}`).
        let endpoint = format!("http://{addr}");
        let deleter = deleter_with(true, Some(&endpoint), "us-east-1", "us-east-1", "");
        let outcome = tokio::time::timeout(
            Duration::from_secs(90),
            deleter.delete_object("contract-expiry.zip"),
        )
        .await;
        let result = outcome.expect("delete_object never returned: deleter is unbounded");
        let error = result.expect_err("black hole must fail the delete");
        // The request reached the wire and the read bound fired — not a
        // signing fast-fail. (No elapsed-time assert: wall-clock bounds
        // flake when the shared runner starves this runtime's timers; the
        // 90s cap plus this error text pin the ~60s bound.)
        assert!(error.contains("transport failure"), "{error}");
    }

    #[tokio::test]
    async fn truthy_key_deletes_falsy_key_skips_url_always_clears() {
        let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let store = store_with(vec![
            (Some("k.zip".to_owned()), ids[0]),
            (Some(String::new()), ids[1]),
            (None, ids[2]),
        ]);
        let s3 = deleter();
        let outcome = run_expiry(&store, &s3, Utc::now()).await.expect("ok");
        assert_eq!(
            outcome,
            ExpiryOutcome {
                deleted: 1,
                cleared: 3
            }
        );
        assert_eq!(*s3.deleted.lock().unwrap(), vec!["k.zip".to_owned()]);
        assert_eq!(*store.cleared.lock().unwrap(), ids.to_vec());
    }

    #[tokio::test]
    async fn s3_failure_parks_with_partial_progress_kept() {
        let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let store = store_with(vec![
            (Some("k1.zip".to_owned()), ids[0]),
            (Some("k2.zip".to_owned()), ids[1]),
            (Some("k3.zip".to_owned()), ids[2]),
        ]);
        let mut s3 = deleter();
        s3.fail_on.insert(
            "k2.zip".to_owned(),
            "s3 delete k2.zip: Service Unavailable".to_owned(),
        );
        let error = run_expiry(&store, &s3, Utc::now())
            .await
            .expect_err("fails");
        assert!(error.contains("k2.zip"), "{error}");
        // Fail-fast: row 1 done, row 2's delete failed (url kept), row 3
        // untouched.
        assert_eq!(*s3.deleted.lock().unwrap(), vec!["k1.zip".to_owned()]);
        assert_eq!(*store.cleared.lock().unwrap(), vec![ids[0]]);
    }

    #[tokio::test]
    async fn bad_client_fails_before_scan_with_zero_rows_touched() {
        // `boto3.client(...)` runs before the loop: even falsy-key rows
        // (which need no S3 call) keep their url when the build fails.
        let ids = [Uuid::new_v4(), Uuid::new_v4()];
        let store = store_with(vec![(None, ids[0]), (Some(String::new()), ids[1])]);
        let mut s3 = deleter();
        s3.fail_client = Some("s3 misconfigured: empty region".to_owned());
        run_expiry(&store, &s3, Utc::now())
            .await
            .expect_err("fails");
        assert!(s3.deleted.lock().unwrap().is_empty());
        assert!(store.cleared.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn scan_failure_touches_nothing() {
        let ids = [Uuid::new_v4()];
        let mut store = store_with(vec![(Some("k.zip".to_owned()), ids[0])]);
        store.fail_scan = Some("expired_exporters: connection reset".to_owned());
        let s3 = deleter();
        run_expiry(&store, &s3, Utc::now())
            .await
            .expect_err("fails");
        assert!(s3.deleted.lock().unwrap().is_empty());
        assert!(store.cleared.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn clear_failure_parks_after_delete() {
        let ids = [Uuid::new_v4()];
        let mut store = store_with(vec![(Some("k.zip".to_owned()), ids[0])]);
        store
            .fail_clear
            .insert(ids[0], "clear_url: connection reset".to_owned());
        let s3 = deleter();
        run_expiry(&store, &s3, Utc::now())
            .await
            .expect_err("fails");
        // The delete already happened (Python deletes before the update).
        assert_eq!(*s3.deleted.lock().unwrap(), vec!["k.zip".to_owned()]);
        assert!(store.cleared.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_scan_acks() {
        let store = store_with(Vec::new());
        let s3 = deleter();
        let outcome = run_expiry(&store, &s3, Utc::now()).await.expect("ok");
        assert_eq!(outcome, ExpiryOutcome::default());
    }
}
