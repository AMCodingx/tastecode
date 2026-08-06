use async_channel::Receiver as EventReceiver;
use harness_client::{ClientEvent, ClientHandle, ConnectionState, Endpoint};
use harness_protocol::{
    DomainEvent, PROTOCOL_VERSION, ProjectSummary, ProjectsListResult, Response, SendTurnResult,
    ServerWelcome, SidebarMode, SidebarSettings, ThreadEventPush, ThreadHistoryResult,
    ThreadInboxStatus, ThreadLifecyclePush, ThreadQueuePush, ThreadQueueResult, channel, method,
};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(crate) struct ClientState {
    client: Option<ClientHandle>,
    pub(crate) connection: ConnectionState,
    pub(crate) projects: Vec<ProjectSummary>,
    pub(crate) projects_loaded: bool,
    pub(crate) sidebar_settings: SidebarSettings,
    pending: HashMap<String, PendingRequest>,
    pub(crate) notice: Option<String>,
}

enum PendingRequest {
    Capabilities,
    Projects,
    SidebarSettings,
    History { thread_id: String, replace: bool },
    Queue { thread_id: String },
    SendTurn { thread_id: String, steer: bool },
    Steer { thread_id: String },
    Interrupt { thread_id: String },
}

#[derive(Default)]
pub(crate) struct ClientUpdate {
    pub(crate) shell_changed: bool,
    pub(crate) chat: Vec<ChatUpdate>,
}

pub(crate) enum ChatUpdate {
    History {
        thread_id: String,
        history: ThreadHistoryResult,
        replace: bool,
    },
    Queue {
        thread_id: String,
        queue: ThreadQueueResult,
    },
    Event(ThreadEventPush),
    Refresh,
    Error {
        thread_id: String,
        message: String,
    },
}

impl ClientUpdate {
    fn shell_changed() -> Self {
        Self {
            shell_changed: true,
            chat: Vec::new(),
        }
    }

    fn chat(update: ChatUpdate) -> Self {
        Self {
            shell_changed: false,
            chat: vec![update],
        }
    }
}

impl ClientState {
    pub(crate) fn new(fixture: bool) -> Self {
        Self {
            client: None,
            connection: if fixture {
                ConnectionState::Open
            } else {
                ConnectionState::Connecting
            },
            projects: Vec::new(),
            projects_loaded: fixture,
            sidebar_settings: SidebarSettings {
                mode: SidebarMode::Inbox,
                auto_settle_days: Some(3),
            },
            pending: HashMap::new(),
            notice: None,
        }
    }

    pub(crate) fn connect(&mut self) -> Option<EventReceiver<ClientEvent>> {
        match Endpoint::from_environment().and_then(ClientHandle::start) {
            Ok((client, events)) => {
                self.client = Some(client);
                Some(events)
            }
            Err(error) => {
                self.connection = ConnectionState::Closed;
                self.notice = Some(error.to_string());
                None
            }
        }
    }

    pub(crate) fn handle_event(&mut self, event: ClientEvent) -> ClientUpdate {
        match event {
            ClientEvent::StateChanged(state) => {
                self.connection = state;
                if state == ConnectionState::Open {
                    self.notice = None;
                    self.request_initial_state();
                }
                ClientUpdate::shell_changed()
            }
            ClientEvent::Response(response) => self.handle_response(response),
            ClientEvent::Push(push) => self.handle_push(&push.channel, push.data),
            ClientEvent::SequenceGap { expected, received } => {
                self.notice = Some(format!(
                    "Live state skipped from sequence {expected} to {received}; refreshing."
                ));
                self.request_projects();
                ClientUpdate {
                    shell_changed: true,
                    chat: vec![ChatUpdate::Refresh],
                }
            }
            ClientEvent::DecodeFailed { reason } => {
                self.notice = Some(format!("The server sent an unreadable frame: {reason}"));
                ClientUpdate::shell_changed()
            }
        }
    }

    pub(crate) fn select_thread(&mut self, thread_id: &str) {
        self.request_history(thread_id, None);
        self.send_request(
            method::THREAD_QUEUE,
            json!({ "threadId": thread_id }),
            PendingRequest::Queue {
                thread_id: thread_id.into(),
            },
        );
    }

    pub(crate) fn request_history(&mut self, thread_id: &str, after_seq: Option<u64>) {
        let params = match after_seq {
            Some(after_seq) => json!({ "threadId": thread_id, "afterSeq": after_seq }),
            None => json!({ "threadId": thread_id }),
        };
        self.send_request(
            method::THREAD_HISTORY,
            params,
            PendingRequest::History {
                thread_id: thread_id.into(),
                replace: after_seq.is_none(),
            },
        );
    }

    pub(crate) fn send_turn(&mut self, thread_id: &str, text: String, steer: bool) {
        self.send_request(
            method::THREAD_SEND_TURN,
            json!({ "threadId": thread_id, "text": text }),
            PendingRequest::SendTurn {
                thread_id: thread_id.into(),
                steer,
            },
        );
    }

    pub(crate) fn interrupt(&mut self, thread_id: &str) {
        self.send_request(
            method::THREAD_INTERRUPT,
            json!({ "threadId": thread_id }),
            PendingRequest::Interrupt {
                thread_id: thread_id.into(),
            },
        );
    }

    pub(crate) fn stage_message(&self, fixture: bool) -> String {
        self.notice
            .clone()
            .unwrap_or_else(|| match self.connection {
                ConnectionState::Connecting => "Connecting to the Harness server…".into(),
                ConnectionState::Reconnecting => "Reconnecting to the Harness server…".into(),
                ConnectionState::Closed => "The Harness server is unavailable.".into(),
                ConnectionState::Open if !self.projects_loaded => "Loading projects…".into(),
                ConnectionState::Open if self.projects.is_empty() && !fixture => {
                    "Add a project to start a native session.".into()
                }
                ConnectionState::Open => {
                    "Parallel work stays visible without owning the whole workspace.".into()
                }
            })
    }

    fn request_initial_state(&mut self) {
        self.send_request(
            method::CLIENT_CAPABILITIES,
            json!({ "previewCapture": false }),
            PendingRequest::Capabilities,
        );
        self.request_projects();
        self.send_request(
            method::SIDEBAR_SETTINGS,
            json!({}),
            PendingRequest::SidebarSettings,
        );
    }

    fn request_projects(&mut self) {
        self.send_request(method::PROJECTS_LIST, json!({}), PendingRequest::Projects);
    }

    fn send_request(&mut self, method: &str, params: Value, request: PendingRequest) {
        let Some(client) = &self.client else {
            return;
        };
        match client.request(method, params) {
            Ok(id) => {
                self.pending.insert(id, request);
            }
            Err(error) => {
                self.notice = Some(error.to_string());
            }
        }
    }

    fn handle_response(&mut self, response: Response<Value>) -> ClientUpdate {
        let pending = self.pending.remove(response.id());
        match response {
            Response::Failure { error, .. } => {
                let message = match error.detail {
                    Some(detail) => format!("{} ({detail})", error.message),
                    None => error.message,
                };
                match pending.and_then(PendingRequest::thread_id) {
                    Some(thread_id) => ClientUpdate::chat(ChatUpdate::Error { thread_id, message }),
                    None => {
                        self.notice = Some(message);
                        ClientUpdate::shell_changed()
                    }
                }
            }
            Response::Success { result, .. } => match pending {
                Some(PendingRequest::Projects) => self.handle_projects_response(result),
                Some(PendingRequest::SidebarSettings) => self.handle_settings_response(result),
                Some(PendingRequest::History { thread_id, replace }) => {
                    match serde_json::from_value::<ThreadHistoryResult>(result) {
                        Ok(history) => ClientUpdate::chat(ChatUpdate::History {
                            thread_id,
                            history,
                            replace,
                        }),
                        Err(error) => ClientUpdate::chat(ChatUpdate::Error {
                            thread_id,
                            message: format!("thread.history was invalid: {error}"),
                        }),
                    }
                }
                Some(PendingRequest::Queue { thread_id }) => {
                    match serde_json::from_value::<ThreadQueueResult>(result) {
                        Ok(queue) => ClientUpdate::chat(ChatUpdate::Queue { thread_id, queue }),
                        Err(error) => ClientUpdate::chat(ChatUpdate::Error {
                            thread_id,
                            message: format!("thread.queue was invalid: {error}"),
                        }),
                    }
                }
                Some(PendingRequest::SendTurn { thread_id, steer }) => {
                    match serde_json::from_value::<SendTurnResult>(result) {
                        Ok(SendTurnResult::Started { queued: false, .. }) => {
                            ClientUpdate::default()
                        }
                        Ok(SendTurnResult::Queued {
                            queued: true,
                            queued_turn,
                        }) => {
                            if steer {
                                self.send_request(
                                    method::THREAD_STEER_QUEUED_TURN,
                                    json!({
                                        "threadId": thread_id,
                                        "queuedTurnId": queued_turn.id
                                    }),
                                    PendingRequest::Steer { thread_id },
                                );
                            }
                            ClientUpdate::default()
                        }
                        Ok(_) => ClientUpdate::chat(ChatUpdate::Error {
                            thread_id,
                            message: "thread.sendTurn returned a contradictory queue state.".into(),
                        }),
                        Err(error) => ClientUpdate::chat(ChatUpdate::Error {
                            thread_id,
                            message: format!("thread.sendTurn was invalid: {error}"),
                        }),
                    }
                }
                Some(PendingRequest::Interrupt { .. })
                | Some(PendingRequest::Steer { .. })
                | Some(PendingRequest::Capabilities)
                | None => ClientUpdate::default(),
            },
        }
    }

    fn handle_projects_response(&mut self, result: Value) -> ClientUpdate {
        match serde_json::from_value::<ProjectsListResult>(result) {
            Ok(result) => {
                self.projects = result.projects;
                self.projects_loaded = true;
                self.notice = None;
            }
            Err(error) => {
                self.notice = Some(format!("projects.list was invalid: {error}"));
            }
        }
        ClientUpdate::shell_changed()
    }

    fn handle_settings_response(&mut self, result: Value) -> ClientUpdate {
        match serde_json::from_value::<SidebarSettings>(result) {
            Ok(settings) => self.sidebar_settings = settings,
            Err(error) => {
                self.notice = Some(format!("sidebar.settings was invalid: {error}"));
            }
        }
        ClientUpdate::shell_changed()
    }

    fn handle_push(&mut self, channel_name: &str, data: Value) -> ClientUpdate {
        match channel_name {
            channel::SERVER_WELCOME => self.handle_welcome(data),
            channel::SIDEBAR_SETTINGS => {
                match serde_json::from_value::<SidebarSettings>(data) {
                    Ok(settings) => self.sidebar_settings = settings,
                    Err(error) => {
                        self.notice = Some(format!("sidebar.settings push was invalid: {error}"));
                    }
                }
                ClientUpdate::shell_changed()
            }
            channel::THREAD_LIFECYCLE => {
                match serde_json::from_value::<ThreadLifecyclePush>(data) {
                    Ok(push) => {
                        if let Some(session) = self.session_mut(&push.thread_id) {
                            session.lifecycle = Some(push.lifecycle);
                        } else {
                            self.request_projects();
                        }
                    }
                    Err(error) => {
                        self.notice = Some(format!("thread.lifecycle push was invalid: {error}"));
                    }
                }
                ClientUpdate::shell_changed()
            }
            channel::THREAD_EVENT => self.handle_thread_event(data),
            channel::THREAD_QUEUE => match serde_json::from_value::<ThreadQueuePush>(data) {
                Ok(push) => ClientUpdate::chat(ChatUpdate::Queue {
                    thread_id: push.thread_id,
                    queue: ThreadQueueResult {
                        items: push.items,
                        can_steer: push.can_steer,
                    },
                }),
                Err(error) => {
                    self.notice = Some(format!("thread.queue push was invalid: {error}"));
                    ClientUpdate::shell_changed()
                }
            },
            _ => ClientUpdate::default(),
        }
    }

    fn handle_welcome(&mut self, data: Value) -> ClientUpdate {
        match serde_json::from_value::<ServerWelcome>(data) {
            Ok(welcome) if welcome.protocol_version == PROTOCOL_VERSION => ClientUpdate::default(),
            Ok(welcome) => {
                self.notice = Some(format!(
                    "Protocol mismatch: native client expects v{PROTOCOL_VERSION}, server sent v{}.",
                    welcome.protocol_version
                ));
                ClientUpdate::shell_changed()
            }
            Err(error) => {
                self.notice = Some(format!("server.welcome was invalid: {error}"));
                ClientUpdate::shell_changed()
            }
        }
    }

    fn handle_thread_event(&mut self, data: Value) -> ClientUpdate {
        let Ok(push) = serde_json::from_value::<ThreadEventPush>(data) else {
            self.notice = Some("thread.event push was invalid.".into());
            return ClientUpdate::shell_changed();
        };
        let shell_changed = self.apply_session_status(&push);
        ClientUpdate {
            shell_changed,
            chat: vec![ChatUpdate::Event(push)],
        }
    }

    fn apply_session_status(&mut self, push: &ThreadEventPush) -> bool {
        let (running, status) = match &push.event {
            DomainEvent::TurnStarted { .. } => (Some(true), Some(ThreadInboxStatus::Working)),
            DomainEvent::TurnCompleted { .. } => (Some(false), Some(ThreadInboxStatus::Ready)),
            DomainEvent::ThreadError { .. } => (Some(false), Some(ThreadInboxStatus::Failed)),
            DomainEvent::ApprovalRequested { .. } => (None, Some(ThreadInboxStatus::Approval)),
            DomainEvent::UserInputRequested { .. } => (None, Some(ThreadInboxStatus::Input)),
            DomainEvent::ApprovalResolved { .. } | DomainEvent::UserInputResolved { .. } => {
                let running = self
                    .session(&push.thread_id)
                    .is_some_and(|session| session.running);
                (
                    None,
                    Some(if running {
                        ThreadInboxStatus::Working
                    } else {
                        ThreadInboxStatus::Ready
                    }),
                )
            }
            DomainEvent::ThreadStarted { .. } => {
                self.request_projects();
                return true;
            }
            DomainEvent::ItemStarted { .. }
            | DomainEvent::ItemDelta { .. }
            | DomainEvent::ItemCompleted { .. }
            | DomainEvent::PlanUpdated { .. }
            | DomainEvent::UsageUpdated { .. }
            | DomainEvent::DiffUpdated { .. }
            | DomainEvent::ApprovalReviewStarted { .. }
            | DomainEvent::ApprovalReviewCompleted { .. } => return false,
        };

        if let Some(session) = self.session_mut(&push.thread_id) {
            if let Some(running) = running {
                session.running = running;
            }
            session.status = status;
        } else {
            self.request_projects();
        }
        true
    }

    fn session(&self, thread_id: &str) -> Option<&harness_protocol::SessionSummary> {
        self.projects
            .iter()
            .flat_map(|project| project.sessions.iter())
            .find(|session| session.id == thread_id)
    }

    fn session_mut(&mut self, thread_id: &str) -> Option<&mut harness_protocol::SessionSummary> {
        self.projects
            .iter_mut()
            .flat_map(|project| project.sessions.iter_mut())
            .find(|session| session.id == thread_id)
    }
}

impl PendingRequest {
    fn thread_id(self) -> Option<String> {
        match self {
            Self::History { thread_id, .. }
            | Self::Queue { thread_id }
            | Self::SendTurn { thread_id, .. }
            | Self::Steer { thread_id }
            | Self::Interrupt { thread_id } => Some(thread_id),
            Self::Capabilities | Self::Projects | Self::SidebarSettings => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_protocol::Push;
    use serde_json::json;

    #[test]
    fn streamed_delta_routes_to_chat_without_invalidating_the_shell() {
        let mut state = ClientState::new(true);
        let update = state.handle_event(ClientEvent::Push(Push {
            channel: channel::THREAD_EVENT.into(),
            sequence: 1,
            data: json!({
                "threadId": "thread-1",
                "event": {
                    "type": "item.delta",
                    "turnId": "turn-1",
                    "itemId": "item-1",
                    "textDelta": "hello"
                },
                "seq": 4
            }),
        }));

        assert!(!update.shell_changed);
        assert_eq!(update.chat.len(), 1);
        assert!(matches!(update.chat[0], ChatUpdate::Event(_)));
    }

    #[test]
    fn projects_response_replaces_the_snapshot() {
        let mut state = ClientState::new(true);
        state
            .pending
            .insert("native-1".into(), PendingRequest::Projects);

        let update = state.handle_response(Response::Success {
            id: "native-1".into(),
            result: json!({
                "projects": [{
                    "path": "/workspace",
                    "name": "Harness",
                    "pinned": true,
                    "createdAt": 1,
                    "sessions": []
                }]
            }),
        });
        assert!(update.shell_changed);
        assert!(state.projects_loaded);
        assert_eq!(state.projects[0].name, "Harness");
    }
}
