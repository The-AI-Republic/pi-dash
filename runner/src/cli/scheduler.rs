// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash scheduler …` subcommands — visibility into project schedulers
//! (PDASHOSS01-225) and writes to a project's install of one or to the
//! definition itself (PDASHOSS01-278).
//!
//! Schedulers are workspace-level definitions installed onto projects; a
//! project install (the *binding*) carries the cadence, the per-project
//! `extra_context`, and fires agent runs. Reads: list the schedulers a
//! workspace or project has, read one definition (prompt included), audit
//! its recent runs. Writes, project admins only: edit the install
//! (`update`) and fire it on the scanner's next tick (`run-now`). Workspace
//! admins can also replace a user-created definition's prompt, name or
//! description (`update --prompt-file`). Installing and uninstalling are
//! not writable here.

use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::issue::{build_query_string, resolve_project_ref};

#[derive(Debug, Args)]
pub struct SchedulerArgs {
    #[command(subcommand)]
    pub command: SchedulerCommand,
}

#[derive(Debug, Subcommand)]
pub enum SchedulerCommand {
    /// List schedulers. With `--project`, the schedulers installed on that
    /// project (each with its cadence, next and last occurrence); without,
    /// every scheduler definition in the workspace.
    List {
        /// Project identifier (slug like `ENG`) or project UUID.
        #[arg(long)]
        project: Option<String>,
    },
    /// Read one scheduler's full definition — prompt, enabled flags, and its
    /// install on the project (cadence, outcome mode, pod, last error).
    Get {
        /// Scheduler UUID (from `pidash scheduler list`).
        scheduler_id: String,
        /// Project identifier or UUID. Defaults to the current run's project.
        #[arg(long)]
        project: Option<String>,
    },
    /// List recent agent runs fired by a scheduler on a project, newest
    /// first: status, timing, and the issues each run wrote to.
    Runs {
        /// Scheduler UUID (from `pidash scheduler list`).
        scheduler_id: String,
        /// Maximum number of runs to return (server default 30, max 100).
        #[arg(long)]
        limit: Option<u32>,
        /// Project identifier or UUID. Defaults to the current run's project.
        #[arg(long)]
        project: Option<String>,
    },
    /// Edit a scheduler's install on a project: its extra context, whether
    /// it is enabled, its cadence, outcome mode or pod. Project admins only.
    /// Prints the updated install plus `previous`, the prior value of each
    /// field changed.
    ///
    /// With `--prompt-file`, `--name` or `--description` it edits the
    /// scheduler definition instead, which every project installing it
    /// shares. Workspace admins only; built-in schedulers are rejected.
    /// Prints the definition, `affected_projects` and `previous`. One call
    /// edits either the definition or the install, not both.
    Update(Box<UpdateArgs>),
    /// Fire a scheduler on a project on the scanner's next tick (within about
    /// a minute); its regular schedule resumes afterwards. Project admins
    /// only. Fails with a 409 when the install is disabled or a run from it
    /// is still active.
    RunNow {
        /// Scheduler UUID (from `pidash scheduler list`).
        scheduler_id: String,
        /// Project identifier or UUID. Defaults to the current run's project.
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Debug, Default, Args)]
pub struct UpdateArgs {
    /// Scheduler UUID (from `pidash scheduler list`).
    pub scheduler_id: String,

    /// Project identifier or UUID. Defaults to the current run's project.
    #[arg(long)]
    pub project: Option<String>,

    /// The install's extra context (markdown appended to the scheduler
    /// prompt on this project), inline; `-` reads it from stdin and `""`
    /// clears it. It replaces the whole text.
    #[arg(long, value_name = "TEXT", conflicts_with = "extra_context_file")]
    pub extra_context: Option<String>,

    /// Read the extra context from a file; `-` reads it from stdin. Prefer
    /// this for multi-paragraph text. An empty file is an error.
    #[arg(long = "extra-context-file", value_name = "PATH")]
    pub extra_context_file: Option<PathBuf>,

    /// Enable the install so it fires on its cadence.
    #[arg(long, conflicts_with = "disable")]
    pub enable: bool,

    /// Disable the install; it stops firing until re-enabled.
    #[arg(long)]
    pub disable: bool,

    /// Recurrence rule (RFC 5545, e.g. `FREQ=HOURLY;INTERVAL=3`). `""` makes
    /// it a single shot at `--dtstart`.
    #[arg(long)]
    pub rrule: Option<String>,

    /// Series anchor: an ISO 8601 datetime (e.g. `2026-01-01T09:00:00Z`).
    #[arg(long)]
    pub dtstart: Option<String>,

    /// IANA timezone name stored with the cadence (e.g. `America/New_York`).
    #[arg(long)]
    pub tzid: Option<String>,

    /// Extra one-off firing (ISO 8601 datetime). Repeatable; the values given
    /// replace the whole list.
    #[arg(
        long = "rdate",
        value_name = "DATETIME",
        conflicts_with = "clear_rdates"
    )]
    pub rdates: Vec<String>,

    /// Remove every extra one-off firing.
    #[arg(long)]
    pub clear_rdates: bool,

    /// Firing to skip (ISO 8601 datetime). Repeatable; the values given
    /// replace the whole list.
    #[arg(
        long = "exdate",
        value_name = "DATETIME",
        conflicts_with = "clear_exdates"
    )]
    pub exdates: Vec<String>,

    /// Remove every skipped firing.
    #[arg(long)]
    pub clear_exdates: bool,

    /// What the install's runs do with findings: `create_issue`,
    /// `apply_fix` or `fix_and_review`.
    #[arg(long)]
    pub outcome_mode: Option<String>,

    /// Pod UUID the install's runs execute on.
    #[arg(long, conflicts_with = "clear_pod")]
    pub pod: Option<String>,

    /// Drop the pod override; runs use the project's default pod.
    #[arg(long)]
    pub clear_pod: bool,

    /// Replace the scheduler definition's prompt from stdin: the only
    /// accepted value is `-`. The prompt is never taken inline.
    #[arg(long, value_name = "-", conflicts_with = "prompt_file")]
    pub prompt: Option<String>,

    /// Replace the scheduler definition's prompt with a file's contents;
    /// `-` reads it from stdin. An empty file is an error.
    #[arg(long = "prompt-file", value_name = "PATH")]
    pub prompt_file: Option<PathBuf>,

    /// Rename the scheduler definition.
    #[arg(long)]
    pub name: Option<String>,

    /// Replace the scheduler definition's description; `""` clears it.
    #[arg(long)]
    pub description: Option<String>,

    /// Only write if the install (or, for a definition edit, the scheduler)
    /// is unchanged since you read it: pass its `updated_at` from
    /// `pidash scheduler get`. A mismatch fails with a 412 naming the
    /// current value, instead of overwriting someone else's edit.
    #[arg(long, value_name = "UPDATED_AT")]
    pub if_updated_at: Option<String>,
}

pub async fn run(args: SchedulerArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        SchedulerCommand::List { project } => cmd_list(&client, project.as_deref()).await,
        SchedulerCommand::Get {
            scheduler_id,
            project,
        } => cmd_get(&client, paths, &scheduler_id, project.as_deref()).await,
        SchedulerCommand::Runs {
            scheduler_id,
            limit,
            project,
        } => cmd_runs(&client, paths, &scheduler_id, limit, project.as_deref()).await,
        SchedulerCommand::Update(args) => cmd_update(&client, paths, *args).await,
        SchedulerCommand::RunNow {
            scheduler_id,
            project,
        } => cmd_run_now(&client, paths, &scheduler_id, project.as_deref()).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_list(client: &ApiClient, project: Option<&str>) -> Result<(), CliError> {
    // Bare `scheduler list` is workspace-wide by design: unlike labels or
    // states, scheduler definitions live on the workspace, so the no-flag
    // form should not silently narrow to a default project.
    let path = match project.map(str::trim).filter(|p| !p.is_empty()) {
        Some(project) => format!(
            "workspaces/{}/projects/{}/schedulers/",
            client.env.workspace_slug, project
        ),
        None => format!("workspaces/{}/schedulers/", client.env.workspace_slug),
    };
    print_json(client.get(&path).await?)
}

async fn cmd_get(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    scheduler_id: &str,
    project: Option<&str>,
) -> Result<(), CliError> {
    let project = resolve_project_ref(client, paths, project).await?;
    let path = format!(
        "workspaces/{}/projects/{}/schedulers/{}/",
        client.env.workspace_slug,
        project,
        scheduler_id.trim()
    );
    print_json(client.get(&path).await?)
}

async fn cmd_runs(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    scheduler_id: &str,
    limit: Option<u32>,
    project: Option<&str>,
) -> Result<(), CliError> {
    let project = resolve_project_ref(client, paths, project).await?;
    let query = runs_query(limit);
    let path = format!(
        "workspaces/{}/projects/{}/schedulers/{}/runs/{query}",
        client.env.workspace_slug,
        project,
        scheduler_id.trim()
    );
    print_json(client.get(&path).await?)
}

async fn cmd_update(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    args: UpdateArgs,
) -> Result<(), CliError> {
    // Build the body before any network call so a bad path, an empty stdin
    // or a flagless invocation fails without touching the server.
    let prompt = load_prompt(&args, std::io::stdin())?;
    if let Some(body) = definition_body(&args, prompt)? {
        // The definition is workspace-scoped: no project to resolve.
        let path = format!(
            "workspaces/{}/schedulers/{}/",
            client.env.workspace_slug,
            args.scheduler_id.trim()
        );
        return print_json(client.patch(&path, &body).await?);
    }
    let extra_context = load_extra_context(&args, std::io::stdin())?;
    let body = update_body(&args, extra_context)?;
    let project = resolve_project_ref(client, paths, args.project.as_deref()).await?;
    let path = format!(
        "workspaces/{}/projects/{}/schedulers/{}/binding/",
        client.env.workspace_slug,
        project,
        args.scheduler_id.trim()
    );
    print_json(client.patch(&path, &body).await?)
}

async fn cmd_run_now(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    scheduler_id: &str,
    project: Option<&str>,
) -> Result<(), CliError> {
    let project = resolve_project_ref(client, paths, project).await?;
    let path = format!(
        "workspaces/{}/projects/{}/schedulers/{}/run-now/",
        client.env.workspace_slug,
        project,
        scheduler_id.trim()
    );
    print_json(client.post(&path, &json!({})).await?)
}

/// Resolve the extra-context source. `--extra-context -` and
/// `--extra-context-file -` both read `stdin`; `None` when neither flag was
/// given. An inline `--extra-context ""` is passed through (it clears the
/// text), but an empty file or empty stdin is an error: it almost always
/// means a failed pipe or a wrong path, not an intent to clear.
fn load_extra_context(args: &UpdateArgs, stdin: impl Read) -> Result<Option<String>, CliError> {
    load_text(
        "extra context",
        args.extra_context.as_deref(),
        args.extra_context_file.as_deref(),
        stdin,
    )
}

/// Resolve the definition prompt source: `--prompt-file <path>`, or `-` on
/// either flag for stdin. Unlike the extra context there is no inline form
/// — a prompt is long multi-paragraph text that shell quoting mangles — and
/// no way to clear it.
fn load_prompt(args: &UpdateArgs, stdin: impl Read) -> Result<Option<String>, CliError> {
    if args.prompt.as_deref().is_some_and(|p| p != "-") {
        return Err(CliError::new(
            EXIT_INVALID,
            "--prompt only accepts `-` (read the prompt from stdin); \
             use --prompt-file <path> for a file",
        ));
    }
    load_text(
        "prompt",
        args.prompt.as_deref(),
        args.prompt_file.as_deref(),
        stdin,
    )
}

/// Read a long text field from its inline flag or its file flag; `-` on
/// either reads `stdin`. `what` names the field in errors.
fn load_text(
    what: &str,
    inline: Option<&str>,
    file: Option<&Path>,
    mut stdin: impl Read,
) -> Result<Option<String>, CliError> {
    let from_stdin = inline == Some("-") || file == Some(Path::new("-"));
    let (body, source) = match (inline, file) {
        _ if from_stdin => {
            let mut buf = String::new();
            stdin.read_to_string(&mut buf).map_err(|e| {
                CliError::new(
                    EXIT_UNKNOWN,
                    format!("failed reading {what} from stdin: {e}"),
                )
            })?;
            (buf, "stdin".to_string())
        }
        (Some(body), _) => return Ok(Some(body.to_string())),
        (None, None) => return Ok(None),
        (None, Some(path)) => {
            let body = std::fs::read_to_string(path).map_err(|e| {
                CliError::new(
                    EXIT_INVALID,
                    format!("failed reading {what} file {}: {e}", path.display()),
                )
            })?;
            (body, path.display().to_string())
        }
    };
    if body.trim().is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            format!("{what} from {source} is empty"),
        ));
    }
    Ok(Some(body))
}

/// Map the definition flags onto the definition PATCH body. `None` when no
/// definition flag was given, i.e. the call edits the install. Mixing the
/// two is an error: they are separate routes with separate permissions, and
/// a half-applied edit is worse than two commands.
fn definition_body(
    args: &UpdateArgs,
    prompt: Option<String>,
) -> Result<Option<Map<String, Value>>, CliError> {
    let mut body = Map::new();
    if let Some(text) = prompt {
        body.insert("prompt".into(), json!(text));
    }
    if let Some(name) = &args.name {
        body.insert("name".into(), json!(name.trim()));
    }
    if let Some(description) = &args.description {
        body.insert("description".into(), json!(description));
    }
    if body.is_empty() {
        return Ok(None);
    }
    if has_binding_flags(args) {
        return Err(CliError::new(
            EXIT_INVALID,
            "--prompt[-file], --name and --description edit the scheduler definition and \
             cannot be combined with flags that edit its install on a project; run two commands",
        ));
    }
    if let Some(expected) = &args.if_updated_at {
        body.insert("expected_updated_at".into(), json!(expected.trim()));
    }
    Ok(Some(body))
}

/// Whether any flag that edits the project install was given.
fn has_binding_flags(args: &UpdateArgs) -> bool {
    args.extra_context.is_some()
        || args.extra_context_file.is_some()
        || args.enable
        || args.disable
        || args.rrule.is_some()
        || args.dtstart.is_some()
        || args.tzid.is_some()
        || !args.rdates.is_empty()
        || args.clear_rdates
        || !args.exdates.is_empty()
        || args.clear_exdates
        || args.outcome_mode.is_some()
        || args.pod.is_some()
        || args.clear_pod
}

/// Map `scheduler update` flags onto the binding PATCH body. Only the flags
/// given are sent, so everything else on the install is left untouched.
fn update_body(
    args: &UpdateArgs,
    extra_context: Option<String>,
) -> Result<Map<String, Value>, CliError> {
    let mut body = Map::new();
    if let Some(text) = extra_context {
        body.insert("extra_context".into(), json!(text));
    }
    if args.enable || args.disable {
        body.insert("enabled".into(), json!(args.enable));
    }
    for (key, value) in [
        ("rrule", &args.rrule),
        ("dtstart", &args.dtstart),
        ("tzid", &args.tzid),
        ("outcome_mode", &args.outcome_mode),
    ] {
        if let Some(value) = value {
            body.insert(key.into(), json!(value.trim()));
        }
    }
    for (key, values, clear) in [
        ("rdates", &args.rdates, args.clear_rdates),
        ("exdates", &args.exdates, args.clear_exdates),
    ] {
        if clear || !values.is_empty() {
            body.insert(key.into(), json!(values));
        }
    }
    if args.clear_pod {
        body.insert("pod".into(), Value::Null);
    } else if let Some(pod) = &args.pod {
        body.insert("pod".into(), json!(pod.trim()));
    }
    if body.is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            "nothing to update: pass at least one of --extra-context[-file], --enable, \
             --disable, --rrule, --dtstart, --tzid, --rdate, --exdate, --outcome-mode, --pod \
             (install), or --prompt-file, --name, --description (definition)",
        ));
    }
    // A precondition, not a field: it rides along only when there is
    // something to write.
    if let Some(expected) = &args.if_updated_at {
        body.insert("expected_updated_at".into(), json!(expected.trim()));
    }
    Ok(body)
}

/// `--limit` maps onto the server's standard `per_page` pagination knob.
fn runs_query(limit: Option<u32>) -> String {
    match limit {
        Some(limit) => build_query_string(&[("per_page", limit.to_string())]),
        None => String::new(),
    }
}

fn print_json(value: serde_json::Value) -> Result<(), CliError> {
    println!(
        "{}",
        serde_json::to_string(&value).expect("serialize JSON value")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use serde_json::{Value, json};

    use super::{
        SchedulerCommand, UpdateArgs, definition_body, load_extra_context, load_prompt, runs_query,
        update_body,
    };
    use crate::api_client::EXIT_INVALID;

    #[test]
    fn runs_query_maps_limit_to_per_page() {
        assert_eq!(runs_query(Some(25)), "?per_page=25");
    }

    #[test]
    fn runs_query_empty_without_limit() {
        assert_eq!(runs_query(None), "");
    }

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(subcommand)]
        command: SchedulerCommand,
    }

    fn parse_update(flags: &[&str]) -> UpdateArgs {
        let argv = ["scheduler", "update", "sched-uuid"]
            .iter()
            .chain(flags)
            .copied();
        match Harness::try_parse_from(argv).expect("parses").command {
            SchedulerCommand::Update(args) => *args,
            other => panic!("expected update, got {other:?}"),
        }
    }

    fn body_of(flags: &[&str]) -> Value {
        let args = parse_update(flags);
        let extra = load_extra_context(&args, &b""[..]).expect("extra context");
        Value::Object(update_body(&args, extra).expect("body"))
    }

    fn conflict(flags: &[&str]) {
        let argv = ["scheduler", "update", "sched-uuid"]
            .iter()
            .chain(flags)
            .copied();
        let err = Harness::try_parse_from(argv).expect_err("flags conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn update_sends_only_the_flags_given() {
        assert_eq!(body_of(&["--disable"]), json!({"enabled": false}));
        assert_eq!(body_of(&["--enable"]), json!({"enabled": true}));
        assert_eq!(
            body_of(&["--rrule", "FREQ=HOURLY;INTERVAL=3"]),
            json!({"rrule": "FREQ=HOURLY;INTERVAL=3"})
        );
    }

    #[test]
    fn update_maps_every_field() {
        assert_eq!(
            body_of(&[
                "--project",
                "ENG",
                "--extra-context",
                "Rule D",
                "--enable",
                "--rrule",
                "FREQ=DAILY",
                "--dtstart",
                "2026-01-01T09:00:00Z",
                "--tzid",
                "America/New_York",
                "--rdate",
                "2026-02-01T09:00:00Z",
                "--rdate",
                "2026-03-01T09:00:00Z",
                "--exdate",
                "2026-01-02T09:00:00Z",
                "--outcome-mode",
                "apply_fix",
                "--pod",
                "pod-uuid",
                "--if-updated-at",
                "2026-10-03T12:00:00.123456Z",
            ]),
            json!({
                "extra_context": "Rule D",
                "enabled": true,
                "rrule": "FREQ=DAILY",
                "dtstart": "2026-01-01T09:00:00Z",
                "tzid": "America/New_York",
                "rdates": ["2026-02-01T09:00:00Z", "2026-03-01T09:00:00Z"],
                "exdates": ["2026-01-02T09:00:00Z"],
                "outcome_mode": "apply_fix",
                "pod": "pod-uuid",
                "expected_updated_at": "2026-10-03T12:00:00.123456Z",
            })
        );
    }

    #[test]
    fn update_clear_flags_send_empty_values() {
        assert_eq!(
            body_of(&["--clear-pod", "--clear-rdates", "--clear-exdates"]),
            json!({"pod": null, "rdates": [], "exdates": []})
        );
        // An empty rrule is a value (single shot), not an absent flag.
        assert_eq!(body_of(&["--rrule", ""]), json!({"rrule": ""}));
        assert_eq!(
            body_of(&["--extra-context", ""]),
            json!({"extra_context": ""})
        );
    }

    #[test]
    fn update_without_a_field_is_invalid() {
        for flags in [&[][..], &["--if-updated-at", "2026-10-03T12:00:00Z"][..]] {
            let args = parse_update(flags);
            let err = update_body(&args, None).expect_err("nothing to update");
            assert_eq!(err.exit_code, EXIT_INVALID);
            assert!(err.message.contains("nothing to update"), "{}", err.message);
        }
    }

    #[test]
    fn update_conflicting_flags_rejected() {
        conflict(&["--enable", "--disable"]);
        conflict(&["--extra-context", "x", "--extra-context-file", "f.md"]);
        conflict(&["--pod", "pod-uuid", "--clear-pod"]);
        conflict(&["--rdate", "2026-02-01T09:00:00Z", "--clear-rdates"]);
        conflict(&["--exdate", "2026-02-01T09:00:00Z", "--clear-exdates"]);
    }

    #[test]
    fn extra_context_file_is_read_verbatim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rules.md");
        let text = "## Rules\n\nA \"queued\" run is not `stuck`.\n\n- $HOME stays literal\n";
        std::fs::write(&path, text).expect("write");
        let args = parse_update(&["--extra-context-file", path.to_str().expect("utf-8")]);
        let extra = load_extra_context(&args, &b""[..]).expect("extra context");
        assert_eq!(extra.as_deref(), Some(text));
        assert_eq!(
            Value::Object(update_body(&args, extra).expect("body")),
            json!({"extra_context": text})
        );
    }

    #[test]
    fn extra_context_dash_reads_stdin() {
        for flags in [
            &["--extra-context", "-"][..],
            &["--extra-context-file", "-"][..],
        ] {
            let args = parse_update(flags);
            let extra = load_extra_context(&args, &b"From stdin\n"[..]).expect("stdin");
            assert_eq!(extra.as_deref(), Some("From stdin\n"));
        }
    }

    #[test]
    fn extra_context_empty_source_is_invalid() {
        let args = parse_update(&["--extra-context", "-"]);
        let err = load_extra_context(&args, &b"  \n"[..]).expect_err("empty stdin");
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("stdin is empty"), "{}", err.message);

        let args = parse_update(&["--extra-context-file", "/nonexistent/rules.md"]);
        let err = load_extra_context(&args, &b""[..]).expect_err("missing file");
        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn extra_context_absent_when_not_given() {
        let args = parse_update(&["--disable"]);
        assert_eq!(
            load_extra_context(&args, &b"ignored"[..]).expect("ok"),
            None
        );
    }

    fn definition_body_of(flags: &[&str], stdin: &[u8]) -> Option<Value> {
        let args = parse_update(flags);
        let prompt = load_prompt(&args, stdin).expect("prompt");
        definition_body(&args, prompt)
            .expect("body")
            .map(Value::Object)
    }

    #[test]
    fn prompt_file_is_read_verbatim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prompt.md");
        let text = "## Coordinator\n\nA \"queued\" run is not `stuck`.\n\n- $HOME stays literal\n";
        std::fs::write(&path, text).expect("write");
        assert_eq!(
            definition_body_of(&["--prompt-file", path.to_str().expect("utf-8")], b""),
            Some(json!({"prompt": text}))
        );
    }

    #[test]
    fn prompt_dash_reads_stdin() {
        for flags in [&["--prompt", "-"][..], &["--prompt-file", "-"][..]] {
            assert_eq!(
                definition_body_of(flags, b"From stdin\n"),
                Some(json!({"prompt": "From stdin\n"}))
            );
        }
    }

    #[test]
    fn prompt_is_never_inline() {
        let args = parse_update(&["--prompt", "Do the thing"]);
        let err = load_prompt(&args, &b""[..]).expect_err("inline prompt");
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("--prompt-file"), "{}", err.message);
        conflict(&["--prompt", "-", "--prompt-file", "p.md"]);
    }

    #[test]
    fn prompt_empty_source_is_invalid() {
        let args = parse_update(&["--prompt", "-"]);
        let err = load_prompt(&args, &b" \n"[..]).expect_err("empty stdin");
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(
            err.message.contains("prompt from stdin is empty"),
            "{}",
            err.message
        );

        let args = parse_update(&["--prompt-file", "/nonexistent/prompt.md"]);
        let err = load_prompt(&args, &b""[..]).expect_err("missing file");
        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn definition_maps_every_field() {
        assert_eq!(
            definition_body_of(
                &[
                    "--prompt",
                    "-",
                    "--name",
                    " Coordinator ",
                    "--description",
                    "",
                    "--if-updated-at",
                    "2026-10-03T12:00:00.123456Z",
                ],
                b"New prompt",
            ),
            Some(json!({
                "prompt": "New prompt",
                "name": "Coordinator",
                "description": "",
                "expected_updated_at": "2026-10-03T12:00:00.123456Z",
            }))
        );
    }

    #[test]
    fn definition_body_absent_for_an_install_edit() {
        assert_eq!(definition_body_of(&["--disable"], b"ignored"), None);
        assert_eq!(definition_body_of(&[], b""), None);
    }

    #[test]
    fn definition_and_install_flags_do_not_mix() {
        for flags in [
            &["--name", "x", "--disable"][..],
            &["--prompt", "-", "--extra-context", "y"][..],
            &["--description", "d", "--clear-pod"][..],
            &["--name", "x", "--rrule", ""][..],
        ] {
            let args = parse_update(flags);
            let prompt = load_prompt(&args, &b"p"[..]).expect("prompt");
            let err = definition_body(&args, prompt).expect_err("mixed flags");
            assert_eq!(err.exit_code, EXIT_INVALID);
            assert!(err.message.contains("two commands"), "{}", err.message);
        }
    }

    #[test]
    fn run_now_parses() {
        let parsed =
            Harness::try_parse_from(["scheduler", "run-now", "sched-uuid", "--project", "ENG"])
                .expect("parses");
        match parsed.command {
            SchedulerCommand::RunNow {
                scheduler_id,
                project,
            } => {
                assert_eq!(scheduler_id, "sched-uuid");
                assert_eq!(project.as_deref(), Some("ENG"));
            }
            other => panic!("expected run-now, got {other:?}"),
        }
    }
}
