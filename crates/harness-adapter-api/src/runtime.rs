use crate::{
    ApiAgentSession, ApiSessionOptions, ApiTool, ApiToolError, ApiToolExecutor, create_transport,
    list_models,
};
use harness_agent::{
    AgentError, AgentHandlers, AgentResult, AgentRuntime, AgentSession, StartOptions,
};
use harness_protocol::{ApprovalMode, Model, ModelConnectionInput, Thread};
use std::sync::Arc;

pub struct ApiToolSet {
    pub definitions: Vec<ApiTool>,
    pub executor: Arc<dyn ApiToolExecutor>,
}

pub trait ApiToolFactory: Send + Sync {
    fn create(
        &self,
        workspace_path: &str,
        approval: ApprovalMode,
    ) -> Result<ApiToolSet, ApiToolError>;
}

pub struct ApiRuntime {
    connection: ModelConnectionInput,
    api_key: String,
    tools: Arc<dyn ApiToolFactory>,
}

impl ApiRuntime {
    pub fn new(
        connection: ModelConnectionInput,
        api_key: String,
        tools: Arc<dyn ApiToolFactory>,
    ) -> Self {
        Self {
            connection,
            api_key,
            tools,
        }
    }
}

impl AgentRuntime for ApiRuntime {
    fn start(
        &self,
        workspace_path: &str,
        options: &StartOptions,
        handlers: AgentHandlers,
    ) -> AgentResult<(Thread, Arc<dyn AgentSession>)> {
        if !self.connection.enabled {
            return Err(AgentError::Failed(format!(
                "model connection \"{}\" is disabled",
                self.connection.id
            )));
        }
        let model = options
            .model
            .as_deref()
            .or(self.connection.default_model.as_deref())
            .filter(|model| !model.is_empty())
            .ok_or_else(|| {
                AgentError::Failed(format!(
                    "choose a model for \"{}\"",
                    self.connection.display_name
                ))
            })?;
        let transport = create_transport(&self.connection, self.api_key.clone())
            .map_err(|error| AgentError::Failed(error.to_string()))?;
        let tools = self
            .tools
            .create(
                workspace_path,
                options.approval.unwrap_or(ApprovalMode::Ask),
            )
            .map_err(|error| AgentError::Failed(error.to_string()))?;
        let mut session_options = ApiSessionOptions::new(model, transport);
        session_options.tools = tools.definitions;
        session_options.executor = tools.executor;
        session_options.secrets = vec![self.api_key.clone()];
        session_options.instructions = options.instructions.clone();
        let (thread, session) = ApiAgentSession::start(
            workspace_path,
            &self.connection.id,
            session_options,
            handlers,
        )?;
        Ok((thread, session))
    }

    fn list_models(&self) -> AgentResult<Vec<Model>> {
        list_models(&self.connection, &self.api_key)
            .map_err(|error| AgentError::Failed(error.to_string()))
    }
}
