//! Contract tests for the top-level CLI surface shape (PR 1: CLI restructure).
//!
//! These tests use `clap::CommandFactory` to inspect the parsed `Cli` tree
//! rather than shelling out to the built binary. We assert:
//!
//! - every verb promised in the design doc is registered at the top level
//! - the `service` subgroup is gone
//! - compatibility/internal commands are registered but hidden from `--help`
//!
//! See `.ai_design/runner_install_ux/cli-restructure-and-install-flow.md`.

use clap::{CommandFactory, Parser};
use pidash::cli::Cli;

fn subcommand_names() -> Vec<String> {
    Cli::command()
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect()
}

#[test]
fn top_level_service_verbs_are_registered() {
    let names = subcommand_names();
    for v in ["install", "uninstall", "start", "stop", "restart", "status"] {
        assert!(
            names.contains(&v.to_string()),
            "missing top-level subcommand: {v} (present: {names:?})",
        );
    }
}

#[test]
fn non_service_commands_still_present() {
    let names = subcommand_names();
    for v in [
        "connect",
        "runner",
        "tui",
        "doctor",
        "remove",
        "ai",
        "issue",
        "comment",
        "page",
        "state",
        "workpad",
        "run",
        "workspace",
        "update",
    ] {
        assert!(
            names.contains(&v.to_string()),
            "missing top-level subcommand: {v} (present: {names:?})",
        );
    }
}

#[test]
fn service_subgroup_is_removed() {
    let names = subcommand_names();
    assert!(
        !names.contains(&"service".to_string()),
        "`service` subcommand group should be gone (present: {names:?})",
    );
}

#[test]
fn internal_run_command_exists_but_is_hidden() {
    let cmd = Cli::command();
    let sub = cmd
        .find_subcommand("__run")
        .expect("`__run` subcommand should be registered so systemd/launchd can exec it");
    assert!(
        sub.is_hide_set(),
        "`__run` must be hidden from --help: it is internal plumbing, not a user-facing verb",
    );
}

#[test]
fn legacy_connect_command_exists_but_is_hidden() {
    let cmd = Cli::command();
    let sub = cmd
        .find_subcommand("connect")
        .expect("`connect` remains registered for compatibility token redemption");
    assert!(
        sub.is_hide_set(),
        "`connect` must be hidden from --help; new setup uses `pidash auth login` and `pidash runner add`",
    );
}

#[test]
fn workpad_update_has_keep_flag() {
    // `workpad update` deletes the --body-file on a successful upload; --keep
    // opts out. Assert the flag is registered so callers can rely on it.
    let cmd = Cli::command();
    let workpad = cmd
        .find_subcommand("workpad")
        .expect("`workpad` subcommand should be registered");
    let update = workpad
        .find_subcommand("update")
        .expect("`workpad update` subcommand should be registered");
    let keep = update
        .get_arguments()
        .find(|a| a.get_id() == "keep")
        .expect("`workpad update` must expose a `--keep` flag");
    assert_eq!(
        keep.get_long(),
        Some("keep"),
        "the keep argument must be spelled `--keep`",
    );
}

#[test]
fn binary_help_omits_hidden_commands() {
    // Render long help the way clap would print it on `pidash --help`, then
    // scan for hidden commands. Hidden subcommands are excluded from both
    // summaries.
    let mut cmd = Cli::command();
    let help = cmd.render_long_help().to_string();
    assert!(
        !help.contains("__run"),
        "--help output must not mention the internal `__run` subcommand:\n{help}",
    );
    assert!(
        !help.contains("\n  connect") && !help.contains("\n    connect"),
        "--help output must not mention the deprecated `connect` subcommand:\n{help}",
    );
}

fn arg_ids(cmd: &clap::Command) -> Vec<String> {
    let mut ids: Vec<String> = cmd
        .get_arguments()
        .filter(|a| !a.is_global_set())
        .map(|a| a.get_id().to_string())
        .collect();
    ids.sort();
    ids
}

#[test]
fn top_level_login_is_listed_in_help() {
    let mut cmd = Cli::command();
    let login = cmd
        .find_subcommand("login")
        .expect("`login` should be registered at the top level");
    assert!(!login.is_hide_set(), "`login` must be visible in --help");
    let help = cmd.render_help().to_string();
    assert!(
        help.contains("\n  login"),
        "--help should list `login`:\n{help}"
    );
    assert!(
        help.contains("\n  auth"),
        "--help should still list `auth`:\n{help}"
    );
}

#[test]
fn top_level_login_accepts_exactly_the_auth_login_flags() {
    // `pidash login` is an alias of `pidash auth login`; the two must share
    // one arg struct so hidden flags (`--workspace`, `--device-code`) can't
    // drift apart.
    let cmd = Cli::command();
    let top = cmd.find_subcommand("login").expect("top-level login");
    let nested = cmd
        .find_subcommand("auth")
        .and_then(|a| a.find_subcommand("login"))
        .expect("`auth login` should still be registered");
    let ids = arg_ids(top);
    assert_eq!(ids, arg_ids(nested));
    for id in ["url", "no_browser", "workspace", "device_code"] {
        assert!(ids.contains(&id.to_string()), "missing {id}: {ids:?}");
    }
}

#[test]
fn top_level_login_parses_every_flag() {
    use pidash::cli::Command;
    let cli = Cli::try_parse_from([
        "pidash",
        "login",
        "--url",
        "https://pidash.example.com",
        "--no-browser",
        "--workspace",
        "acme",
        "--device-code",
        "DEV-123",
    ])
    .expect("`pidash login` should accept every `auth login` flag");
    let Some(Command::Login(args)) = cli.command else {
        panic!("expected Command::Login, got {:?}", cli.command);
    };
    assert_eq!(args.url.as_deref(), Some("https://pidash.example.com"));
    assert!(args.no_browser);
    assert_eq!(args.workspace.as_deref(), Some("acme"));
    assert_eq!(args.device_code.as_deref(), Some("DEV-123"));
}

#[test]
fn nested_auth_login_still_parses_desktop_invocation() {
    // Mirrors desktop/src-tauri/src/pidash_cli.rs: `auth login --device-code`.
    use pidash::cli::Command;
    use pidash::cli::auth::AuthCommand;
    let cli = Cli::try_parse_from(["pidash", "auth", "login", "--device-code", "DEV-123"])
        .expect("`pidash auth login --device-code` must keep parsing");
    let Some(Command::Auth(auth)) = cli.command else {
        panic!("expected Command::Auth, got {:?}", cli.command);
    };
    let AuthCommand::Login(args) = auth.command else {
        panic!("expected AuthCommand::Login");
    };
    assert_eq!(args.device_code.as_deref(), Some("DEV-123"));
}
