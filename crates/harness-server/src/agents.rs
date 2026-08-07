use crate::ServerState;
use harness_adapter_codex::CodexRuntime;
use harness_agent::{
    AgentError, AgentHandlers, AgentRuntime, AgentSession, StartOptions, TurnOptions,
};
use harness_protocol::{
    ApprovalDecision, DomainEvent, Model, ProviderId, QueuedTurn, SendTurnResult, Thread,
    ThreadEventPush, ThreadInboxStatus, ThreadLifecyclePush, ThreadQueuePush, ThreadQueueResult,
    channel,
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
}

impl Default for NativeRuntimes {
    fn default() -> Self {
        Self {
            codex: Arc::new(CodexRuntime::default()),
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

#[derive(Default)]
struct LiveState {
    sessions: HashMap<String, Arc<dyn AgentSession>>,
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

pub(crate) struct AgentManager {
    runtimes: Arc<dyn RuntimeRegistry>,
    live: Mutex<LiveState>,
    resumes: Mutex<HashMap<String, Arc<ResumeSlot>>>,
    worktree_root: PathBuf,
}

impl AgentManager {
    pub(crate) fn new(runtimes: Arc<dyn RuntimeRegistry>) -> Self {
        Self {
            runtimes,
            live: Mutex::new(LiveState::default()),
            resumes: Mutex::new(HashMap::new()),
            worktree_root: std::env::temp_dir().join("personal-harness-trees"),
        }
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

    pub(crate) fn start_thread(
        &self,
        state: &Arc<ServerState>,
        request: StartThreadRequest,
    ) -> Result<Thread, String> {
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
        let handlers = AgentHandlers::new(
            move |event| event_bridge.emit(event),
            |line| eprintln!("[agent] {line}"),
        );
        let options = StartOptions {
            instructions: Some(REPLY_STYLE_INSTRUCTIONS.into()),
            model: request.model,
            service_tier: request.service_tier,
            effort: request.effort,
            approval: request.approval,
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
        let stored = {
            let store = lock(&state.store);
            store
                .add_project(&request.workspace_path, None)
                .and_then(|_| {
                    store.add_thread(NewThread {
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
                    })
                })
        };
        if let Err(error) = stored {
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
            live.sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session in sessions {
            session.dispose();
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

    fn resume_session(
        &self,
        state: &Arc<ServerState>,
        thread_id: &str,
    ) -> Result<Arc<dyn AgentSession>, String> {
        let stored = lock(&state.store)
            .thread(thread_id)
            .map_err(|error| error.to_string())?
            .filter(|thread| thread.closed_at.is_none())
            .ok_or_else(|| format!("no such thread: {thread_id}"))?;
        let runtime = self
            .runtimes
            .runtime(stored.provider, stored.agent.as_deref(), None)
            .map_err(|error| error.to_string())?;
        let bridge = Arc::new(EventBridge::new(Arc::downgrade(state)));
        let event_bridge = Arc::clone(&bridge);
        let handlers = AgentHandlers::new(
            move |event| event_bridge.emit(event),
            |line| eprintln!("[agent] {line}"),
        );
        let workspace = stored
            .worktree_path
            .as_deref()
            .unwrap_or(&stored.project_path);
        let (thread, session) = runtime
            .resume(thread_id, workspace, &StartOptions::default(), handlers)
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
        let previous = lock(&self.live).sessions.insert(thread_id.into(), session);
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
    let lifecycle = {
        let mut store = lock(&state.store);
        let Ok(seq) = store.append(thread_id, &event) else {
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
