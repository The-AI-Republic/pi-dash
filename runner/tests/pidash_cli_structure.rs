//! Contract tests for the top-level CLI surface shape (PR 1: CLI restructure).
//!
//! These tests use `clap::CommandFactory` to inspect the parsed `Cli` tree
//! rather than shelling out to the built binary. We assert:
//!
//! - every verb promised in the design doc is registered at the top level
//! - the `service` subgroup is gone
//! - `__run` is registered but hidden from `--help`
//!
//! See `.ai_design/runner_install_ux/cli-restructure-and-install-flow.md`.

use clap::CommandFactory;
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
        "issue",
        "comment",
        "state",
        "workspace",
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
fn binary_help_omits_internal_run() {
    // Render long help the way clap would print it on `pidash --help`, then
    // scan for `__run`. Hidden subcommands are excluded from both summaries.
    let mut cmd = Cli::command();
    let help = cmd.render_long_help().to_string();
    assert!(
        !help.contains("__run"),
        "--help output must not mention the internal `__run` subcommand:\n{help}",
    );
}
