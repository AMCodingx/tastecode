use crate::session::{ClaudeCodeSession, ClaudeCommand, ClaudeSessionState};
use harness_agent::{
    AgentError, AgentHandlers, AgentResult, AgentRuntime, AgentSession, StartOptions,
};
use harness_protocol::{Model, Thread};
use std::ffi::OsString;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct ClaudeLaunchOptions {
    /// Extra process environment for controlled launches and tests.
    pub environment: Vec<(OsString, OsString)>,
}

pub struct ClaudeCodeRuntime {
    command: ClaudeCommand,
}

impl ClaudeCodeRuntime {
    pub fn new(options: ClaudeLaunchOptions) -> Self {
        Self {
            command: ClaudeCommand {
                program: OsString::from("claude"),
                prefix_args: Vec::new(),
                environment: options.environment,
                append_turn_args: true,
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn with_command(command: ClaudeCommand) -> Self {
        Self { command }
    }
}

impl Default for ClaudeCodeRuntime {
    fn default() -> Self {
        Self::new(ClaudeLaunchOptions::default())
    }
}

impl AgentRuntime for ClaudeCodeRuntime {
    fn start(
        &self,
        workspace_path: &str,
        options: &StartOptions,
        handlers: AgentHandlers,
    ) -> AgentResult<(Thread, Arc<dyn AgentSession>)> {
        let (thread, session) =
            ClaudeCodeSession::start(workspace_path, options, self.command.clone(), handlers)?;
        Ok((thread, session))
    }

    fn resume(
        &self,
        thread_id: &str,
        workspace_path: &str,
        options: &StartOptions,
        handlers: AgentHandlers,
    ) -> AgentResult<(Thread, Arc<dyn AgentSession>)> {
        let saved = options
            .resume_state
            .as_ref()
            .ok_or_else(|| AgentError::Failed("Claude Code session state is unavailable".into()))?;
        let state = serde_json::from_value::<ClaudeSessionState>(saved.value().clone())
            .map_err(|_| AgentError::Failed("Claude Code session state is invalid".into()))?;
        if state.thread.id != thread_id || state.thread.workspace_path != workspace_path {
            return Err(AgentError::Failed(
                "Claude Code session state does not match the stored thread".into(),
            ));
        }
        let (thread, session) = ClaudeCodeSession::resume(state, self.command.clone(), handlers)?;
        Ok((thread, session))
    }

    fn list_models(&self) -> AgentResult<Vec<Model>> {
        Ok(claude_models())
    }
}

pub fn claude_models() -> Vec<Model> {
    vec![
        claude_alias("fable", "Fable 5", "Most capable — flagship tier", true),
        claude_alias("opus", "Opus 5", "Deep reasoning", false),
        claude_alias("sonnet", "Sonnet 5", "Balanced speed and capability", false),
        claude_alias("haiku", "Haiku 4.5", "Fastest and cheapest", false),
    ]
}

fn claude_alias(id: &str, name: &str, description: &str, is_default: bool) -> Model {
    Model {
        id: id.into(),
        display_name: name.into(),
        description: Some(description.into()),
        is_default,
        reasoning_efforts: Vec::new(),
        default_reasoning_effort: None,
        service_tiers: Vec::new(),
        default_service_tier: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn documented_aliases_have_one_default() {
        let models = claude_models();
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["fable", "opus", "sonnet", "haiku"]
        );
        assert_eq!(models.iter().filter(|model| model.is_default).count(), 1);
        assert_eq!(models[0].display_name, "Fable 5");
    }

    #[test]
    fn default_runtime_invokes_the_vendor_cli_directly() {
        let runtime = ClaudeCodeRuntime::default();
        assert_eq!(runtime.command.program, OsStr::new("claude"));
        assert!(runtime.command.prefix_args.is_empty());
        assert!(runtime.command.append_turn_args);
    }
}
