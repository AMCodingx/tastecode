use async_channel::Receiver as EventReceiver;
use harness_client::{ClientEvent, ClientHandle, ConnectionState, Endpoint};
use harness_protocol::{
    AcpAgentsResult, ApprovalMode, DomainEvent, Model, ModelConnectionsResult, ModelsListResult,
    PROTOCOL_VERSION, ProjectAddedResult, ProjectSummary, ProjectsListResult, ProviderId,
    ProviderStatus, ProvidersListResult, Response, SendTurnResult, ServerWelcome, SessionSummary,
    SidebarMode, SidebarSettings, ThreadEventPush, ThreadHistoryResult, ThreadInboxStatus,
    ThreadLifecycle, ThreadLifecyclePush, ThreadQueuePush, ThreadQueueResult, ThreadStartResult,
    channel, method,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct ClientState {
    client: Option<ClientHandle>,
    pub(crate) connection: ConnectionState,
    pub(crate) projects: Vec<ProjectSummary>,
    pub(crate) projects_loaded: bool,
    pub(crate) sidebar_settings: SidebarSettings,
    pub(crate) provider_statuses: Vec<ProviderStatus>,
    pub(crate) model_catalog: Vec<ModelChoice>,
    pub(crate) model_catalog_loaded: bool,
    pending: HashMap<String, PendingRequest>,
    catalog_discovery_pending: usize,
    catalog_model_pending: usize,
    pub(crate) notice: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelChoice {
    pub(crate) key: String,
    pub(crate) provider: ProviderId,
    pub(crate) source_name: String,
    pub(crate) connection_id: Option<String>,
    pub(crate) agent_id: Option<String>,
    pub(crate) agent_name: Option<String>,
    pub(crate) model: Model,
    catalog_order: (u8, usize, usize),
}

#[derive(Clone)]
struct ModelSource {
    key: String,
    provider: ProviderId,
    source_name: String,
    connection_id: Option<String>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    fallback_model: Option<Model>,
    catalog_group: u8,
    source_index: usize,
}

#[derive(Clone)]
pub(crate) struct NewThreadRequest {
    pub(crate) project_path: String,
    pub(crate) text: String,
    pub(crate) title: String,
    pub(crate) choice: ModelChoice,
    pub(crate) effort: Option<String>,
    pub(crate) service_tier: Option<String>,
    pub(crate) approval: ApprovalMode,
    pub(crate) isolate: bool,
}

enum PendingRequest {
    Capabilities,
    Projects,
    SidebarSettings,
    Providers,
    Connections,
    AcpAgents,
    Models { source: ModelSource },
    AddProject { path: String },
    StartThread { request: NewThreadRequest },
    RenameThread,
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
    pub(crate) shell_events: Vec<ShellEvent>,
}

pub(crate) enum ShellEvent {
    ProjectAdded {
        path: String,
    },
    ThreadStarted {
        thread_id: String,
        project_path: String,
        title: String,
        provider: ProviderId,
    },
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
    DraftError {
        message: String,
        restore_text: String,
    },
}

impl ClientUpdate {
    fn shell_changed() -> Self {
        Self {
            shell_changed: true,
            chat: Vec::new(),
            shell_events: Vec::new(),
        }
    }

    fn chat(update: ChatUpdate) -> Self {
        Self {
            shell_changed: false,
            chat: vec![update],
            shell_events: Vec::new(),
        }
    }

    fn shell_event(event: ShellEvent) -> Self {
        Self {
            shell_changed: true,
            chat: Vec::new(),
            shell_events: vec![event],
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
            provider_statuses: Vec::new(),
            model_catalog: Vec::new(),
            model_catalog_loaded: fixture,
            pending: HashMap::new(),
            catalog_discovery_pending: 0,
            catalog_model_pending: 0,
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
                    return ClientUpdate {
                        shell_changed: true,
                        chat: vec![ChatUpdate::Refresh],
                        shell_events: Vec::new(),
                    };
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
                    shell_events: Vec::new(),
                }
            }
            ClientEvent::RequestAborted { id } => self.handle_aborted_request(&id),
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

    pub(crate) fn add_project(&mut self, path: String) {
        self.send_request(
            method::PROJECTS_ADD,
            json!({ "path": path }),
            PendingRequest::AddProject { path },
        );
    }

    pub(crate) fn start_thread(&mut self, request: NewThreadRequest) {
        let mut params = serde_json::Map::from_iter([
            ("provider".into(), json!(request.choice.provider)),
            ("workspacePath".into(), json!(request.project_path)),
            ("approval".into(), json!(request.approval)),
        ]);
        if let Some(agent) = &request.choice.agent_id {
            params.insert("agent".into(), json!(agent));
        }
        if let Some(connection_id) = &request.choice.connection_id {
            params.insert("connectionId".into(), json!(connection_id));
        }
        if !request.choice.model.id.is_empty() {
            params.insert("model".into(), json!(request.choice.model.id));
        }
        if let Some(service_tier) = &request.service_tier {
            params.insert("serviceTier".into(), json!(service_tier));
        }
        if let Some(effort) = &request.effort {
            params.insert("effort".into(), json!(effort));
        }
        if request.isolate {
            params.insert("isolate".into(), json!(true));
        }
        self.send_request(
            method::THREAD_START,
            Value::Object(params),
            PendingRequest::StartThread { request },
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
        self.request_model_catalog();
    }

    fn request_model_catalog(&mut self) {
        self.pending
            .retain(|_, request| !request.is_catalog_request());
        self.provider_statuses.clear();
        self.model_catalog.clear();
        self.model_catalog_loaded = false;
        self.catalog_discovery_pending = 0;
        self.catalog_model_pending = 0;
        if self.send_request(method::PROVIDERS_LIST, json!({}), PendingRequest::Providers) {
            self.catalog_discovery_pending += 1;
        }
        if self.send_request(
            method::CONNECTIONS_LIST,
            json!({}),
            PendingRequest::Connections,
        ) {
            self.catalog_discovery_pending += 1;
        }
        if self.send_request(method::ACP_AGENTS, json!({}), PendingRequest::AcpAgents) {
            self.catalog_discovery_pending += 1;
        }
        self.update_catalog_loaded();
    }

    fn request_projects(&mut self) {
        self.send_request(method::PROJECTS_LIST, json!({}), PendingRequest::Projects);
    }

    fn send_request(&mut self, method: &str, params: Value, request: PendingRequest) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        match client.request(method, params) {
            Ok(id) => {
                self.pending.insert(id, request);
                true
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                false
            }
        }
    }

    fn handle_response(&mut self, response: Response<Value>) -> ClientUpdate {
        let pending = self.pending.remove(response.id());
        match response {
            Response::Failure { error, .. } => {
                if pending
                    .as_ref()
                    .is_some_and(PendingRequest::is_catalog_request)
                {
                    self.finish_catalog_request(pending.as_ref().expect("checked above"));
                    return ClientUpdate::shell_changed();
                }
                let message = match error.detail {
                    Some(detail) => format!("{} ({detail})", error.message),
                    None => error.message,
                };
                match pending {
                    Some(PendingRequest::StartThread { request }) => {
                        ClientUpdate::chat(ChatUpdate::DraftError {
                            message,
                            restore_text: request.text,
                        })
                    }
                    Some(request) => match request.thread_id() {
                        Some(thread_id) => {
                            ClientUpdate::chat(ChatUpdate::Error { thread_id, message })
                        }
                        None => {
                            self.notice = Some(message);
                            ClientUpdate::shell_changed()
                        }
                    },
                    None => {
                        self.notice = Some(message);
                        ClientUpdate::shell_changed()
                    }
                }
            }
            Response::Success { result, .. } => match pending {
                Some(PendingRequest::Projects) => self.handle_projects_response(result),
                Some(PendingRequest::SidebarSettings) => self.handle_settings_response(result),
                Some(PendingRequest::Providers) => self.handle_providers_response(result),
                Some(PendingRequest::Connections) => self.handle_connections_response(result),
                Some(PendingRequest::AcpAgents) => self.handle_acp_agents_response(result),
                Some(PendingRequest::Models { source }) => {
                    self.handle_models_response(result, source)
                }
                Some(PendingRequest::AddProject { path }) => {
                    self.handle_add_project_response(result, path)
                }
                Some(PendingRequest::StartThread { request }) => {
                    self.handle_start_thread_response(result, request)
                }
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
                | Some(PendingRequest::RenameThread)
                | Some(PendingRequest::Capabilities)
                | None => ClientUpdate::default(),
            },
        }
    }

    fn handle_aborted_request(&mut self, id: &str) -> ClientUpdate {
        let Some(pending) = self.pending.remove(id) else {
            return ClientUpdate::default();
        };
        if pending.is_catalog_request() {
            self.finish_catalog_request(&pending);
            return ClientUpdate::shell_changed();
        }
        match pending {
            PendingRequest::StartThread { request } => ClientUpdate::chat(ChatUpdate::DraftError {
                message: "The server connection was lost before the session was created.".into(),
                restore_text: request.text,
            }),
            request => match request.thread_id() {
                Some(thread_id) => ClientUpdate::chat(ChatUpdate::Error {
                    thread_id,
                    message: "The server connection was lost before this request completed.".into(),
                }),
                None => {
                    self.notice = Some("The server connection was lost; refreshing state.".into());
                    ClientUpdate::shell_changed()
                }
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

    fn handle_add_project_response(
        &mut self,
        result: Value,
        requested_path: String,
    ) -> ClientUpdate {
        match serde_json::from_value::<ProjectAddedResult>(result) {
            Ok(project) => {
                let path = project.path.clone();
                if let Some(existing) = self
                    .projects
                    .iter_mut()
                    .find(|existing| existing.path == project.path)
                {
                    existing.name = project.name;
                    existing.pinned = project.pinned;
                    existing.created_at = project.created_at;
                } else {
                    self.projects.push(ProjectSummary {
                        path: project.path,
                        name: project.name,
                        pinned: project.pinned,
                        created_at: project.created_at,
                        sessions: Vec::new(),
                    });
                }
                self.projects_loaded = true;
                self.notice = None;
                ClientUpdate::shell_event(ShellEvent::ProjectAdded { path })
            }
            Err(error) => {
                self.notice = Some(format!(
                    "projects.add returned invalid data for {requested_path}: {error}"
                ));
                ClientUpdate::shell_changed()
            }
        }
    }

    fn handle_start_thread_response(
        &mut self,
        result: Value,
        request: NewThreadRequest,
    ) -> ClientUpdate {
        let started = match serde_json::from_value::<ThreadStartResult>(result) {
            Ok(started) => started,
            Err(error) => {
                return ClientUpdate::chat(ChatUpdate::DraftError {
                    message: format!("thread.start was invalid: {error}"),
                    restore_text: request.text,
                });
            }
        };
        let thread_id = started.thread_id;
        let now = unix_time_ms();
        if let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| project.path == request.project_path)
            && !project
                .sessions
                .iter()
                .any(|session| session.id == thread_id)
        {
            project.sessions.insert(
                0,
                SessionSummary {
                    id: thread_id.clone(),
                    title: request.title.clone(),
                    provider: request.choice.provider,
                    agent: request.choice.agent_id.clone(),
                    created_at: now,
                    running: true,
                    pinned: false,
                    status: Some(ThreadInboxStatus::Starting),
                    unread: Some(false),
                    lifecycle: Some(ThreadLifecycle::Active {
                        keep_active: false,
                        woke_at: None,
                    }),
                    closed_at: None,
                    worktree_branch: None,
                },
            );
        }

        self.send_request(
            method::THREAD_RENAME,
            json!({ "threadId": thread_id, "title": request.title }),
            PendingRequest::RenameThread,
        );
        self.send_turn(&thread_id, request.text, false);
        self.request_projects();
        ClientUpdate::shell_event(ShellEvent::ThreadStarted {
            thread_id,
            project_path: request.project_path,
            title: request.title,
            provider: request.choice.provider,
        })
    }

    fn handle_providers_response(&mut self, result: Value) -> ClientUpdate {
        match serde_json::from_value::<ProvidersListResult>(result) {
            Ok(result) => {
                self.provider_statuses = result.providers;
                let sources = self
                    .provider_statuses
                    .iter()
                    .filter(|provider| {
                        provider.installed
                            && !matches!(provider.id, ProviderId::Acp | ProviderId::Api)
                    })
                    .enumerate()
                    .map(|(source_index, provider)| ModelSource {
                        key: provider_key(provider.id).into(),
                        provider: provider.id,
                        source_name: provider.display_name.clone(),
                        connection_id: None,
                        agent_id: None,
                        agent_name: None,
                        fallback_model: None,
                        catalog_group: 0,
                        source_index,
                    })
                    .collect::<Vec<_>>();
                for source in sources {
                    self.request_models(source, None);
                }
            }
            Err(error) => {
                self.notice = Some(format!("providers.list was invalid: {error}"));
            }
        }
        self.finish_catalog_discovery();
        ClientUpdate::shell_changed()
    }

    fn handle_connections_response(&mut self, result: Value) -> ClientUpdate {
        match serde_json::from_value::<ModelConnectionsResult>(result) {
            Ok(result) => {
                for (source_index, connection) in result
                    .connections
                    .into_iter()
                    .filter(|connection| connection.enabled && connection.credential_configured)
                    .enumerate()
                {
                    let fallback_model = connection.default_model.as_ref().map(|id| Model {
                        id: id.clone(),
                        display_name: id.clone(),
                        description: None,
                        is_default: true,
                        reasoning_efforts: Vec::new(),
                        default_reasoning_effort: None,
                        service_tiers: Vec::new(),
                        default_service_tier: None,
                    });
                    let connection_id = connection.id;
                    self.request_models(
                        ModelSource {
                            key: format!("api:{connection_id}"),
                            provider: ProviderId::Api,
                            source_name: connection.display_name,
                            connection_id: Some(connection_id.clone()),
                            agent_id: None,
                            agent_name: None,
                            fallback_model,
                            catalog_group: 2,
                            source_index,
                        },
                        Some(json!({ "connectionId": connection_id })),
                    );
                }
            }
            Err(error) => {
                self.notice = Some(format!("connections.list was invalid: {error}"));
            }
        }
        self.finish_catalog_discovery();
        ClientUpdate::shell_changed()
    }

    fn handle_acp_agents_response(&mut self, result: Value) -> ClientUpdate {
        match serde_json::from_value::<AcpAgentsResult>(result) {
            Ok(result) => {
                for (source_index, agent) in result
                    .agents
                    .into_iter()
                    .filter(|agent| agent.installed)
                    .enumerate()
                {
                    let agent_id = agent.id;
                    self.request_models(
                        ModelSource {
                            key: format!("acp:{agent_id}"),
                            provider: ProviderId::Acp,
                            source_name: agent.name.clone(),
                            connection_id: None,
                            agent_id: Some(agent_id.clone()),
                            agent_name: Some(agent.name),
                            fallback_model: None,
                            catalog_group: 1,
                            source_index,
                        },
                        Some(json!({ "provider": "acp", "agent": agent_id })),
                    );
                }
            }
            Err(error) => {
                self.notice = Some(format!("acp.agents was invalid: {error}"));
            }
        }
        self.finish_catalog_discovery();
        ClientUpdate::shell_changed()
    }

    fn request_models(&mut self, source: ModelSource, params: Option<Value>) {
        let (request_method, params) = match (source.provider, params) {
            (ProviderId::Api, Some(params)) => (method::CONNECTIONS_MODELS, params),
            (_, Some(params)) => (method::MODELS_LIST, params),
            (provider, None) => (
                method::MODELS_LIST,
                json!({ "provider": provider_key(provider) }),
            ),
        };
        if self.send_request(request_method, params, PendingRequest::Models { source }) {
            self.catalog_model_pending += 1;
        }
    }

    fn handle_models_response(&mut self, result: Value, source: ModelSource) -> ClientUpdate {
        match serde_json::from_value::<ModelsListResult>(result) {
            Ok(result) => {
                let models = if result.models.is_empty() {
                    source.fallback_model.clone().into_iter().collect()
                } else {
                    result.models
                };
                self.model_catalog
                    .extend(models.into_iter().enumerate().map(|(model_index, model)| {
                        ModelChoice {
                            key: format!("{}\u{1f}{}", source.key, model.id),
                            provider: source.provider,
                            source_name: source.source_name.clone(),
                            connection_id: source.connection_id.clone(),
                            agent_id: source.agent_id.clone(),
                            agent_name: source.agent_name.clone(),
                            model,
                            catalog_order: (source.catalog_group, source.source_index, model_index),
                        }
                    }));
            }
            Err(error) => {
                self.notice = Some(format!("models.list was invalid: {error}"));
            }
        }
        self.catalog_model_pending = self.catalog_model_pending.saturating_sub(1);
        self.update_catalog_loaded();
        ClientUpdate::shell_changed()
    }

    fn finish_catalog_discovery(&mut self) {
        self.catalog_discovery_pending = self.catalog_discovery_pending.saturating_sub(1);
        self.update_catalog_loaded();
    }

    fn finish_catalog_request(&mut self, request: &PendingRequest) {
        match request {
            PendingRequest::Providers | PendingRequest::Connections | PendingRequest::AcpAgents => {
                self.finish_catalog_discovery()
            }
            PendingRequest::Models { .. } => {
                self.catalog_model_pending = self.catalog_model_pending.saturating_sub(1);
                self.update_catalog_loaded();
            }
            _ => {}
        }
    }

    fn update_catalog_loaded(&mut self) {
        self.model_catalog_loaded =
            self.catalog_discovery_pending == 0 && self.catalog_model_pending == 0;
        if self.model_catalog_loaded {
            self.model_catalog
                .sort_by_key(|choice| choice.catalog_order);
        }
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
            shell_events: Vec::new(),
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
    fn is_catalog_request(&self) -> bool {
        matches!(
            self,
            Self::Providers | Self::Connections | Self::AcpAgents | Self::Models { .. }
        )
    }

    fn thread_id(self) -> Option<String> {
        match self {
            Self::History { thread_id, .. }
            | Self::Queue { thread_id }
            | Self::SendTurn { thread_id, .. }
            | Self::Steer { thread_id }
            | Self::Interrupt { thread_id } => Some(thread_id),
            Self::Capabilities
            | Self::Projects
            | Self::SidebarSettings
            | Self::AddProject { .. }
            | Self::StartThread { .. }
            | Self::RenameThread
            | Self::Providers
            | Self::Connections
            | Self::AcpAgents
            | Self::Models { .. } => None,
        }
    }
}

fn unix_time_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_millis() as f64)
}

fn provider_key(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "codex",
        ProviderId::ClaudeCode => "claude-code",
        ProviderId::Cursor => "cursor",
        ProviderId::OpenCode => "opencode",
        ProviderId::Acp => "acp",
        ProviderId::Api => "api",
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

    #[test]
    fn model_response_builds_a_provider_owned_choice() {
        let mut state = ClientState::new(true);
        state.model_catalog_loaded = false;
        state.catalog_model_pending = 1;
        state.pending.insert(
            "native-model".into(),
            PendingRequest::Models {
                source: ModelSource {
                    key: "codex".into(),
                    provider: ProviderId::Codex,
                    source_name: "Codex".into(),
                    connection_id: None,
                    agent_id: None,
                    agent_name: None,
                    fallback_model: None,
                    catalog_group: 0,
                    source_index: 0,
                },
            },
        );

        let update = state.handle_response(Response::Success {
            id: "native-model".into(),
            result: json!({
                "models": [{
                    "id": "gpt-test",
                    "displayName": "GPT Test",
                    "isDefault": true,
                    "reasoningEfforts": ["low", "high"],
                    "serviceTiers": []
                }]
            }),
        });

        assert!(update.shell_changed);
        assert!(state.model_catalog_loaded);
        assert_eq!(state.model_catalog[0].provider, ProviderId::Codex);
        assert_eq!(state.model_catalog[0].model.id, "gpt-test");
    }

    #[test]
    fn starting_a_thread_promotes_the_draft_and_adds_the_sidebar_row() {
        let mut state = ClientState::new(true);
        state.projects.push(ProjectSummary {
            path: "/workspace".into(),
            name: "Harness".into(),
            pinned: false,
            created_at: 1.0,
            sessions: Vec::new(),
        });
        state.pending.insert(
            "native-start".into(),
            PendingRequest::StartThread {
                request: NewThreadRequest {
                    project_path: "/workspace".into(),
                    text: "Build it".into(),
                    title: "Build it".into(),
                    choice: ModelChoice {
                        key: "codex\u{1f}gpt-test".into(),
                        provider: ProviderId::Codex,
                        source_name: "Codex".into(),
                        connection_id: None,
                        agent_id: None,
                        agent_name: None,
                        model: Model {
                            id: "gpt-test".into(),
                            display_name: "GPT Test".into(),
                            description: None,
                            is_default: true,
                            reasoning_efforts: vec!["high".into()],
                            default_reasoning_effort: Some("high".into()),
                            service_tiers: Vec::new(),
                            default_service_tier: None,
                        },
                        catalog_order: (0, 0, 0),
                    },
                    effort: Some("high".into()),
                    service_tier: None,
                    approval: ApprovalMode::Ask,
                    isolate: false,
                },
            },
        );

        let update = state.handle_response(Response::Success {
            id: "native-start".into(),
            result: json!({ "threadId": "thread-1" }),
        });

        assert!(matches!(
            update.shell_events.as_slice(),
            [ShellEvent::ThreadStarted { thread_id, .. }] if thread_id == "thread-1"
        ));
        assert_eq!(state.projects[0].sessions[0].id, "thread-1");
        assert_eq!(
            state.projects[0].sessions[0].status,
            Some(ThreadInboxStatus::Starting)
        );
    }
}
