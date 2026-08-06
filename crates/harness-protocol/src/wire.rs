use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request<P = Value> {
    pub id: String,
    pub method: String,
    pub params: P,
}

impl<P> Request<P> {
    pub fn new(id: impl Into<String>, method: impl Into<String>, params: P) -> Self {
        Self {
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response<R = Value> {
    Success { id: String, result: R },
    Failure { id: String, error: WireError },
}

impl<R> Response<R> {
    pub fn id(&self) -> &str {
        match self {
            Self::Success { id, .. } | Self::Failure { id, .. } => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    NotFound,
    ProviderUnavailable,
    StaleSnapshot,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Push<D = Value> {
    pub channel: String,
    pub sequence: u64,
    pub data: D,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InboundFrame {
    Response(Response),
    Push(Push),
}

pub mod method {
    pub const CLIENT_CAPABILITIES: &str = "client.capabilities";
    pub const SYSTEM_INFO: &str = "system.info";
    pub const PROVIDERS_LIST: &str = "providers.list";
    pub const CONNECTIONS_LIST: &str = "connections.list";
    pub const CONNECTIONS_MODELS: &str = "connections.models";
    pub const ACP_AGENTS: &str = "acp.agents";
    pub const PROJECTS_LIST: &str = "projects.list";
    pub const PROJECTS_ADD: &str = "projects.add";
    pub const MODELS_LIST: &str = "models.list";
    pub const THREAD_START: &str = "thread.start";
    pub const THREAD_RENAME: &str = "thread.rename";
    pub const THREAD_HISTORY: &str = "thread.history";
    pub const THREAD_SEND_TURN: &str = "thread.sendTurn";
    pub const THREAD_INTERRUPT: &str = "thread.interrupt";
    pub const THREAD_QUEUE: &str = "thread.queue";
    pub const THREAD_STEER_QUEUED_TURN: &str = "thread.steerQueuedTurn";
    pub const SIDEBAR_SETTINGS: &str = "sidebar.settings";
}

pub mod channel {
    pub const SERVER_WELCOME: &str = "server.welcome";
    pub const THREAD_EVENT: &str = "thread.event";
    pub const THREAD_QUEUE: &str = "thread.queue";
    pub const THREAD_LIFECYCLE: &str = "thread.lifecycle";
    pub const SIDEBAR_SETTINGS: &str = "sidebar.settings";
    pub const TERMINAL_OUTPUT: &str = "terminal.output";
    pub const TERMINAL_EXIT: &str = "terminal.exit";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_uses_the_existing_wire_shape() {
        let request = Request::new(
            "request-1",
            method::THREAD_HISTORY,
            json!({ "threadId": "thread-1" }),
        );

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "id": "request-1",
                "method": "thread.history",
                "params": { "threadId": "thread-1" }
            })
        );
    }

    #[test]
    fn inbound_frames_distinguish_responses_from_pushes() {
        let response: InboundFrame = serde_json::from_value(json!({
            "id": "request-1",
            "result": { "protocolVersion": 2 }
        }))
        .unwrap();
        let push: InboundFrame = serde_json::from_value(json!({
            "channel": "server.welcome",
            "sequence": 7,
            "data": { "serverVersion": "0.0.0", "protocolVersion": 2 }
        }))
        .unwrap();

        assert!(matches!(response, InboundFrame::Response(_)));
        assert!(matches!(push, InboundFrame::Push(_)));
    }
}
