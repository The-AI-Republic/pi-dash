//! `pidash reconnect [<runner>]` — retry the cloud connection now.
//!
//! While the cloud is unreachable each runner backs off up to 6 hours
//! between attempts (PDASHOSS01-306). This cuts the current wait short and
//! starts the backoff schedule over, for one runner or for all of them.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::ipc::client::Client;
use crate::ipc::protocol::{Request, Response};
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Runner to reconnect. Omit to reconnect every runner.
    pub runner: Option<String>,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let socket = paths.ipc_socket_path();
    let mut client = Client::connect(&socket)
        .await
        .with_context(|| format!("connecting to the daemon at {socket:?}; is it running?"))?;
    let req = Request::Reconnect {
        runner: args.runner.clone(),
    };
    match client.call(req).await? {
        Response::Ack => {
            match &args.runner {
                Some(name) => println!("reconnecting runner {name} now."),
                None => println!("reconnecting all runners now."),
            }
            Ok(())
        }
        Response::Error(err) if err.code == 400 => anyhow::bail!(
            "the running daemon is too old for `pidash reconnect`; `pidash restart` also \
             retries every runner immediately."
        ),
        Response::Error(err) => anyhow::bail!("daemon rejected reconnect: {}", err.message),
        other => anyhow::bail!("unexpected daemon response: {other:?}"),
    }
}
