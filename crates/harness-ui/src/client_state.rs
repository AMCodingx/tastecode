use async_channel::Receiver as EventReceiver;
use harness_client::{ClientEvent, ClientHandle, ConnectionState, Endpoint};
use harness_protocol::{
    DomainEvent, PROTOCOL_VERSION, ProjectSummary, ProjectsListResult, Response, ServerWelcome,
    SidebarMode, SidebarSettings, ThreadEventPush, ThreadInboxStatus, ThreadLifecyclePush, channel,
    method,
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

#[derive(Clone, Copy)]
enum PendingRequest {
    Capabilities,
    Projects,
    SidebarSettings,
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

    /// Returns whether the event changed visible state and needs a GPUI frame.
    pub(crate) fn handle_event(&mut self, event: ClientEvent) -> bool {
        match event {
            ClientEvent::StateChanged(state) => {
                self.connection = state;
                if state == ConnectionState::Open {
                    self.notice = None;
                    self.request_initial_state();
                }
                true
            }
            ClientEvent::Response(response) => self.handle_response(response),
            ClientEvent::Push(push) => self.handle_push(&push.channel, push.data),
            ClientEvent::SequenceGap { expected, received } => {
                self.notice = Some(format!(
                    "Live state skipped from sequence {expected} to {received}; refreshing."
                ));
                self.request_projects();
                true
            }
            ClientEvent::DecodeFailed { reason } => {
                self.notice = Some(format!("The server sent an unreadable frame: {reason}"));
                true
            }
        }
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

    fn handle_response(&mut self, response: Response<Value>) -> bool {
        let pending = self.pending.remove(response.id());
        match response {
            Response::Failure { error, .. } => {
                self.notice = Some(match error.detail {
                    Some(detail) => format!("{} ({detail})", error.message),
                    None => error.message,
                });
                true
            }
            Response::Success { result, .. } => match pending {
                Some(PendingRequest::Projects) => {
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
                    true
                }
                Some(PendingRequest::SidebarSettings) => {
                    match serde_json::from_value::<SidebarSettings>(result) {
                        Ok(settings) => self.sidebar_settings = settings,
                        Err(error) => {
                            self.notice = Some(format!("sidebar.settings was invalid: {error}"));
                        }
                    }
                    true
                }
                Some(PendingRequest::Capabilities) | None => false,
            },
        }
    }

    fn handle_push(&mut self, channel_name: &str, data: Value) -> bool {
        match channel_name {
            channel::SERVER_WELCOME => {
                match serde_json::from_value::<ServerWelcome>(data) {
                    Ok(welcome) if welcome.protocol_version == PROTOCOL_VERSION => return false,
                    Ok(welcome) => {
                        self.notice = Some(format!(
                            "Protocol mismatch: native client expects v{PROTOCOL_VERSION}, server sent v{}.",
                            welcome.protocol_version
                        ));
                    }
                    Err(error) => {
                        self.notice = Some(format!("server.welcome was invalid: {error}"));
                    }
                }
                true
            }
            channel::SIDEBAR_SETTINGS => {
                match serde_json::from_value::<SidebarSettings>(data) {
                    Ok(settings) => self.sidebar_settings = settings,
                    Err(error) => {
                        self.notice = Some(format!("sidebar.settings push was invalid: {error}"));
                    }
                }
                true
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
                true
            }
            channel::THREAD_EVENT => self.handle_thread_event(data),
            _ => false,
        }
    }

    fn handle_thread_event(&mut self, data: Value) -> bool {
        let Ok(push) = serde_json::from_value::<ThreadEventPush>(data) else {
            self.notice = Some("thread.event push was invalid.".into());
            return true;
        };

        match push.event {
            DomainEvent::TurnStarted { .. } => {
                if let Some(session) = self.session_mut(&push.thread_id) {
                    session.running = true;
                    session.status = Some(ThreadInboxStatus::Working);
                } else {
                    self.request_projects();
                }
                true
            }
            DomainEvent::TurnCompleted { .. } => {
                if let Some(session) = self.session_mut(&push.thread_id) {
                    session.running = false;
                    session.status = Some(ThreadInboxStatus::Ready);
                }
                self.request_projects();
                true
            }
            DomainEvent::ThreadError { .. } => {
                if let Some(session) = self.session_mut(&push.thread_id) {
                    session.running = false;
                    session.status = Some(ThreadInboxStatus::Failed);
                }
                true
            }
            DomainEvent::ApprovalRequested { .. } => {
                if let Some(session) = self.session_mut(&push.thread_id) {
                    session.status = Some(ThreadInboxStatus::Approval);
                }
                true
            }
            DomainEvent::UserInputRequested { .. } => {
                if let Some(session) = self.session_mut(&push.thread_id) {
                    session.status = Some(ThreadInboxStatus::Input);
                }
                true
            }
            DomainEvent::ApprovalResolved { .. } | DomainEvent::UserInputResolved { .. } => {
                if let Some(session) = self.session_mut(&push.thread_id) {
                    session.status = Some(if session.running {
                        ThreadInboxStatus::Working
                    } else {
                        ThreadInboxStatus::Ready
                    });
                }
                true
            }
            // Deltas are the hot path. The transcript model consumes them,
            // while the rail remains untouched until status actually changes.
            DomainEvent::ItemStarted { .. }
            | DomainEvent::ItemDelta { .. }
            | DomainEvent::ItemCompleted { .. }
            | DomainEvent::PlanUpdated { .. }
            | DomainEvent::UsageUpdated { .. }
            | DomainEvent::DiffUpdated { .. }
            | DomainEvent::ApprovalReviewStarted { .. }
            | DomainEvent::ApprovalReviewCompleted { .. }
            | DomainEvent::ThreadStarted { .. } => false,
        }
    }

    fn session_mut(&mut self, thread_id: &str) -> Option<&mut harness_protocol::SessionSummary> {
        self.projects
            .iter_mut()
            .flat_map(|project| project.sessions.iter_mut())
            .find(|session| session.id == thread_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_client::ClientEvent;
    use harness_protocol::Push;
    use serde_json::json;

    #[test]
    fn streamed_delta_does_not_request_a_frame() {
        let mut state = ClientState::new(true);
        let changed = state.handle_event(ClientEvent::Push(Push {
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

        assert!(!changed);
    }

    #[test]
    fn projects_response_replaces_the_snapshot() {
        let mut state = ClientState::new(true);
        state
            .pending
            .insert("native-1".into(), PendingRequest::Projects);

        assert!(state.handle_response(Response::Success {
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
        }));
        assert!(state.projects_loaded);
        assert_eq!(state.projects[0].name, "Harness");
    }
}
