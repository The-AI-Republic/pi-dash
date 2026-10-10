//! The `ops instance` group (PIDASHCONV-809): `configure_instance`,
//! `register_instance`, `ensure_project_pods`, `test_email` and
//! `dry_run_scheduler_migration`.
//!
//! `configure_instance` and `register_instance` execute the D-01 logic
//! (`pidash_jobs::license::commands`) over its Postgres stores; this
//! module supplies the process seams (cwd-relative `package.json`,
//! the GitHub release probe, the AMQP `delay`). The other three
//! commands pair new `pidash_db::ops` reads/writes with
//! `pidash_services::ops` shapes and the reviewed jobs clients
//! (SMTP, RRULE).

use clap::Subcommand;
use pidash_db::config::encryption::Keyring;
use pidash_db::config::registry::ConfigRegistry;
use pidash_db::ops::instance as db_ops;
use pidash_jobs::license::commands as license_commands;
use pidash_services::ops::instance as shapes;

use super::{connect_primary, InstanceFailure};

/// Test seam for the latest-release probe; default is the hardcoded
/// GitHub URL (`register_instance.py:43`).
const RELEASES_URL_ENV: &str = "PIDASH_RELEASES_URL";

/// `timeout=10` on the GitHub GET (`:46`).
const RELEASES_TIMEOUT_SECS: u64 = 10;

/// `timeout=30` on the SMTP connection (`test_email.py:44`).
const SMTP_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Subcommand)]
pub enum InstanceCommand {
    /// Seed instance configuration variables.
    ConfigureInstance,
    /// Register the instance (or refresh its check-in).
    RegisterInstance {
        /// Machine signature.
        machine_signature: String,
    },
    /// Create missing default project pods.
    EnsureProjectPods {
        /// Report what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Send a probe email through the instance SMTP settings.
    TestEmail {
        /// Receiver's email.
        to_email: String,
    },
    /// Dry-run the cron → RRULE migration. Read-only.
    DryRunSchedulerMigration {
        /// Limit to bindings whose workspace has this slug.
        #[arg(long)]
        workspace: Option<String>,
        /// Emit machine-readable JSON instead of human-readable rows.
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(command: InstanceCommand) -> Result<(), InstanceFailure> {
    match command {
        InstanceCommand::ConfigureInstance => configure_instance().await,
        InstanceCommand::RegisterInstance { machine_signature } => {
            register_instance(&machine_signature).await
        }
        InstanceCommand::EnsureProjectPods { dry_run } => ensure_project_pods(dry_run).await,
        InstanceCommand::TestEmail { to_email } => test_email(&to_email).await,
        InstanceCommand::DryRunSchedulerMigration { workspace, json } => {
            dry_run_scheduler_migration(workspace.as_deref(), json).await
        }
    }
}

async fn configure_instance() -> Result<(), InstanceFailure> {
    use license_commands::ProcessEnv;
    let pool = connect_primary().await?;
    let store = license_commands::PgSeedStore::new(pool);
    // Printing is style-free: Django's `style.*` adds color only on a
    // tty, and these commands always run piped in practice.
    let report = license_commands::configure_instance(
        &ProcessEnv,
        &store,
        &ConfigRegistry::build(),
        &Keyring::from_env(),
    )
    .await?;
    for line in &report.lines {
        println!("{}", line.text);
    }
    Ok(())
}

/// `open("package.json")` resolves against the process cwd, like
/// Python (`register_instance.py:33`).
fn read_package_json() -> license_commands::PackageJson {
    let text = std::fs::read_to_string("package.json").unwrap_or_default();
    if text.is_empty() {
        return license_commands::PackageJson::Unreadable;
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(map)) => {
            license_commands::PackageJson::Parsed(map.get("version").map(|value| match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            }))
        }
        _ => license_commands::PackageJson::Unreadable,
    }
}

async fn probe_latest_release() -> license_commands::LatestProbe {
    let url = std::env::var(RELEASES_URL_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| license_commands::RELEASES_URL.to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(RELEASES_TIMEOUT_SECS))
        // `requests` sends a default UA; GitHub 403s UA-less callers.
        .user_agent(concat!("pidash-api/", env!("CARGO_PKG_VERSION")))
        .no_proxy()
        .build();
    let client = match client {
        Ok(client) => client,
        Err(_) => return license_commands::LatestProbe::Failed,
    };
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(_) => return license_commands::LatestProbe::Failed,
    };
    // `raise_for_status` then `.json()` then `.get("tag_name")`
    // (`:44-48`); any failure falls back with the error line.
    let response = match response.error_for_status() {
        Ok(response) => response,
        Err(_) => return license_commands::LatestProbe::Failed,
    };
    let text = match response.text().await {
        Ok(text) => text,
        Err(_) => return license_commands::LatestProbe::Failed,
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(map)) => {
            license_commands::LatestProbe::Tag(map.get("tag_name").map(|value| match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            }))
        }
        _ => license_commands::LatestProbe::Failed,
    }
}

async fn register_instance(machine_signature: &str) -> Result<(), InstanceFailure> {
    use license_commands::ProcessEnv;
    let pool = connect_primary().await?;
    let store = license_commands::PgInstanceStore::new(pool);
    let package = read_package_json();
    let latest = probe_latest_release().await;
    // Python prints the version lines as it resolves them, so they
    // survive a later failure; D-01 buffers them in the report, which
    // `Err` drops. Precompute the same lines from the same inputs for
    // the error path (the success path prints the report as-is).
    let mut early = Vec::new();
    let app_version = std::env::var("APP_VERSION").ok();
    license_commands::resolve_current_version(app_version.as_deref(), &package, &mut early);
    if matches!(latest, license_commands::LatestProbe::Failed) {
        early.push("Error checking for latest version".to_string());
    }
    let report = match license_commands::register_instance(
        &ProcessEnv,
        &store,
        Some(machine_signature),
        &package,
        latest,
        chrono::Utc::now(),
    )
    .await
    {
        Ok(report) => report,
        Err(error) => {
            for line in &early {
                println!("{line}");
            }
            return Err(error.into());
        }
    };
    for line in &report.stdout {
        println!("{line}");
    }
    // `instance_traces.delay()` always runs at the end (`:90`).
    let config = pidash_jobs::AmqpConfig::from_env()
        .map_err(|error| InstanceFailure::Fatal(error.to_string()))?;
    let publisher = pidash_jobs::Publisher::connect(&config)
        .await
        .map_err(|error| InstanceFailure::Fatal(error.to_string()))?;
    let published = publisher
        .publish(&report.delay)
        .await
        .map_err(|error| InstanceFailure::Fatal(error.to_string()));
    let _ = publisher.close().await;
    published?;
    Ok(())
}

async fn ensure_project_pods(dry_run: bool) -> Result<(), InstanceFailure> {
    let pool = connect_primary().await?;
    let missing = db_ops::scan_projects_missing_pods(&pool)
        .await
        .map_err(|error| InstanceFailure::Fatal(error.to_string()))?;
    if missing.is_empty() {
        println!("{}", shapes::ALL_HAVE_PODS);
        return Ok(());
    }
    println!("{}", shapes::found_line(missing.len()));
    for project in &missing {
        println!("{}", shapes::missing_line(&project.id, &project.identifier));
    }
    if dry_run {
        println!("{}", shapes::DRY_RUN_TAIL);
        return Ok(());
    }
    let mut created = 0;
    for project in &missing {
        let name = shapes::pod_name(&project.identifier);
        let creator = shapes::choose_creator(project.project_lead_id, project.default_assignee_id);
        let was_created = db_ops::insert_default_pod(&pool, project, &name, creator)
            .await
            .map_err(|error| InstanceFailure::Fatal(error.to_string()))?;
        if was_created {
            created += 1;
        }
    }
    println!("{}", shapes::created_line(created));
    Ok(())
}

async fn test_email(to_email: &str) -> Result<(), InstanceFailure> {
    use pidash_jobs::tasks_mail::mail_send::{resolve_smtp, smtp_send, strip_tags, OutgoingMail};
    if to_email.is_empty() {
        return Err(InstanceFailure::Command(
            "Receiver email is required".to_string(),
        ));
    }
    let pool = connect_primary().await?;
    let store = pidash_db::config::PgConfigStore::new(pool);
    // Resolution and `int(EMAIL_PORT)` run outside the `try`
    // (`test_email.py:27-45`), so their failures are fatal, like the
    // oracle's tracebacks.
    let values = pidash_services::license::config::get_email_configuration(
        &store,
        &Keyring::from_env(),
        &ConfigRegistry::build(),
    )
    .await
    .map_err(|error| InstanceFailure::Fatal(error.to_string()))?;
    let smtp = resolve_smtp(&values).map_err(InstanceFailure::Fatal)?;
    let html = shapes::TEST_TEMPLATE_HTML.to_string();
    let text = strip_tags(&html);
    println!("{}", shapes::TRYING_LINE);
    let send = tokio::time::timeout(
        std::time::Duration::from_secs(SMTP_TIMEOUT_SECS),
        smtp_send(
            &smtp,
            &OutgoingMail {
                to: to_email.to_string(),
                subject: shapes::TEST_SUBJECT.to_string(),
                text_body: text,
                html_body: html,
            },
        ),
    )
    .await;
    match send {
        Ok(Ok(())) => println!("{}", shapes::SENT_LINE),
        Ok(Err(error)) => println!("{}", shapes::delivery_error_line(&error)),
        Err(_) => println!("{}", shapes::delivery_error_line("timed out")),
    }
    Ok(())
}

async fn dry_run_scheduler_migration(
    workspace: Option<&str>,
    as_json: bool,
) -> Result<(), InstanceFailure> {
    use pidash_jobs::tasks_ticker::rrule::{cron_to_rrule, next_fire_from_rrule};
    let pool = connect_primary().await?;
    if !db_ops::has_cron_column(&pool)
        .await
        .map_err(|error| InstanceFailure::Fatal(error.to_string()))?
    {
        println!("{}", shapes::CRON_GONE_NOTICE);
        return Ok(());
    }
    let rows = db_ops::fetch_dry_run_rows(&pool, workspace)
        .await
        .map_err(|error| InstanceFailure::Fatal(error.to_string()))?;
    let now = chrono::Utc::now();
    let mut entries = Vec::with_capacity(rows.len());
    for row in &rows {
        // `(row.get("cron") or "").strip()` (`:125`).
        let cron = row.cron.as_deref().unwrap_or("").trim().to_string();
        let mut entry = shapes::DryRunEntry {
            binding_id: row.id.to_string(),
            workspace: row.workspace_slug.clone(),
            enabled: row.enabled,
            cron,
            cron_next_fire: None,
            rrule: None,
            rrule_next_fire: None,
            verdict: shapes::DryRunVerdict::Fail,
            reason: None,
        };
        let rrule = match cron_to_rrule(&entry.cron) {
            Ok(rrule) => rrule,
            Err(error) => {
                entry.reason = Some(format!("conversion error: {error}"));
                entries.push(entry);
                continue;
            }
        };
        entry.rrule = Some(rrule.clone());
        // Anchor `dtstart` at the next valid firing after `created_at`,
        // mirroring the migration (`:157-162`).
        let anchor = row.created_at - chrono::Duration::seconds(1);
        let dtstart = next_fire_from_rrule(anchor, &rrule, "UTC", &[], &[], anchor);
        let Some(dtstart) = dtstart else {
            entry.reason = Some("rrule produced no next-fire from created_at".to_string());
            entries.push(entry);
            continue;
        };
        let rrule_next = next_fire_from_rrule(dtstart, &rrule, "UTC", &[], &[], now);
        entry.rrule_next_fire = rrule_next.as_ref().map(shapes::iso_utc);
        // No croniter exists in Rust, so the cron side is always
        // absent — the documented end state (croniter is slated for
        // removal together with this throwaway command). Every
        // convertible row therefore lands on FAIL, like the oracle
        // with the lazy import missing (`:176-179`).
        let cron_next: Option<chrono::DateTime<chrono::Utc>> = None;
        match (cron_next, rrule_next) {
            (None, _) | (_, None) => {
                entry.reason = Some("could not compute one or both next-fires".to_string());
            }
            (Some(cron_fire), Some(rrule_fire)) => {
                let diff = (rrule_fire.timestamp_micros() - cron_fire.timestamp_micros()) as f64
                    / 1_000_000.0;
                if diff.abs() <= shapes::MATCH_TOLERANCE_SECS {
                    entry.verdict = shapes::DryRunVerdict::Match;
                } else {
                    entry.verdict = shapes::DryRunVerdict::Mismatch;
                    entry.reason = Some(format!("next-fire diff = {diff:.0}s"));
                }
            }
        }
        entries.push(entry);
    }
    let counts = shapes::DryRunCounts::of(&entries);
    if as_json {
        print!("{}", shapes::format_json(&entries, counts));
    } else {
        for line in shapes::format_human(&entries, counts) {
            println!("{line}");
        }
    }
    Ok(())
}
