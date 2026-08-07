use crate::ServerState;
use crate::api_workspace_tools::ApiWorkspaceToolFactory;
use crate::model_connections::ModelConnectionStore;
use harness_adapter_api::{ApiRuntime, ApiToolFactory};
use harness_adapter_claude_code::ClaudeCodeRuntime;
use harness_adapter_codex::CodexRuntime;
use harness_adapter_cursor::CursorRuntime;
use harness_agent::{
    AgentError, AgentHandlers, AgentRuntime, AgentSession, AgentSessionState, ControlHandlers,
    CredentialValues, ProviderControl, StartOptions, TurnOptions,
};
use harness_credentials::CredentialStore;
use harness_protocol::{
    Account, ApprovalDecision, AuthEventPush, AuthStartLoginResult, DomainEvent, McpAuth,
    McpConfigValue, McpListResult, McpOAuthPush, McpOAuthStartResult, McpServer, McpServerConfig,
    McpServerScope, McpStartupStatus, McpTransport, Model, ProviderId, QueuedTurn, SendTurnResult,
    Skill, SkillSource, SkillsListResult, Thread, ThreadEventPush, ThreadInboxStatus,
    ThreadLifecyclePush, ThreadQueuePush, ThreadQueueResult, channel,
};
use harness_store::{NewCheckpoint, NewThread};
use harness_workspace::Worktree;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use uuid::Uuid;

const REPLY_STYLE_INSTRUCTIONS: &str = "Write like a clear, capable teammate.\n\n- Lead with the useful answer or outcome.\n- Use plain, specific language. Avoid generic AI filler, canned praise, and throat-clearing.\n- Keep routine replies compact, but include detail when the task needs it.\n- Prefer short paragraphs and only use lists when they improve scanning.\n- Do not use em dashes. Use a comma, colon, parentheses, or a new sentence instead.\n- Be warm and direct without slang overload or forced personality.\n- Never omit risks, blockers, or verification results just to sound concise.";

pub(crate) trait RuntimeRegistry: Send + Sync {
    fn runtime(
        &self,
        provider: ProviderId,
        agent: Option<&str>,
        connection_id: Option<&str>,
    ) -> Result<Arc<dyn AgentRuntime>, AgentError>;
}

pub(crate) struct NativeRuntimes {
    codex: Arc<CodexRuntime>,
    claude: Arc<ClaudeCodeRuntime>,
    cursor: Arc<CursorRuntime>,
    model_connections: Arc<Mutex<ModelConnectionStore>>,
    credentials: Arc<dyn CredentialStore>,
    api_tools: Arc<ApiWorkspaceToolFactory>,
}

impl NativeRuntimes {
    pub(crate) fn new(
        model_connections: Arc<Mutex<ModelConnectionStore>>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            codex: Arc::new(CodexRuntime::default()),
            claude: Arc::new(ClaudeCodeRuntime::default()),
            cursor: Arc::new(CursorRuntime::default()),
            model_connections,
            credentials,
            api_tools: Arc::new(ApiWorkspaceToolFactory),
        }
    }
}

impl RuntimeRegistry for NativeRuntimes {
    fn runtime(
        &self,
        provider: ProviderId,
        agent: Option<&str>,
        connection_id: Option<&str>,
    ) -> Result<Arc<dyn AgentRuntime>, AgentError> {
        match provider {
            ProviderId::Codex if agent.is_none() && connection_id.is_none() => {
                Ok(self.codex.clone())
            }
            ProviderId::Codex => Err(AgentError::Failed(
                "Codex does not accept an ACP agent or model connection".into(),
            )),
            ProviderId::ClaudeCode if agent.is_none() && connection_id.is_none() => {
                Ok(self.claude.clone())
            }
            ProviderId::ClaudeCode => Err(AgentError::Failed(
                "Claude Code does not accept an ACP agent or model connection".into(),
            )),
            ProviderId::Cursor if agent.is_none() && connection_id.is_none() => {
                Ok(self.cursor.clone())
            }
            ProviderId::Cursor => Err(AgentError::Failed(
                "Cursor does not accept an ACP agent or model connection".into(),
            )),
            ProviderId::Api if agent.is_none() => {
                let connection_id = connection_id.ok_or_else(|| {
                    AgentError::Failed("connectionId is required for direct API sessions".into())
                })?;
                let connection = lock(&self.model_connections)
                    .get(connection_id)
                    .map_err(|error| AgentError::Failed(error.to_string()))?;
                if !connection.enabled {
                    return Err(AgentError::Failed(format!(
                        "model connection \"{connection_id}\" is disabled"
                    )));
                }
                let api_key = self
                    .credentials
                    .read(&connection.credential_ref)
                    .map_err(|error| AgentError::Failed(error.to_string()))?;
                let tools: Arc<dyn ApiToolFactory> = self.api_tools.clone();
                Ok(Arc::new(ApiRuntime::new(
                    connection.input(),
                    api_key,
                    tools,
                )))
            }
            ProviderId::Api => Err(AgentError::Failed(
                "direct API sessions do not accept an ACP agent".into(),
            )),
            provider => Err(AgentError::Failed(format!(
                "provider \"{}\" is not implemented in the native server yet",
                provider_key(provider)
            ))),
        }
    }
}

pub(crate) struct StartThreadRequest {
    pub provider: ProviderId,
    pub agent: Option<String>,
    pub connection_id: Option<String>,
    pub workspace_path: String,
    pub model: Option<String>,
    pub service_tier: Option<String>,
    pub effort: Option<String>,
    pub approval: Option<harness_protocol::ApprovalMode>,
    pub isolate: bool,
}

pub(crate) struct SubmitTurnRequest {
    pub thread_id: String,
    pub text: String,
    pub attachments: Vec<String>,
    pub options: TurnOptions,
}

#[derive(Clone)]
struct QueuedEntry {
    turn: QueuedTurn,
    options: TurnOptions,
}

type ProjectSession = (String, Arc<dyn AgentSession>);

#[derive(Default)]
struct LiveState {
    sessions: HashMap<String, Arc<dyn AgentSession>>,
    session_order: Vec<String>,
    active_turns: HashSet<String>,
    starting_turns: HashSet<String>,
    terminal_while_starting: HashSet<String>,
    queues: HashMap<String, VecDeque<QueuedEntry>>,
    draining: HashSet<String>,
}

struct ResumeSlot {
    result: Mutex<Option<Result<Arc<dyn AgentSession>, String>>>,
    ready: Condvar,
}

impl ResumeSlot {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ControlKey {
    provider: ProviderId,
    agent: Option<String>,
}

struct ControlSlot {
    result: Mutex<Option<Result<Arc<dyn ProviderControl>, String>>>,
    ready: Condvar,
}

impl ControlSlot {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }
}

pub(crate) struct AgentManager {
    runtimes: Arc<dyn RuntimeRegistry>,
    live: Mutex<LiveState>,
    resumes: Mutex<HashMap<String, Arc<ResumeSlot>>>,
    controls: Mutex<HashMap<ControlKey, Arc<dyn ProviderControl>>>,
    control_starts: Mutex<HashMap<ControlKey, Arc<ControlSlot>>>,
    watched_mcp_projects: Arc<Mutex<HashMap<ProviderId, HashSet<String>>>>,
    watched_skill_projects: Arc<Mutex<HashMap<ProviderId, HashSet<String>>>>,
    worktree_root: PathBuf,
}

impl AgentManager {
    pub(crate) fn new(runtimes: Arc<dyn RuntimeRegistry>) -> Self {
        Self {
            runtimes,
            live: Mutex::new(LiveState::default()),
            resumes: Mutex::new(HashMap::new()),
            controls: Mutex::new(HashMap::new()),
            control_starts: Mutex::new(HashMap::new()),
            watched_mcp_projects: Arc::new(Mutex::new(HashMap::new())),
            watched_skill_projects: Arc::new(Mutex::new(HashMap::new())),
            worktree_root: std::env::temp_dir().join("personal-harness-trees"),
        }
    }

    pub(crate) fn account(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        agent: Option<&str>,
    ) -> Result<Account, String> {
        self.control(state, provider, agent)?
            .account()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn start_login(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        agent: Option<&str>,
    ) -> Result<AuthStartLoginResult, String> {
        self.control(state, provider, agent)?
            .start_login()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn cancel_login(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        agent: Option<&str>,
        login_id: &str,
    ) -> Result<(), String> {
        self.control(state, provider, agent)?
            .cancel_login(login_id)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn use_api_key(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        agent: Option<&str>,
        api_key: &str,
    ) -> Result<Account, String> {
        self.control(state, provider, agent)?
            .use_api_key(api_key)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn sign_out(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        agent: Option<&str>,
    ) -> Result<(), String> {
        self.control(state, provider, agent)?
            .sign_out()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn list_mcp_servers(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        project_path: &str,
    ) -> Result<McpListResult, String> {
        watch_project(&self.watched_mcp_projects, provider, project_path);
        let active = self.project_session(state, provider, project_path)?;
        let mut inventory = match active {
            Some((thread_id, session)) => match session.list_mcp_servers(&thread_id) {
                Ok(inventory) => inventory,
                Err(AgentError::Unsupported(_)) => self
                    .control(state, provider, None)?
                    .list_mcp_servers()
                    .map_err(|error| error.to_string())?,
                Err(error) => return Err(error.to_string()),
            },
            None => self
                .control(state, provider, None)?
                .list_mcp_servers()
                .map_err(|error| error.to_string())?,
        };
        let configured = lock(&state.mcp_config)
            .list(provider, project_path)
            .map_err(|error| error.to_string())?;
        let mut positions = inventory
            .servers
            .iter()
            .enumerate()
            .map(|(index, server)| (server.id.clone(), index))
            .collect::<HashMap<_, _>>();
        for config in configured {
            let index = positions.get(&config.id).copied().unwrap_or_else(|| {
                inventory.servers.push(McpServer {
                    id: config.id.clone(),
                    display_name: None,
                    description: None,
                    version: None,
                    scope: McpServerScope::Project,
                    enabled: config.enabled,
                    transport: None,
                    auth: McpAuth::NotRequired,
                    startup: McpStartupStatus::Stopped,
                    tools: Vec::new(),
                    resources: Vec::new(),
                    resource_templates: Vec::new(),
                });
                let index = inventory.servers.len() - 1;
                positions.insert(config.id.clone(), index);
                index
            });
            let server = &mut inventory.servers[index];
            server.scope = McpServerScope::Project;
            server.enabled = config.enabled;
            if config.enabled {
                server.transport = config.transport;
                if config.display_name.is_some() {
                    server.display_name = config.display_name;
                }
            } else {
                server.startup = McpStartupStatus::Stopped;
            }
        }
        Ok(inventory)
    }

    pub(crate) fn reload_mcp_servers(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        project_path: &str,
    ) -> Result<(), String> {
        let (thread_id, session) = self
            .project_session(state, provider, project_path)?
            .ok_or_else(|| {
                "start a compatible session for this project before reloading MCP servers"
                    .to_owned()
            })?;
        let (servers, credentials) = self.mcp_runtime_config(state, provider, project_path)?;
        session
            .reload_mcp_servers(&thread_id, &servers, &credentials)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn start_mcp_o_auth(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        project_path: &str,
        server_id: &str,
    ) -> Result<McpOAuthStartResult, String> {
        let (thread_id, session) = self
            .project_session(state, provider, project_path)?
            .ok_or_else(|| {
                "start a compatible session for this project before signing in to an MCP server"
                    .to_owned()
            })?;
        session
            .start_mcp_o_auth(server_id, &thread_id)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn cancel_mcp_o_auth(&self, provider: ProviderId) -> Result<(), String> {
        Err(format!(
            "provider \"{}\" cannot cancel MCP OAuth; close the browser flow instead",
            provider_key(provider)
        ))
    }

    pub(crate) fn list_skills(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        project_path: &str,
    ) -> Result<SkillsListResult, String> {
        watch_project(&self.watched_skill_projects, provider, project_path);
        self.control(state, provider, None)?
            .list_skills(project_path)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn set_skill_enabled(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        project_path: &str,
        skill_id: &str,
        enabled: bool,
    ) -> Result<bool, String> {
        watch_project(&self.watched_skill_projects, provider, project_path);
        self.control(state, provider, None)?
            .set_skill_enabled(skill_id, enabled)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn install_skill(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        project_path: &str,
        folder_path: &str,
    ) -> Result<Skill, String> {
        watch_project(&self.watched_skill_projects, provider, project_path);
        let control = self.control(state, provider, None)?;
        let current = control
            .list_skills(project_path)
            .map_err(|error| error.to_string())?;
        if !current.capabilities.install {
            return Err("this provider cannot install skills".into());
        }
        let destination = crate::skill_install::install_local_skill(project_path, folder_path)
            .map_err(|error| error.to_string())?;
        let discovered = control
            .list_skills(project_path)
            .map_err(|error| error.to_string())
            .and_then(|inventory| {
                inventory
                    .skills
                    .into_iter()
                    .find(|skill| match &skill.source {
                        SkillSource::Folder { path } => same_path(path, &destination),
                        SkillSource::Provider => false,
                    })
                    .ok_or_else(|| {
                        inventory
                            .errors
                            .iter()
                            .find(|error| path_inside(&destination, &error.path))
                            .map(|error| error.message.clone())
                            .unwrap_or_else(|| {
                                "provider did not discover the installed skill".into()
                            })
                    })
            });
        if discovered.is_err() {
            let _ = std::fs::remove_dir_all(destination);
        }
        discovered
    }

    pub(crate) fn list_models(
        &self,
        provider: ProviderId,
        agent: Option<&str>,
    ) -> Result<Vec<Model>, String> {
        self.runtimes
            .runtime(provider, agent, None)
            .and_then(|runtime| runtime.list_models())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn list_connection_models(
        &self,
        state: &ServerState,
        connection_id: &str,
    ) -> Result<Vec<Model>, String> {
        let connection = lock(&state.model_connections)
            .get(connection_id)
            .map_err(|error| error.to_string())?;
        let api_key = state
            .credentials
            .read(&connection.credential_ref)
            .map_err(|error| error.to_string())?;
        harness_adapter_api::list_models(&connection.input(), &api_key)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn start_thread(
        &self,
        state: &Arc<ServerState>,
        request: StartThreadRequest,
    ) -> Result<Thread, String> {
        let stored_connection_id = request.connection_id.clone();
        let (mcp_servers, mcp_credentials) =
            self.mcp_runtime_config(state, request.provider, &request.workspace_path)?;
        let provisional_id = format!("{}-{}", provider_key(request.provider), Uuid::new_v4());
        let worktree = request
            .isolate
            .then(|| {
                harness_workspace::create_worktree(
                    &request.workspace_path,
                    &provisional_id,
                    &self.worktree_root,
                )
                .map_err(|error| error.to_string())
            })
            .transpose()?;
        let runtime = match self.runtimes.runtime(
            request.provider,
            request.agent.as_deref(),
            request.connection_id.as_deref(),
        ) {
            Ok(runtime) => runtime,
            Err(error) => {
                cleanup_failed_worktree(worktree.as_ref());
                return Err(error.to_string());
            }
        };
        let bridge = Arc::new(EventBridge::new(Arc::downgrade(state)));
        let event_bridge = Arc::clone(&bridge);
        let handlers = session_handlers(
            state,
            event_bridge,
            request.provider,
            request.workspace_path.clone(),
        );
        let options = StartOptions {
            instructions: Some(REPLY_STYLE_INSTRUCTIONS.into()),
            model: request.model,
            service_tier: request.service_tier,
            effort: request.effort,
            approval: request.approval,
            mcp_servers,
            mcp_credentials,
            resume_state: None,
        };
        let workspace = worktree
            .as_ref()
            .map(|worktree| worktree.path.to_string_lossy().into_owned())
            .unwrap_or_else(|| request.workspace_path.clone());
        let (thread, session) = match runtime.start(&workspace, &options, handlers) {
            Ok(started) => started,
            Err(error) => {
                cleanup_failed_worktree(worktree.as_ref());
                return Err(error.to_string());
            }
        };
        let initial_state = match session.export_state() {
            Ok(state) => state,
            Err(_) => {
                session.dispose();
                cleanup_failed_worktree(worktree.as_ref());
                return Err("provider could not prepare durable session state".into());
            }
        };
        let mut inserted = false;
        let stored = {
            let store = lock(&state.store);
            store
                .add_project(&request.workspace_path, None)
                .and_then(|_| {
                    store.add_thread_with_connection(
                        NewThread {
                            id: thread.id.clone(),
                            project_path: request.workspace_path,
                            provider: request.provider,
                            agent: request.agent,
                            title: "New session".into(),
                            created_at: Some(timestamp_i64(thread.created_at)),
                            worktree_path: worktree
                                .as_ref()
                                .map(|entry| entry.path.to_string_lossy().into_owned()),
                            worktree_branch: worktree.as_ref().map(|entry| entry.branch.clone()),
                        },
                        stored_connection_id.as_deref(),
                    )
                })
                .and_then(|stored| {
                    inserted = true;
                    if let Some(initial_state) = initial_state.as_ref() {
                        store.set_provider_session_state(&thread.id, initial_state.value())?;
                    }
                    Ok(stored)
                })
        };
        if let Err(error) = stored {
            if inserted {
                let mut store = lock(&state.store);
                let _ = store.forget_worktree(&thread.id);
                let _ = store.delete_thread(&thread.id);
            }
            session.dispose();
            cleanup_failed_worktree(worktree.as_ref());
            return Err(error.to_string());
        }
        self.attach_session(&thread.id, session);
        bridge.attach(thread.id.clone());
        Ok(thread)
    }

    pub(crate) fn submit_turn(
        &self,
        state: &Arc<ServerState>,
        request: SubmitTurnRequest,
    ) -> Result<SendTurnResult, String> {
        let session = self.ensure_session(state, &request.thread_id)?;
        let queued = {
            let mut live = lock(&self.live);
            let busy = live.active_turns.contains(&request.thread_id)
                || live.starting_turns.contains(&request.thread_id)
                || live
                    .queues
                    .get(&request.thread_id)
                    .is_some_and(|queue| !queue.is_empty());
            if busy {
                let turn = QueuedTurn {
                    id: Uuid::new_v4().to_string(),
                    text: request.text.clone(),
                    attachments: request.attachments.clone(),
                    created_at: now_ms(),
                };
                live.queues
                    .entry(request.thread_id.clone())
                    .or_default()
                    .push_back(QueuedEntry {
                        turn: turn.clone(),
                        options: request.options.clone(),
                    });
                Some(turn)
            } else {
                live.starting_turns.insert(request.thread_id.clone());
                None
            }
        };
        if let Some(queued_turn) = queued {
            self.notify_queue(state, &request.thread_id);
            return Ok(SendTurnResult::Queued {
                queued: true,
                queued_turn,
            });
        }
        let turn_id = match self.run_started_turn(
            state,
            &request.thread_id,
            &request.text,
            &request.attachments,
            &request.options,
            &session,
        ) {
            Ok(turn_id) => turn_id,
            Err(error) => {
                schedule_drain(state, request.thread_id);
                return Err(error);
            }
        };
        Ok(SendTurnResult::Started {
            queued: false,
            turn_id,
        })
    }

    pub(crate) fn queue(&self, thread_id: &str) -> ThreadQueueResult {
        let live = lock(&self.live);
        queue_result(&live, thread_id)
    }

    pub(crate) fn steer_queued(
        &self,
        state: &Arc<ServerState>,
        thread_id: &str,
        queued_turn_id: &str,
    ) -> Result<(), String> {
        let (session, entry, index) = {
            let mut live = lock(&self.live);
            if !live.active_turns.contains(thread_id) {
                return Err("there is no running turn to steer".into());
            }
            let session = live
                .sessions
                .get(thread_id)
                .cloned()
                .ok_or_else(|| format!("no such live thread: {thread_id}"))?;
            if !session.capabilities().steer {
                return Err("this agent does not support steering a running turn".into());
            }
            let queue = live.queues.entry(thread_id.into()).or_default();
            let index = queue
                .iter()
                .position(|entry| entry.turn.id == queued_turn_id)
                .ok_or_else(|| "queued prompt not found".to_owned())?;
            let entry = queue.remove(index).expect("queued entry disappeared");
            (session, entry, index)
        };
        self.notify_queue(state, thread_id);
        if let Err(error) = session.steer(thread_id, &entry.turn.text, &entry.turn.attachments) {
            lock(&self.live)
                .queues
                .entry(thread_id.into())
                .or_default()
                .insert(index, entry);
            self.notify_queue(state, thread_id);
            return Err(error.to_string());
        }
        Ok(())
    }

    pub(crate) fn respond_to_approval(
        &self,
        thread_id: &str,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), String> {
        let session = self.live_session(thread_id)?;
        session
            .respond_to_approval(approval_id, decision)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn respond_to_user_input(
        &self,
        thread_id: &str,
        request_id: &str,
        answers: &HashMap<String, Vec<String>>,
    ) -> Result<(), String> {
        let session = self.live_session(thread_id)?;
        session
            .respond_to_user_input(request_id, answers)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn interrupt(&self, thread_id: &str) -> Result<(), String> {
        let session = lock(&self.live).sessions.get(thread_id).cloned();
        session
            .map(|session| session.interrupt(thread_id))
            .transpose()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn close(&self, thread_id: &str) {
        let session = {
            let mut live = lock(&self.live);
            live.active_turns.remove(thread_id);
            live.starting_turns.remove(thread_id);
            live.terminal_while_starting.remove(thread_id);
            live.queues.remove(thread_id);
            live.draining.remove(thread_id);
            live.session_order
                .retain(|candidate| candidate != thread_id);
            live.sessions.remove(thread_id)
        };
        if let Some(session) = session {
            session.dispose();
        }
    }

    pub(crate) fn dispose_all(&self) {
        let sessions = {
            let mut live = lock(&self.live);
            live.active_turns.clear();
            live.starting_turns.clear();
            live.terminal_while_starting.clear();
            live.queues.clear();
            live.draining.clear();
            live.session_order.clear();
            live.sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session in sessions {
            session.dispose();
        }
        let controls = lock(&self.controls)
            .drain()
            .map(|(_, control)| control)
            .collect::<Vec<_>>();
        for control in controls {
            control.dispose();
        }
    }

    pub(crate) fn is_running(&self, thread_id: &str) -> bool {
        let live = lock(&self.live);
        live.active_turns.contains(thread_id) || live.starting_turns.contains(thread_id)
    }

    pub(crate) fn activity_status(&self, thread_id: &str) -> Option<ThreadInboxStatus> {
        let live = lock(&self.live);
        if live.starting_turns.contains(thread_id) {
            Some(ThreadInboxStatus::Starting)
        } else if live.active_turns.contains(thread_id) {
            Some(ThreadInboxStatus::Working)
        } else if live
            .queues
            .get(thread_id)
            .is_some_and(|queue| !queue.is_empty())
        {
            Some(ThreadInboxStatus::Queued)
        } else {
            None
        }
    }

    fn run_started_turn(
        &self,
        state: &Arc<ServerState>,
        thread_id: &str,
        text: &str,
        attachments: &[String],
        options: &TurnOptions,
        session: &Arc<dyn AgentSession>,
    ) -> Result<String, String> {
        self.checkpoint(state, thread_id, text);
        let result = session
            .send_turn(thread_id, text, attachments, options)
            .map_err(|error| error.to_string());
        let completed_during_start = {
            let mut live = lock(&self.live);
            live.starting_turns.remove(thread_id);
            let completed = live.terminal_while_starting.remove(thread_id);
            if result.is_ok() && !completed {
                live.active_turns.insert(thread_id.into());
            }
            completed
        };
        if completed_during_start {
            schedule_drain(state, thread_id.into());
        }
        result
    }

    fn checkpoint(&self, state: &ServerState, thread_id: &str, label: &str) {
        let stored = lock(&state.store).thread(thread_id).ok().flatten();
        let Some(stored) = stored else {
            return;
        };
        let path = stored.worktree_path.unwrap_or(stored.project_path);
        let Ok(snapshot) = harness_workspace::take_snapshot(path) else {
            return;
        };
        let store = lock(&state.store);
        let Ok(seq) = store.last_seq(thread_id) else {
            return;
        };
        let label = label.trim().chars().take(60).collect::<String>();
        let _ = store.add_checkpoint(NewCheckpoint {
            thread_id: thread_id.into(),
            seq,
            commit: snapshot.commit,
            label: if label.is_empty() {
                "Turn".into()
            } else {
                label
            },
        });
    }

    fn ensure_session(
        &self,
        state: &Arc<ServerState>,
        thread_id: &str,
    ) -> Result<Arc<dyn AgentSession>, String> {
        if let Some(session) = lock(&self.live).sessions.get(thread_id).cloned() {
            return Ok(session);
        }
        let (slot, leader) = {
            let mut resumes = lock(&self.resumes);
            match resumes.get(thread_id) {
                Some(slot) => (Arc::clone(slot), false),
                None => {
                    let slot = Arc::new(ResumeSlot::new());
                    resumes.insert(thread_id.into(), Arc::clone(&slot));
                    (slot, true)
                }
            }
        };
        if leader {
            let result = self.resume_session(state, thread_id);
            *lock(&slot.result) = Some(result.clone());
            slot.ready.notify_all();
            lock(&self.resumes).remove(thread_id);
            result
        } else {
            let mut result = lock(&slot.result);
            while result.is_none() {
                result = slot
                    .ready
                    .wait(result)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            result.clone().expect("resume result disappeared")
        }
    }

    fn project_session(
        &self,
        state: &ServerState,
        provider: ProviderId,
        project_path: &str,
    ) -> Result<Option<ProjectSession>, String> {
        let candidates = {
            let live = lock(&self.live);
            live.session_order
                .iter()
                .filter_map(|thread_id| {
                    live.sessions
                        .get(thread_id)
                        .cloned()
                        .map(|session| (thread_id.clone(), session))
                })
                .collect::<Vec<_>>()
        };
        let store = lock(&state.store);
        for (thread_id, session) in candidates {
            let Some(thread) = store
                .thread(&thread_id)
                .map_err(|error| error.to_string())?
            else {
                continue;
            };
            if thread.closed_at.is_none()
                && thread.provider == provider
                && thread.project_path == project_path
            {
                return Ok(Some((thread_id, session)));
            }
        }
        Ok(None)
    }

    fn mcp_runtime_config(
        &self,
        state: &ServerState,
        provider: ProviderId,
        project_path: &str,
    ) -> Result<(Vec<McpServerConfig>, CredentialValues), String> {
        let servers = lock(&state.mcp_config)
            .list(provider, project_path)
            .map_err(|error| error.to_string())?;
        let mut credentials = CredentialValues::default();
        for server in servers.iter().filter(|server| server.enabled) {
            let values = match server.transport.as_ref() {
                Some(McpTransport::Stdio { environment, .. }) => environment.as_ref(),
                Some(McpTransport::Http { headers, .. }) => headers.as_ref(),
                None => None,
            };
            for value in values.into_iter().flat_map(|values| values.values()) {
                let McpConfigValue::Credential { credential_ref } = value else {
                    continue;
                };
                if credentials.get(credential_ref).is_none() {
                    let secret = state
                        .credentials
                        .read(credential_ref)
                        .map_err(|error| error.to_string())?;
                    credentials.insert(credential_ref.clone(), secret);
                }
            }
        }
        Ok((servers, credentials))
    }

    fn control(
        &self,
        state: &Arc<ServerState>,
        provider: ProviderId,
        agent: Option<&str>,
    ) -> Result<Arc<dyn ProviderControl>, String> {
        let key = ControlKey {
            provider,
            agent: agent.map(str::to_owned),
        };
        if let Some(control) = lock(&self.controls).get(&key).cloned() {
            return Ok(control);
        }
        let (slot, leader) = {
            let mut starts = lock(&self.control_starts);
            match starts.get(&key) {
                Some(slot) => (Arc::clone(slot), false),
                None => {
                    let slot = Arc::new(ControlSlot::new());
                    starts.insert(key.clone(), Arc::clone(&slot));
                    (slot, true)
                }
            }
        };
        if leader {
            let result = self.open_control(state, &key);
            if let Ok(control) = &result {
                lock(&self.controls).insert(key.clone(), Arc::clone(control));
            }
            *lock(&slot.result) = Some(result.clone());
            slot.ready.notify_all();
            lock(&self.control_starts).remove(&key);
            result
        } else {
            let mut result = lock(&slot.result);
            while result.is_none() {
                result = slot
                    .ready
                    .wait(result)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            result.clone().expect("control result disappeared")
        }
    }

    fn open_control(
        &self,
        state: &Arc<ServerState>,
        key: &ControlKey,
    ) -> Result<Arc<dyn ProviderControl>, String> {
        let runtime = self
            .runtimes
            .runtime(key.provider, key.agent.as_deref(), None)
            .map_err(|error| error.to_string())?;
        let auth_state = Arc::downgrade(state);
        let provider = key.provider;
        let agent = key.agent.clone();
        let mcp_state = Arc::downgrade(state);
        let mcp_projects = Arc::clone(&self.watched_mcp_projects);
        let skill_state = Arc::downgrade(state);
        let skill_projects = Arc::clone(&self.watched_skill_projects);
        runtime
            .open_control(
                ControlHandlers::new(
                    move |event| {
                        if let Some(state) = auth_state.upgrade() {
                            let _ = state.push.broadcast(
                                channel::AUTH_EVENT,
                                AuthEventPush {
                                    provider,
                                    agent: agent.clone(),
                                    login_id: event.login_id,
                                    success: event.success,
                                    error: event.error,
                                },
                            );
                        }
                    },
                    |line| eprintln!("[agent control] {line}"),
                )
                .with_mcp_changed(move |_| {
                    if let Some(state) = mcp_state.upgrade() {
                        broadcast_watched_projects(
                            &state,
                            &mcp_projects,
                            channel::MCP_CHANGED,
                            provider,
                        );
                    }
                })
                .with_skills_changed(move || {
                    if let Some(state) = skill_state.upgrade() {
                        broadcast_watched_projects(
                            &state,
                            &skill_projects,
                            channel::SKILLS_CHANGED,
                            provider,
                        );
                    }
                }),
            )
            .map_err(|error| error.to_string())
    }

    fn resume_session(
        &self,
        state: &Arc<ServerState>,
        thread_id: &str,
    ) -> Result<Arc<dyn AgentSession>, String> {
        let (stored, connection_id, resume_state) = {
            let store = lock(&state.store);
            let stored = store
                .thread(thread_id)
                .map_err(|error| error.to_string())?
                .filter(|thread| thread.closed_at.is_none())
                .ok_or_else(|| format!("no such thread: {thread_id}"))?;
            let connection_id = store
                .thread_connection_id(thread_id)
                .map_err(|error| error.to_string())?;
            let resume_state = store
                .provider_session_state(thread_id)
                .map_err(|error| error.to_string())?;
            (stored, connection_id, resume_state)
        };
        let runtime = self
            .runtimes
            .runtime(
                stored.provider,
                stored.agent.as_deref(),
                connection_id.as_deref(),
            )
            .map_err(|error| error.to_string())?;
        let bridge = Arc::new(EventBridge::new(Arc::downgrade(state)));
        let event_bridge = Arc::clone(&bridge);
        let handlers = session_handlers(
            state,
            event_bridge,
            stored.provider,
            stored.project_path.clone(),
        );
        let workspace = stored
            .worktree_path
            .as_deref()
            .unwrap_or(&stored.project_path);
        let (mcp_servers, mcp_credentials) =
            self.mcp_runtime_config(state, stored.provider, &stored.project_path)?;
        let options = StartOptions {
            mcp_servers,
            mcp_credentials,
            resume_state: resume_state.map(AgentSessionState::new),
            ..StartOptions::default()
        };
        let (thread, session) = runtime
            .resume(thread_id, workspace, &options, handlers)
            .map_err(|error| error.to_string())?;
        if thread.id != thread_id {
            session.dispose();
            return Err(format!("provider resumed unexpected thread {}", thread.id));
        }
        if lock(&state.store)
            .thread(thread_id)
            .map_err(|error| error.to_string())?
            .is_none_or(|thread| thread.closed_at.is_some())
        {
            session.dispose();
            return Err(format!("thread {thread_id} was closed while resuming"));
        }
        self.attach_session(thread_id, Arc::clone(&session));
        bridge.attach(thread_id.into());
        Ok(session)
    }

    fn attach_session(&self, thread_id: &str, session: Arc<dyn AgentSession>) {
        let previous = {
            let mut live = lock(&self.live);
            if !live.sessions.contains_key(thread_id) {
                live.session_order.push(thread_id.into());
            }
            live.sessions.insert(thread_id.into(), session)
        };
        if let Some(previous) = previous {
            previous.dispose();
        }
    }

    fn live_session(&self, thread_id: &str) -> Result<Arc<dyn AgentSession>, String> {
        lock(&self.live)
            .sessions
            .get(thread_id)
            .cloned()
            .ok_or_else(|| format!("no such live thread: {thread_id}"))
    }

    fn export_provider_session_state(&self, thread_id: &str) -> Option<AgentSessionState> {
        let session = lock(&self.live).sessions.get(thread_id).cloned()?;
        match session.export_state() {
            Ok(state) => state,
            Err(_) => {
                eprintln!("[server] could not export provider state for {thread_id}");
                None
            }
        }
    }

    fn notify_queue(&self, state: &ServerState, thread_id: &str) {
        let queue = {
            let live = lock(&self.live);
            let result = queue_result(&live, thread_id);
            ThreadQueuePush {
                thread_id: thread_id.into(),
                items: result.items,
                can_steer: result.can_steer,
            }
        };
        let _ = state.push.broadcast(channel::THREAD_QUEUE, queue);
    }

    fn drain_queue(&self, state: &Arc<ServerState>, thread_id: &str) {
        let selected = {
            let mut live = lock(&self.live);
            if live.active_turns.contains(thread_id)
                || live.starting_turns.contains(thread_id)
                || !live.draining.insert(thread_id.into())
            {
                return;
            }
            let entry = live.queues.get_mut(thread_id).and_then(VecDeque::pop_front);
            let session = live.sessions.get(thread_id).cloned();
            match (entry, session) {
                (Some(entry), Some(session)) => {
                    live.starting_turns.insert(thread_id.into());
                    Some((entry, session))
                }
                (entry, _) => {
                    if let Some(entry) = entry {
                        live.queues
                            .entry(thread_id.into())
                            .or_default()
                            .push_front(entry);
                    }
                    live.draining.remove(thread_id);
                    None
                }
            }
        };
        let Some((entry, session)) = selected else {
            return;
        };
        self.notify_queue(state, thread_id);
        let result = self.run_started_turn(
            state,
            thread_id,
            &entry.turn.text,
            &entry.turn.attachments,
            &entry.options,
            &session,
        );
        {
            let mut live = lock(&self.live);
            live.draining.remove(thread_id);
            if let Err(error) = &result {
                eprintln!("[agent] queued turn failed: {error}");
                live.queues
                    .entry(thread_id.into())
                    .or_default()
                    .push_front(entry);
            }
        }
        self.notify_queue(state, thread_id);
    }
}

fn session_handlers(
    state: &Arc<ServerState>,
    event_bridge: Arc<EventBridge>,
    provider: ProviderId,
    project_path: String,
) -> AgentHandlers {
    let oauth_state = Arc::downgrade(state);
    let oauth_project = project_path.clone();
    let mcp_state = Arc::downgrade(state);
    let mcp_project = project_path.clone();
    let skills_state = Arc::downgrade(state);
    AgentHandlers::new(
        move |event| event_bridge.emit(event),
        |line| eprintln!("[agent] {line}"),
    )
    .with_control_handlers(
        ControlHandlers::new(|_| {}, |_| {})
            .with_mcp_o_auth(move |event| {
                if let Some(state) = oauth_state.upgrade() {
                    let _ = state.push.broadcast(
                        channel::MCP_OAUTH,
                        McpOAuthPush {
                            provider,
                            project_path: oauth_project.clone(),
                            server_id: event.server_id,
                            login_id: event.login_id,
                            success: event.success,
                            error: event.error,
                        },
                    );
                }
            })
            .with_mcp_changed(move |_| {
                if let Some(state) = mcp_state.upgrade() {
                    broadcast_project_changed(&state, channel::MCP_CHANGED, provider, &mcp_project);
                }
            })
            .with_skills_changed(move || {
                if let Some(state) = skills_state.upgrade() {
                    broadcast_project_changed(
                        &state,
                        channel::SKILLS_CHANGED,
                        provider,
                        &project_path,
                    );
                }
            }),
    )
}

fn watch_project(
    projects: &Mutex<HashMap<ProviderId, HashSet<String>>>,
    provider: ProviderId,
    project_path: &str,
) {
    lock(projects)
        .entry(provider)
        .or_default()
        .insert(project_path.into());
}

fn broadcast_watched_projects(
    state: &ServerState,
    projects: &Mutex<HashMap<ProviderId, HashSet<String>>>,
    channel_name: &str,
    provider: ProviderId,
) {
    let projects = lock(projects).get(&provider).cloned().unwrap_or_default();
    for project_path in projects {
        broadcast_project_changed(state, channel_name, provider, &project_path);
    }
}

fn broadcast_project_changed(
    state: &ServerState,
    channel_name: &str,
    provider: ProviderId,
    project_path: &str,
) {
    let _ = state.push.broadcast(
        channel_name,
        serde_json::json!({ "provider": provider, "projectPath": project_path }),
    );
}

struct EventBridge {
    state: Weak<ServerState>,
    route: Mutex<EventRoute>,
}

#[derive(Default)]
struct EventRoute {
    thread_id: Option<String>,
    buffered: Vec<DomainEvent>,
}

impl EventBridge {
    fn new(state: Weak<ServerState>) -> Self {
        Self {
            state,
            route: Mutex::new(EventRoute::default()),
        }
    }

    fn emit(&self, event: DomainEvent) {
        let mut route = lock(&self.route);
        let Some(thread_id) = route.thread_id.clone() else {
            route.buffered.push(event);
            return;
        };
        drop(route);
        if let Some(state) = self.state.upgrade() {
            record_event(&state, &thread_id, event);
        }
    }

    fn attach(&self, thread_id: String) {
        let mut route = lock(&self.route);
        route.thread_id = Some(thread_id.clone());
        if let Some(state) = self.state.upgrade() {
            for event in route.buffered.drain(..) {
                record_event(&state, &thread_id, event);
            }
        } else {
            route.buffered.clear();
        }
    }
}

fn record_event(state: &Arc<ServerState>, thread_id: &str, event: DomainEvent) {
    let terminal = matches!(
        event,
        DomainEvent::TurnCompleted { .. } | DomainEvent::ThreadError { .. }
    );
    let provider_state = terminal
        .then(|| state.agents.export_provider_session_state(thread_id))
        .flatten();
    let lifecycle = {
        let mut store = lock(&state.store);
        let provider_state = provider_state.as_ref().map(AgentSessionState::value);
        let Ok(seq) = store.append_with_provider_state(thread_id, &event, provider_state) else {
            eprintln!("[server] could not persist agent event for {thread_id}");
            return;
        };
        let lifecycle = store.touch_thread(thread_id, terminal, None).ok();
        if state
            .push
            .broadcast(
                channel::THREAD_EVENT,
                ThreadEventPush {
                    thread_id: thread_id.into(),
                    event: event.clone(),
                    seq: Some(seq),
                },
            )
            .is_err()
        {
            eprintln!("[server] could not encode agent event for {thread_id}");
        }
        lifecycle
    };
    if let Some(lifecycle) = lifecycle {
        let _ = state.push.broadcast(
            channel::THREAD_LIFECYCLE,
            ThreadLifecyclePush {
                thread_id: thread_id.into(),
                lifecycle,
            },
        );
    }
    {
        let mut live = lock(&state.agents.live);
        match event {
            DomainEvent::TurnStarted { .. } => {
                live.active_turns.insert(thread_id.into());
            }
            DomainEvent::TurnCompleted { .. } | DomainEvent::ThreadError { .. } => {
                live.active_turns.remove(thread_id);
                if live.starting_turns.contains(thread_id) {
                    live.terminal_while_starting.insert(thread_id.into());
                }
            }
            _ => return,
        }
    }
    if terminal {
        schedule_drain(state, thread_id.into());
    }
}

fn schedule_drain(state: &Arc<ServerState>, thread_id: String) {
    let state = Arc::downgrade(state);
    let _ = thread::Builder::new()
        .name("harness-queue-drain".into())
        .spawn(move || {
            if let Some(state) = state.upgrade() {
                state.agents.drain_queue(&state, &thread_id);
            }
        });
}

fn queue_result(live: &LiveState, thread_id: &str) -> ThreadQueueResult {
    let items = live
        .queues
        .get(thread_id)
        .into_iter()
        .flatten()
        .map(|entry| entry.turn.clone())
        .collect();
    let can_steer = live
        .sessions
        .get(thread_id)
        .is_some_and(|session| session.capabilities().steer);
    ThreadQueueResult { items, can_steer }
}

fn cleanup_failed_worktree(worktree: Option<&Worktree>) {
    if let Some(worktree) = worktree {
        let _ = harness_workspace::remove_worktree(worktree, true);
    }
}

fn same_path(left: &str, right: &std::path::Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| PathBuf::from(left));
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    #[cfg(target_os = "windows")]
    return left
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy());
    #[cfg(not(target_os = "windows"))]
    {
        left == right
    }
}

fn path_inside(root: &std::path::Path, candidate: &str) -> bool {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let candidate = std::fs::canonicalize(candidate).unwrap_or_else(|_| PathBuf::from(candidate));
    #[cfg(target_os = "windows")]
    {
        let root = root.to_string_lossy().to_lowercase();
        let candidate = candidate.to_string_lossy().to_lowercase();
        return candidate == root
            || candidate
                .strip_prefix(&root)
                .and_then(|suffix| suffix.as_bytes().first())
                .is_some_and(|separator| matches!(separator, b'\\' | b'/'));
    }
    #[cfg(not(target_os = "windows"))]
    {
        candidate == root || candidate.strip_prefix(root).is_ok()
    }
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

fn timestamp_i64(timestamp: f64) -> i64 {
    if timestamp.is_finite() {
        timestamp.clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1_000.0
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
