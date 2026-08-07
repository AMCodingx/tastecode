use harness_protocol::{
    Account, ApprovalDecision, ApprovalMode, AuthStartLoginResult, Capabilities, DomainEvent,
    Model, Thread,
};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

type EventHandler = dyn Fn(DomainEvent) + Send + Sync;
type LogHandler = dyn Fn(String) + Send + Sync;
type LoginHandler = dyn Fn(LoginEvent) + Send + Sync;

/// Provider-neutral callbacks installed before an agent process is started.
/// Events can arrive during initialization, so attaching them afterwards has
/// an unavoidable race.
#[derive(Clone)]
pub struct AgentHandlers {
    event: Arc<EventHandler>,
    log: Arc<LogHandler>,
}

impl AgentHandlers {
    pub fn new(
        event: impl Fn(DomainEvent) + Send + Sync + 'static,
        log: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        Self {
            event: Arc::new(event),
            log: Arc::new(log),
        }
    }

    pub fn emit_event(&self, event: DomainEvent) {
        (self.event)(event);
    }

    pub fn emit_log(&self, line: impl Into<String>) {
        (self.log)(line.into());
    }
}

impl Default for AgentHandlers {
    fn default() -> Self {
        Self::new(|_| {}, |_| {})
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginEvent {
    pub login_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct ControlHandlers {
    login: Arc<LoginHandler>,
    log: Arc<LogHandler>,
}

impl ControlHandlers {
    pub fn new(
        login: impl Fn(LoginEvent) + Send + Sync + 'static,
        log: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        Self {
            login: Arc::new(login),
            log: Arc::new(log),
        }
    }

    pub fn emit_login(&self, event: LoginEvent) {
        (self.login)(event);
    }

    pub fn emit_log(&self, line: impl Into<String>) {
        (self.log)(line.into());
    }
}

impl Default for ControlHandlers {
    fn default() -> Self {
        Self::new(|_| {}, |_| {})
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartOptions {
    pub instructions: Option<String>,
    pub model: Option<String>,
    pub service_tier: Option<String>,
    pub effort: Option<String>,
    pub approval: Option<ApprovalMode>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnOptions {
    pub model: Option<String>,
    pub service_tier: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("{0}")]
    Failed(String),
    #[error("this agent does not support {0}")]
    Unsupported(&'static str),
}

pub type AgentResult<T> = Result<T, AgentError>;

/// One live provider session. Shared orchestration reads declared capabilities
/// and drives this interface; vendor wire details stay in adapter crates.
pub trait AgentSession: Send + Sync {
    fn capabilities(&self) -> Capabilities;

    fn send_turn(
        &self,
        thread_id: &str,
        text: &str,
        attachments: &[String],
        options: &TurnOptions,
    ) -> AgentResult<String>;

    fn steer(&self, _thread_id: &str, _text: &str, _attachments: &[String]) -> AgentResult<()> {
        Err(AgentError::Unsupported("steering"))
    }

    fn interrupt(&self, thread_id: &str) -> AgentResult<()>;

    fn respond_to_approval(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> AgentResult<bool>;

    fn respond_to_user_input(
        &self,
        _request_id: &str,
        _answers: &HashMap<String, Vec<String>>,
    ) -> AgentResult<bool> {
        Err(AgentError::Unsupported("structured input"))
    }

    fn dispose(&self);
}

/// Long-lived provider control channel for account and sign-in operations.
/// OAuth completion is delivered on the same process that started it, so this
/// object intentionally outlives any one request.
pub trait ProviderControl: Send + Sync {
    fn account(&self) -> AgentResult<Account>;
    fn start_login(&self) -> AgentResult<AuthStartLoginResult>;
    fn cancel_login(&self, login_id: &str) -> AgentResult<()>;
    fn use_api_key(&self, api_key: &str) -> AgentResult<Account>;
    fn sign_out(&self) -> AgentResult<()>;
    fn list_models(&self) -> AgentResult<Vec<Model>>;
    fn dispose(&self);
}

/// Provider construction is separate from session orchestration, making the
/// shared lifecycle testable without spawning vendor binaries.
pub trait AgentRuntime: Send + Sync {
    fn start(
        &self,
        workspace_path: &str,
        options: &StartOptions,
        handlers: AgentHandlers,
    ) -> AgentResult<(Thread, Arc<dyn AgentSession>)>;

    fn resume(
        &self,
        _thread_id: &str,
        _workspace_path: &str,
        _options: &StartOptions,
        _handlers: AgentHandlers,
    ) -> AgentResult<(Thread, Arc<dyn AgentSession>)> {
        Err(AgentError::Unsupported("session resume"))
    }

    fn list_models(&self) -> AgentResult<Vec<Model>>;

    fn open_control(&self, _handlers: ControlHandlers) -> AgentResult<Arc<dyn ProviderControl>> {
        Err(AgentError::Unsupported("account control"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_protocol::{Item, ItemStatus, ItemType};
    use std::sync::mpsc;

    #[test]
    fn handlers_are_cloneable_and_preserve_event_order() {
        let (sender, receiver) = mpsc::channel();
        let handlers = AgentHandlers::new(
            move |event| {
                let _ = sender.send(event);
            },
            |_| {},
        );
        let cloned = handlers.clone();
        handlers.emit_event(DomainEvent::ItemStarted {
            item: item("first"),
        });
        cloned.emit_event(DomainEvent::ItemCompleted {
            item: item("second"),
        });
        assert!(matches!(
            receiver.recv().unwrap(),
            DomainEvent::ItemStarted { item } if item.id == "first"
        ));
        assert!(matches!(
            receiver.recv().unwrap(),
            DomainEvent::ItemCompleted { item } if item.id == "second"
        ));
    }

    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            turn_id: "turn-1".into(),
            item_type: ItemType::Unknown,
            status: ItemStatus::Started,
            role: None,
            text: None,
            command: None,
            exit_code: None,
            duration_ms: None,
            path: None,
            lines_added: None,
            lines_removed: None,
            created_at: 0.0,
        }
    }
}
