//! `pidash status` — combined service-level + daemon-runtime status.
//!
//! Two data sources in one command:
//!
//! 1. Service state from systemd/launchd (`is-active` / `launchctl list`).
//!    Tells you whether the OS service manager thinks the daemon is up.
//! 2. Runtime state from the daemon's IPC socket (connected, in-flight run,
//!    pending approvals). Only available while the daemon is actually running.
//!
//! If the daemon isn't running, the IPC connection will fail; we print the
//! service-level status and an explanatory note instead of bubbling the error.

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::ipc::probe::{legacy_daemon, status_at, unreachable_detail};
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Print JSON instead of a human summary.
    #[arg(long)]
    pub json: bool,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let service_state = match crate::service::detect().status().await {
        Ok(s) if s.is_empty() => "unknown".to_string(),
        Ok(s) => s,
        Err(e) => format!("error: {e}"),
    };

    let socket = paths.ipc_socket_path();
    // Keep the attempted socket and the underlying error: "not reachable"
    // alone can't tell a stopped daemon from a CLI looking at the wrong
    // socket (PDASHOSS01-311).
    let (daemon, ipc_error, legacy_socket) = match status_at(&socket).await {
        Ok(s) => (Some(s), None, None),
        Err(e) => match legacy_daemon(paths).await {
            Some((legacy, s)) => (Some(s), None, Some(legacy)),
            None => (None, Some(e), None),
        },
    };

    if args.json {
        let payload = serde_json::json!({
            "service": service_state,
            "daemon": daemon,
            "ipc_socket": socket,
            "ipc_error": ipc_error.as_ref().map(|e| format!("{e:#}")),
            "legacy_socket": legacy_socket,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("service: {service_state}");
        match (&daemon, &ipc_error) {
            (Some(s), _) => {
                s.print_compact();
                if let Some(legacy) = &legacy_socket {
                    eprintln!(
                        "note: reached the daemon at legacy socket {} (started by an older \
                         pidash); run `pidash restart` to move it to {}",
                        legacy.display(),
                        socket.display()
                    );
                }
            }
            (None, Some(e)) => {
                println!("daemon: not reachable via IPC (service may be stopped)");
                for line in unreachable_detail(&socket, e, None).lines() {
                    println!("  {line}");
                }
                if service_state == "active" {
                    println!(
                        "  The service is active but no daemon answers at this socket. If it \
                         runs with a custom data dir, pass the same --data-dir / PIDASH_DATA_DIR."
                    );
                }
            }
            (None, None) => unreachable!("an unreachable daemon always carries its error"),
        }
    }
    Ok(())
}
