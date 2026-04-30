use serde::{Deserialize, Serialize};

/// Subset of Codex app-server notification kinds the runner cares about.
/// Unknown kinds are treated as opaque blobs and forwarded to local history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationKind {
    ItemStarted,
    ItemCompleted,
    AgentMessageDelta,
    ReasoningTextDelta,
    CommandExecutionOutputDelta,
    CommandExecutionRequestApproval,
    FileChangeOutputDelta,
    FileChangeRequestApproval,
    TurnDiffUpdated,
    TurnPlanUpdated,
    TurnCompleted,
    ThreadTokenUsageUpdated,
    AccountReauthRequired,
    Other(String),
}

impl NotificationKind {
    pub fn from_method(method: &str) -> Self {
        match method {
            "item/started" => Self::ItemStarted,
            "item/completed" => Self::ItemCompleted,
            "item/agentMessage/delta" => Self::AgentMessageDelta,
            "item/reasoning/textDelta" => Self::ReasoningTextDelta,
            "item/commandExecution/outputDelta" => Self::CommandExecutionOutputDelta,
            "item/commandExecution/requestApproval" => Self::CommandExecutionRequestApproval,
            "item/fileChange/outputDelta" => Self::FileChangeOutputDelta,
            "item/fileChange/requestApproval" => Self::FileChangeRequestApproval,
            "turn/diff/updated" => Self::TurnDiffUpdated,
            "turn/plan/updated" => Self::TurnPlanUpdated,
            "turn/completed" => Self::TurnCompleted,
            "thread/tokenUsage/updated" => Self::ThreadTokenUsageUpdated,
            "account/reauthRequired" => Self::AccountReauthRequired,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn is_approval_request(&self) -> bool {
        matches!(
            self,
            Self::CommandExecutionRequestApproval | Self::FileChangeRequestApproval
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    pub cwd: String,
    pub model: Option<String>,
    pub sandbox_policy: String,
    pub approval_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub thread_id: String,
    pub input: Vec<TurnInputItem>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalResponseParams {
    pub approval_id: String,
    pub decision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResumeParams {
    pub thread_id: String,
}
