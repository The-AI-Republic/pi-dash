use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufStream};
#[cfg(unix)]
use tokio::net::UnixStream as IpcStream;
#[cfg(windows)]
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient as IpcStream};

use super::protocol::{Request, Response};

pub struct Client {
    stream: BufStream<IpcStream>,
    path: PathBuf,
}

impl Client {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let stream = connect_stream(&path)
            .await
            .with_context(|| format!("connecting to runner IPC at {}", endpoint_display(&path)))?;
        Ok(Self {
            stream: BufStream::new(stream),
            path,
        })
    }

    pub async fn call(&mut self, req: Request) -> Result<Response> {
        let mut line = serde_json::to_vec(&req)?;
        line.push(b'\n');
        self.stream.write_all(&line).await?;
        self.stream.flush().await?;
        let mut buf = String::new();
        let n = self.stream.read_line(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("daemon closed IPC at {:?}", self.path);
        }
        let resp: Response = serde_json::from_str(buf.trim())?;
        Ok(resp)
    }

    /// Read streaming responses (used by subscribe-style requests).
    pub async fn read_next(&mut self) -> Result<Option<Response>> {
        let mut buf = String::new();
        let n = self.stream.read_line(&mut buf).await?;
        if n == 0 {
            return Ok(None);
        }
        let resp: Response = serde_json::from_str(buf.trim())?;
        Ok(Some(resp))
    }
}

/// Convenience that returns a reader half bound to the same stream.
pub fn reader_from(stream: IpcStream) -> BufReader<IpcStream> {
    BufReader::new(stream)
}

#[cfg(unix)]
async fn connect_stream(path: &Path) -> std::io::Result<IpcStream> {
    IpcStream::connect(path).await
}

#[cfg(windows)]
async fn connect_stream(path: &Path) -> std::io::Result<IpcStream> {
    ClientOptions::new().open(crate::ipc::windows_pipe_name(path))
}

#[cfg(unix)]
fn endpoint_display(path: &Path) -> String {
    format!("{path:?}")
}

#[cfg(windows)]
fn endpoint_display(path: &Path) -> String {
    crate::ipc::windows_pipe_name(path)
}
