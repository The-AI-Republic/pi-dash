//! Prompting ops commands: reseed + revalidate CLI edge (D-37, stage 7).
//!
//! Ports `apps/api/pi_dash/prompting/management/commands/` (4 files):
//! argv (`--force` / `--clear`), the stdout lines, and exit 0 on success.
//! Decisions come from `pidash_services::ops::prompting`, SQL from
//! `pidash_db::ops::prompting` — this module only connects, applies, and
//! prints.
//!
//! Django's `self.style.*` helpers add ANSI color only on a tty; off-tty
//! (pipes, which is all this surface is ever tested or scripted under)
//! they print the plain string plus a newline — exactly what `println!`
//! emits. Nothing here logs: any `tracing` event would land on stderr
//! and break the byte-identical stderr contract (Django writes nothing
//! to stderr on these paths).

use clap::Args;

/// `--force` for the three reseed commands
/// (`reseed_*_template.py:17-22`). The help text is the command's own
/// Django string; only the template word differs per command.
#[derive(Debug, Args)]
pub struct ReseedArgs {
    /// Overwrite the body of the existing global default row.
    #[arg(long)]
    pub force: bool,
}

/// `--clear` for `revalidate_section_overrides`
/// (`revalidate_section_overrides.py:26-31`).
#[derive(Debug, Args)]
pub struct RevalidateArgs {
    /// Clear needs_attention on overrides that now validate cleanly.
    #[arg(long)]
    pub clear: bool,
}

/// Connect the primary pool, quietly (no log lines: stderr must stay
/// empty on success). `DATABASE_URL` must be a TCP `postgres://` URL,
/// same as `serve`/`worker`.
async fn connect_primary() -> Result<sqlx::PgPool, Box<dyn std::error::Error>> {
    let db = pidash_db::DbConfig::from_env()?;
    let pools = pidash_db::Pools::connect(&db, None).await?;
    Ok(pools.primary().clone())
}

/// `reseed_{default,review,test}_template [--force]`
/// (`reseed_*_template.py:24-26`): `seed_*(force)` + the
/// `"<name> template: <result>"` line on stdout.
pub async fn run_reseed(
    kind: pidash_services::ops::prompting::TemplateKind,
    args: ReseedArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    use pidash_services::ops::prompting::plan_reseed;
    use pidash_services::prompting::seed::{ExistingTemplate, SeedOutcome};

    let pool = connect_primary().await?;
    let existing_row = pidash_db::ops::prompting::fetch_global_template(&pool, kind.name()).await?;
    let existing = existing_row.as_ref().map(|row| ExistingTemplate {
        body: row.body.clone(),
        version: Some(row.version),
    });
    let plan = plan_reseed(kind, existing.as_ref(), args.force);
    match (plan.outcome, plan.version) {
        (SeedOutcome::Created, Some(_)) => {
            pidash_db::ops::prompting::insert_global_template(
                &pool,
                uuid::Uuid::new_v4(),
                kind.name(),
                &plan.body,
            )
            .await?;
        }
        (SeedOutcome::Refreshed, Some(version)) => {
            let row = existing_row.expect("refresh plans only exist over a loaded row");
            pidash_db::ops::prompting::refresh_global_template(&pool, row.id, &plan.body, version)
                .await?;
        }
        _ => {}
    }
    println!("{}", plan.line);
    Ok(())
}

/// `revalidate_section_overrides [--clear]`
/// (`revalidate_section_overrides.py:33-68`): scan the active rows,
/// flag-or-clear `needs_attention` per row, print the `flagged …` lines
/// in scan order, then the summary line.
pub async fn run_revalidate(args: RevalidateArgs) -> Result<(), Box<dyn std::error::Error>> {
    use pidash_services::ops::prompting::{plan_revalidate, ActiveOverride};
    use pidash_services::prompting::seed::RevalidateAction;

    let pool = connect_primary().await?;
    let rows = pidash_db::ops::prompting::fetch_active_overrides(&pool).await?;
    let inputs: Vec<ActiveOverride> = rows
        .iter()
        .map(|row| ActiveOverride {
            id: row.id.to_string(),
            workspace_id: row.workspace_id.to_string(),
            user_id: row.user_id.map(|id| id.to_string()),
            section_key: row.section_key.clone(),
            body: row.body.clone(),
            version: row.version.into(),
            needs_attention: row.needs_attention,
        })
        .collect();
    let plan = plan_revalidate(&inputs, args.clear);
    for step in &plan.steps {
        match step.action {
            RevalidateAction::Flag => {
                let id: uuid::Uuid = step.id.parse()?;
                pidash_db::ops::prompting::set_needs_attention(&pool, id, true).await?;
                println!("{}", step.line.as_deref().unwrap_or_default());
            }
            RevalidateAction::Clear => {
                let id: uuid::Uuid = step.id.parse()?;
                pidash_db::ops::prompting::set_needs_attention(&pool, id, false).await?;
            }
            RevalidateAction::Keep => {}
        }
    }
    println!("{}", plan.summary);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// clap accepts the Django argv shapes for all four commands.
    #[test]
    fn parses_django_argv() {
        use clap::Parser;
        #[derive(Debug, Parser)]
        struct Cli {
            #[command(subcommand)]
            command: crate::ops::OpsCommand,
        }
        let cli =
            Cli::try_parse_from(["pidash-api", "reseed_default_template"]).expect("bare reseed");
        assert!(matches!(
            cli.command,
            crate::ops::OpsCommand::ReseedDefaultTemplate(ReseedArgs { force: false })
        ));
        let cli = Cli::try_parse_from(["pidash-api", "reseed_review_template", "--force"])
            .expect("forced reseed");
        assert!(matches!(
            cli.command,
            crate::ops::OpsCommand::ReseedReviewTemplate(ReseedArgs { force: true })
        ));
        let cli = Cli::try_parse_from(["pidash-api", "reseed_test_template", "--force"])
            .expect("forced reseed");
        assert!(matches!(
            cli.command,
            crate::ops::OpsCommand::ReseedTestTemplate(ReseedArgs { force: true })
        ));
        let cli = Cli::try_parse_from(["pidash-api", "revalidate_section_overrides"])
            .expect("bare revalidate");
        assert!(matches!(
            cli.command,
            crate::ops::OpsCommand::RevalidateSectionOverrides(RevalidateArgs { clear: false })
        ));
        let cli = Cli::try_parse_from(["pidash-api", "revalidate_section_overrides", "--clear"])
            .expect("clear revalidate");
        assert!(matches!(
            cli.command,
            crate::ops::OpsCommand::RevalidateSectionOverrides(RevalidateArgs { clear: true })
        ));
    }
}
