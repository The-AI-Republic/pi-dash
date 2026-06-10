//! `pidash workdir` — manage shared work directories (worktree pools).
//!
//! A work dir is one canonical git clone on this machine plus a pool of git
//! worktrees that runs execute in. Multiple runners can reference one work dir
//! by name (`pidash runner add --workdir <name>`), letting e.g. a codex runner
//! and a claude_code runner share a single repo checkout. See
//! `.ai_design/worktree_pooling/design.md`.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args as ClapArgs, Subcommand};

use crate::config::file;
use crate::config::schema::{CleanMode, WorkdirConfig, DEFAULT_POOL_SIZE, POOL_SIZE_WARN_ABOVE};
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct WorkdirArgs {
    #[command(subcommand)]
    pub command: WorkdirCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkdirCommand {
    /// Add a work dir. The path must be an existing git clone (its history is
    /// shared by every worktree the pool materializes; agents never run in it
    /// directly).
    Add(AddArgs),
    /// List configured work dirs.
    List(ListArgs),
    /// Remove a work dir. Refused while any runner references it.
    Remove(RemoveArgs),
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Stable name runners reference via `--workdir`.
    #[arg(long)]
    pub name: String,
    /// Path to the canonical git clone.
    #[arg(long)]
    pub path: PathBuf,
    /// Max concurrent leases (worktrees). Defaults to 2.
    #[arg(long)]
    pub pool_size: Option<usize>,
    /// How a worktree is scrubbed when returned to the pool.
    #[arg(long, value_enum)]
    pub clean_mode: Option<CleanMode>,
    /// Globs preserved across cleans in `allowlist` mode (repeatable).
    #[arg(long = "keep-path")]
    pub keep_paths: Vec<String>,
    /// Command run once per new worktree (e.g. `pnpm install`).
    #[arg(long)]
    pub setup_command: Option<String>,
    /// Override where worktrees are materialized.
    #[arg(long)]
    pub worktrees_dir: Option<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct ListArgs {}

#[derive(Debug, ClapArgs)]
pub struct RemoveArgs {
    /// Name of the work dir to remove.
    pub name: String,
}

pub async fn run(args: WorkdirArgs, paths: &Paths) -> Result<()> {
    match args.command {
        WorkdirCommand::Add(a) => add(a, paths),
        WorkdirCommand::List(_) => list(paths),
        WorkdirCommand::Remove(r) => remove(r, paths),
    }
}

fn add(args: AddArgs, paths: &Paths) -> Result<()> {
    // The canonical clone must already be a git repo: the pool shares its
    // object database and never clones from a URL itself.
    if !crate::workspace::git::is_git_repo(&args.path) {
        bail!(
            "path {:?} is not a git repo. Clone the repository there first, \
             then `pidash workdir add` it.",
            args.path
        );
    }

    let pool_size = args.pool_size.unwrap_or(DEFAULT_POOL_SIZE);
    if pool_size < 1 {
        bail!("pool_size must be at least 1");
    }
    if pool_size > POOL_SIZE_WARN_ABOVE {
        eprintln!(
            "warning: pool_size {pool_size} is unusually large (> {POOL_SIZE_WARN_ABOVE}); \
             each desk is a full checkout on disk."
        );
    }

    let workdir = WorkdirConfig {
        name: args.name.clone(),
        path: args.path.clone(),
        pool_size,
        clean_mode: args.clean_mode.unwrap_or_default(),
        keep_paths: args.keep_paths,
        setup_command: args.setup_command,
        worktrees_dir: args.worktrees_dir,
    };

    let name = args.name.clone();
    file::mutate_config(paths, move |cfg| {
        if cfg.workdirs.iter().any(|w| w.name == workdir.name) {
            bail!("a work dir named {:?} already exists", workdir.name);
        }
        cfg.workdirs.push(workdir);
        // Re-validate so path collisions etc. are caught before write.
        cfg.validate()
            .map_err(|e| anyhow::anyhow!("invalid config after add: {e}"))?;
        Ok(())
    })?;

    println!("Added work dir {name:?} (pool_size {pool_size}). Reference it with `pidash runner add --workdir {name}`.");
    Ok(())
}

fn list(paths: &Paths) -> Result<()> {
    let cfg = file::load_config(paths)?;
    if cfg.workdirs.is_empty() {
        println!("No work dirs configured. Add one with `pidash workdir add`.");
        return Ok(());
    }
    for w in &cfg.workdirs {
        let runners: Vec<&str> = cfg
            .runners
            .iter()
            .filter(|r| r.workdir.as_deref() == Some(w.name.as_str()))
            .map(|r| r.name.as_str())
            .collect();
        println!(
            "{}  path={}  pool_size={}  clean_mode={:?}  runners=[{}]",
            w.name,
            w.path.display(),
            w.pool_size,
            w.clean_mode,
            runners.join(", ")
        );
    }
    Ok(())
}

fn remove(args: RemoveArgs, paths: &Paths) -> Result<()> {
    let name = args.name.clone();
    file::mutate_config(paths, move |cfg| {
        let referencing: Vec<String> = cfg
            .runners
            .iter()
            .filter(|r| r.workdir.as_deref() == Some(name.as_str()))
            .map(|r| r.name.clone())
            .collect();
        if !referencing.is_empty() {
            bail!(
                "work dir {:?} is still referenced by runner(s): {}. \
                 Remove or repoint them first.",
                name,
                referencing.join(", ")
            );
        }
        let before = cfg.workdirs.len();
        cfg.workdirs.retain(|w| w.name != name);
        if cfg.workdirs.len() == before {
            bail!("no work dir named {:?}", name);
        }
        Ok(())
    })?;
    println!("Removed work dir {:?}.", args.name);
    Ok(())
}
