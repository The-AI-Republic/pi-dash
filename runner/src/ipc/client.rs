use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufStream};
use tokio::net::UnixStream;

use super::protocol::{Request, Response};

pub struct Client {
    stream: BufStream<UnixStream>,
    path: PathBuf,
}

impl Client {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let stream = UnixStream::connect(&path)
            .await
            .with_context(|| format!("connecting to runner IPC at {path:?}"))?;
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
pub fn reader_from(stream: UnixStream) -> BufReader<UnixStream> {
    BufReader::new(stream)
}
