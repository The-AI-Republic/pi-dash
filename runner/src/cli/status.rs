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

use crate::ipc::protocol::{Request, Response};
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

    let daemon = match crate::ipc::client::Client::connect(paths.ipc_socket_path()).await {
        Ok(mut c) => match c.call(Request::StatusGet).await {
            Ok(Response::Status(s)) => Some(s),
            // A running daemon that explicitly reports an error is very
            // different from "socket unreachable" — surface it distinctly so
            // operators don't chase a phantom "service stopped" diagnosis.
            Ok(Response::Error(e)) => {
                eprintln!("daemon reported error: {}", e.message);
                None
            }
            Ok(other) => {
                eprintln!("unexpected IPC response: {other:?}");
                None
            }
            Err(e) => {
                eprintln!("IPC call failed: {e}");
                None
            }
        },
        Err(_) => None,
    };

    if args.json {
        let payload = serde_json::json!({
            "service": service_state,
            "daemon": daemon,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("service: {service_state}");
        match daemon {
            Some(s) => s.print_compact(),
            None => println!("daemon: not reachable via IPC (service may be stopped)"),
        }
    }
    Ok(())
}
