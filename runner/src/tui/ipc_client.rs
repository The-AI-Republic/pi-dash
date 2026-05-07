use anyhow::Result;
use std::path::PathBuf;

use crate::ipc::client::Client;
use crate::ipc::protocol::{Request, Response, StatusSnapshot};

#[derive(Clone)]
pub struct TuiIpc {
    pub socket: PathBuf,
    /// When set, scope per-runner read requests (`runs`, `approvals`,
    /// `decide`) to the named runner. `None` means "let the daemon
    /// decide" — works on single-runner installs and falls through to
    /// the union for multi-runner read endpoints.
    pub selected_runner: Option<String>,
}

impl TuiIpc {
    pub async fn status(&self) -> Result<StatusSnapshot> {
        let mut c = Client::connect(&self.socket).await?;
        match c.call(Request::StatusGet).await? {
            Response::Status(s) => Ok(s),
            other => anyhow::bail!("unexpected: {other:?}"),
        }
    }

    pub async fn runs(&self) -> Result<Vec<crate::history::index::RunSummary>> {
        let mut c = Client::connect(&self.socket).await?;
        match c
            .call(Request::RunsList {
                limit: Some(100),
                runner: self.selected_runner.clone(),
            })
            .await?
        {
            Response::Runs(r) => Ok(r),
            other => anyhow::bail!("unexpected: {other:?}"),
        }
    }

    pub async fn approvals(&self) -> Result<Vec<crate::approval::router::ApprovalRecord>> {
        let mut c = Client::connect(&self.socket).await?;
        match c
            .call(Request::ApprovalsList {
                runner: self.selected_runner.clone(),
            })
            .await?
        {
            Response::Approvals(v) => Ok(v),
            other => anyhow::bail!("unexpected: {other:?}"),
        }
    }

    pub async fn decide(
        &self,
        approval_id: &str,
        decision: crate::cloud::protocol::ApprovalDecision,
    ) -> Result<()> {
        self.decide_for_runner(approval_id, decision, self.selected_runner.clone())
            .await
    }

    /// Same as `decide` but with an explicit runner override. Used by
    /// the approvals tab so the decision routes to the runner that
    /// owned the approval at selection time, regardless of any picker
    /// change in between.
    pub async fn decide_for_runner(
        &self,
        approval_id: &str,
        decision: crate::cloud::protocol::ApprovalDecision,
        runner: Option<String>,
    ) -> Result<()> {
        let mut c = Client::connect(&self.socket).await?;
        match c
            .call(Request::ApprovalsDecide {
                approval_id: approval_id.to_string(),
                decision,
                runner,
            })
            .await?
        {
            Response::Ack => Ok(()),
            other => anyhow::bail!("unexpected: {other:?}"),
        }
    }
}
