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
    let mut client = crate::ipc::client::Client::connect(paths.ipc_socket_path()).await?;
    let resp = client.call(Request::StatusGet).await?;
    match resp {
        Response::Status(s) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                s.print_compact();
            }
            Ok(())
        }
        Response::Error(e) => {
            eprintln!("daemon error: {}", e.message);
            std::process::exit(1);
        }
        other => {
            eprintln!("unexpected response: {other:?}");
            std::process::exit(1);
        }
    }
}
