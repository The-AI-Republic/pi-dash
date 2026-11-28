#![forbid(unsafe_code)]

use anyhow::Result;
use clap::Parser;
use pidash::cli;

fn main() -> Result<()> {
    // Warn before clap parsing so even an unknown-subcommand error (a wrong
    // build lacking the verb) carries the explanation (NEWPIDASH-363).
    pidash::util::shell::warn_if_runner_exe_mismatch();
    let argv: Vec<String> = std::env::args().collect();
    // Inside an agent run, operator commands are refused before clap
    // parsing (NEWPIDASH-320 §6.5): clap would otherwise answer with a
    // usage error for commands with required flags (e.g. `pidash runner
    // add`) instead of the refusal JSON.
    if let Some(error) = cli::early_operator_refusal(&argv) {
        eprintln!("{error}");
        std::process::exit(2);
    }
    // Inside an agent run, a bare top-level help request (`pidash --help`
    // or the equivalent `pidash help`) lists only agent commands.
    // Subcommand help keeps clap's default rendering: an operator command
    // is refused when actually run.
    if cli::wants_filtered_help(&argv) {
        // This help is printed before `cli::run` installs the broken-pipe
        // hook, so install it here too (never reached for `pidash __run`).
        pidash::util::broken_pipe::install();
        print!("{}", cli::agent_help());
        return Ok(());
    }
    let cli = cli::Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(cli::run(cli))
}
