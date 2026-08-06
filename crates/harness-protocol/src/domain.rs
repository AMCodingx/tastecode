use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "acp")]
    Acp,
    #[serde(rename = "api")]
    Api,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    Message,
    Reasoning,
    Command,
    FileChange,
    ToolCall,
    Plan,
    Error,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Started,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub turn_id: String,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub status: ItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<MessageRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines_added: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines_removed: Option<i64>,
    pub created_at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    pub thread_id: String,
    pub status: TurnStatus,
    pub created_at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub provider: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub workspace_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub created_at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub text: String,
    pub status: PlanStepStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    Running,
    Done,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: f64,
    pub cached_input_tokens: f64,
    pub output_tokens: f64,
    pub reasoning_tokens: f64,
    pub total_tokens: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub kind: ApprovalKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub created_at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    Command,
    FileChange,
    Permissions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalReview {
    pub id: String,
    pub turn_id: String,
    pub status: ApprovalReviewStatus,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<RiskLevel>,
    pub started_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalReviewStatus {
    InProgress,
    Approved,
    Denied,
    TimedOut,
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputRequest {
    pub id: String,
    pub turn_id: String,
    pub questions: Vec<UserInputQuestion>,
    pub auto_resolution_ms: Option<u64>,
    pub created_at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub allow_other: bool,
    pub secret: bool,
    pub options: Option<Vec<UserInputOption>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputOption {
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DomainEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted { thread: Thread },
    #[serde(rename = "turn.started")]
    TurnStarted { turn: Turn },
    #[serde(rename = "item.started")]
    ItemStarted { item: Item },
    #[serde(rename = "item.delta", rename_all = "camelCase")]
    ItemDelta {
        turn_id: String,
        item_id: String,
        text_delta: String,
    },
    #[serde(rename = "item.completed")]
    ItemCompleted { item: Item },
    #[serde(rename = "turn.completed", rename_all = "camelCase")]
    TurnCompleted { turn_id: String, status: TurnStatus },
    #[serde(rename = "thread.error", rename_all = "camelCase")]
    ThreadError { thread_id: String, message: String },
    #[serde(rename = "plan.updated", rename_all = "camelCase")]
    PlanUpdated {
        turn_id: String,
        steps: Vec<PlanStep>,
    },
    #[serde(rename = "usage.updated")]
    UsageUpdated { usage: Usage },
    #[serde(rename = "diff.updated", rename_all = "camelCase")]
    DiffUpdated { turn_id: String, diff: String },
    #[serde(rename = "approval.requested")]
    ApprovalRequested { request: ApprovalRequest },
    #[serde(rename = "approval.resolved")]
    ApprovalResolved { id: String },
    #[serde(rename = "user_input.requested")]
    UserInputRequested { request: UserInputRequest },
    #[serde(rename = "user_input.resolved")]
    UserInputResolved { id: String },
    #[serde(rename = "approval.review.started")]
    ApprovalReviewStarted { review: ApprovalReview },
    #[serde(rename = "approval.review.completed")]
    ApprovalReviewCompleted { review: ApprovalReview },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalMode {
    #[serde(rename = "ask")]
    Ask,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "auto-review")]
    AutoReview,
    #[serde(rename = "full")]
    Full,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub is_default: bool,
    pub reasoning_efforts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<String>,
    #[serde(default)]
    pub service_tiers: Vec<ServiceTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_service_tier: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceTier {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub steer: bool,
    pub fork: bool,
    pub interrupt: bool,
    pub reasoning_items: bool,
    pub approvals: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_input: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_review: Option<bool>,
    pub images: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuth {
    Authenticated,
    Unauthenticated,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLogin {
    App,
    Provider,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetup {
    pub install_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
    pub login: ProviderLogin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub id: ProviderId,
    pub display_name: String,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub auth: ProviderAuth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<ProviderSetup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersListResult {
    pub providers: Vec<ProviderStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelTransport {
    OpenaiResponses,
    AnthropicMessages,
    OpenaiCompatible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelConnectionPreset {
    Openai,
    Anthropic,
    Openrouter,
    Kimi,
    Zai,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTransportCapabilities {
    pub streaming: bool,
    pub tools: bool,
    pub images: bool,
    pub reasoning: bool,
    pub model_discovery: bool,
    pub usage: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnection {
    pub id: String,
    pub display_name: String,
    pub preset: ModelConnectionPreset,
    pub transport: ModelTransport,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub enabled: bool,
    pub credential_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelTransportCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnectionsResult {
    pub connections: Vec<ModelConnection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgent {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<String>,
    pub setup: ProviderSetup,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgentsResult {
    pub agents: Vec<AcpAgent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsListResult {
    pub models: Vec<Model>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadInboxStatus {
    Starting,
    Working,
    Queued,
    Approval,
    Input,
    Failed,
    Ready,
    Idle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ThreadLifecycle {
    #[serde(rename_all = "camelCase")]
    Active {
        keep_active: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        woke_at: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Settled {
        settled_at: u64,
        reason: SettleReason,
    },
    #[serde(rename_all = "camelCase")]
    Snoozed { snoozed_at: u64, wake_at: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettleReason {
    Manual,
    Inactivity,
    ChangeRequest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub path: String,
    pub name: String,
    pub pinned: bool,
    pub created_at: f64,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub provider: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub created_at: f64,
    pub running: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ThreadInboxStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<ThreadLifecycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsListResult {
    pub projects: Vec<ProjectSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAddedResult {
    pub path: String,
    pub name: String,
    pub pinned: bool,
    pub created_at: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartResult {
    pub thread_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarMode {
    Classic,
    Inbox,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarSettings {
    pub mode: SidebarMode,
    pub auto_settle_days: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerWelcome {
    pub server_version: String,
    pub protocol_version: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEventPush {
    pub thread_id: String,
    pub event: DomainEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLifecyclePush {
    pub thread_id: String,
    pub lifecycle: ThreadLifecycle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencedDomainEvent {
    pub seq: u64,
    pub event: DomainEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistoryResult {
    pub events: Vec<SequencedDomainEvent>,
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedTurn {
    pub id: String,
    pub text: String,
    pub attachments: Vec<String>,
    pub created_at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadQueueResult {
    pub items: Vec<QueuedTurn>,
    pub can_steer: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadQueuePush {
    pub thread_id: String,
    pub items: Vec<QueuedTurn>,
    pub can_steer: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SendTurnResult {
    Started {
        queued: bool,
        #[serde(rename = "turnId")]
        turn_id: String,
    },
    Queued {
        queued: bool,
        #[serde(rename = "queuedTurn")]
        queued_turn: QueuedTurn,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn domain_delta_matches_the_typescript_discriminant() {
        let event = DomainEvent::ItemDelta {
            turn_id: "turn-1".into(),
            item_id: "item-1".into(),
            text_delta: "hello".into(),
        };

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "type": "item.delta",
                "turnId": "turn-1",
                "itemId": "item-1",
                "textDelta": "hello"
            })
        );
    }

    #[test]
    fn provider_and_approval_hyphenation_is_stable() {
        assert_eq!(
            serde_json::to_value(ProviderId::ClaudeCode).unwrap(),
            "claude-code"
        );
        assert_eq!(
            serde_json::to_value(ApprovalMode::AutoReview).unwrap(),
            "auto-review"
        );
    }

    #[test]
    fn provider_catalog_matches_the_typescript_wire_shape() {
        let providers: ProvidersListResult = serde_json::from_value(json!({
            "providers": [{
                "id": "codex",
                "displayName": "Codex",
                "installed": true,
                "version": "1.2.3",
                "auth": "authenticated"
            }]
        }))
        .unwrap();
        let connections: ModelConnectionsResult = serde_json::from_value(json!({
            "connections": [{
                "id": "local",
                "displayName": "Local",
                "preset": "custom",
                "transport": "openai-compatible",
                "baseUrl": "http://127.0.0.1:8080/v1",
                "defaultModel": "local-model",
                "enabled": true,
                "credentialConfigured": true
            }]
        }))
        .unwrap();

        assert_eq!(providers.providers[0].id, ProviderId::Codex);
        assert_eq!(
            connections.connections[0].transport,
            ModelTransport::OpenaiCompatible
        );
        assert_eq!(
            connections.connections[0].default_model.as_deref(),
            Some("local-model")
        );
    }
}
