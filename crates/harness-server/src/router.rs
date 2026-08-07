use crate::ServerState;
use chrono::{Datelike as _, Local, TimeZone as _};
use harness_protocol::method;
use harness_protocol::{
    CheckpointSummary, ErrorCode, ProjectAddedResult, ProjectSummary, ProjectsListResult,
    ProviderId, ServerWelcome, SessionSummary, SettleReason, SidebarMode, SystemInfo,
    SystemPlatform, ThreadCheckpointsResult, ThreadHistoryResult, ThreadInboxStatus,
    ThreadLifecycle, ThreadLifecyclePush, ThreadLifecycleResult, ThreadUnsavedWorkResult, Usage,
    UsageSummaryResult, WireError, channel,
};
use harness_store::{SearchOptions, SidebarSettingsUpdate, Store, StoreError};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SERVER_VERSION: &str = "0.0.0";

pub(crate) struct RouteError(pub(crate) WireError);

impl RouteError {
    fn bad_params(method: &str, detail: impl Into<String>) -> Self {
        Self(WireError {
            code: ErrorCode::BadRequest,
            message: format!("invalid params for {method}"),
            detail: Some(detail.into()),
        })
    }

    pub(crate) fn unknown_method(method: &str) -> Self {
        Self(WireError {
            code: ErrorCode::BadRequest,
            message: format!("unknown method: {method}"),
            detail: None,
        })
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self(WireError {
            code: ErrorCode::Internal,
            message: error.to_string(),
            detail: None,
        })
    }
}

impl From<StoreError> for RouteError {
    fn from(error: StoreError) -> Self {
        Self::internal(error)
    }
}

pub(crate) fn route(
    state: &ServerState,
    _connection_id: u64,
    method_name: &str,
    params: Value,
) -> Result<Value, RouteError> {
    match method_name {
        method::CLIENT_CAPABILITIES => {
            let _: ClientCapabilitiesParams = decode(method_name, params)?;
            empty_result()
        }
        method::SYSTEM_INFO => {
            let _: EmptyParams = decode(method_name, params)?;
            encoded(SystemInfo {
                server_version: SERVER_VERSION.into(),
                protocol_version: harness_protocol::PROTOCOL_VERSION,
                platform: current_platform(),
            })
        }
        method::SEARCH_SESSIONS => {
            let params: SearchParams = decode(method_name, params)?;
            let query = params.query.trim().to_owned();
            if query.is_empty()
                || params.project_path.as_deref().is_some_and(str::is_empty)
                || params.cursor.as_deref().is_some_and(str::is_empty)
                || params
                    .limit
                    .is_some_and(|limit| !(1..=100).contains(&limit))
            {
                return Err(RouteError::bad_params(
                    method_name,
                    "query, filters, cursor, or limit is outside the protocol bounds",
                ));
            }
            encoded(lock_store(state)?.search_sessions(&SearchOptions {
                query,
                project_path: params.project_path,
                provider: params.provider,
                cursor: params.cursor,
                limit: params.limit,
            })?)
        }
        method::WORKSPACE_INFO => {
            let params: WorkspacePathParams = decode(method_name, params)?;
            encoded(harness_workspace::read_workspace(params.path))
        }
        method::WORKSPACE_BRANCHES => {
            let params: WorkspacePathParams = decode(method_name, params)?;
            Ok(json!({
                "branches": harness_workspace::list_workspace_branches(params.path)
            }))
        }
        method::WORKSPACE_SWITCH_BRANCH => {
            let params: WorkspaceSwitchParams = decode(method_name, params)?;
            require_non_empty(method_name, "branch", &params.branch)?;
            encoded(
                harness_workspace::switch_workspace_branch(params.path, &params.branch)
                    .map_err(RouteError::internal)?,
            )
        }
        method::PROJECTS_LIST => {
            let _: EmptyParams = decode(method_name, params)?;
            let store = lock_store(state)?;
            let projects = store
                .projects()?
                .into_iter()
                .map(|project| {
                    let sessions = store
                        .threads(Some(&project.path))?
                        .into_iter()
                        .map(|thread| {
                            let status = inbox_status(state, &store, &thread.id, thread.unread)?;
                            Ok(SessionSummary {
                                id: thread.id,
                                title: thread.title,
                                provider: thread.provider,
                                agent: thread.agent,
                                created_at: thread.created_at as f64,
                                running: false,
                                pinned: thread.pinned,
                                status: Some(status),
                                unread: Some(thread.unread),
                                lifecycle: Some(thread.lifecycle),
                                closed_at: thread.closed_at.map(|value| value as f64),
                                worktree_branch: thread.worktree_branch,
                            })
                        })
                        .collect::<Result<Vec<_>, RouteError>>()?;
                    Ok(ProjectSummary {
                        path: project.path,
                        name: project.name,
                        pinned: project.pinned,
                        created_at: project.created_at as f64,
                        sessions,
                    })
                })
                .collect::<Result<Vec<_>, RouteError>>()?;
            encoded(ProjectsListResult { projects })
        }
        method::PROJECTS_ADD => {
            let params: ProjectAddParams = decode(method_name, params)?;
            let project = lock_store(state)?.add_project(&params.path, params.name.as_deref())?;
            encoded(ProjectAddedResult {
                path: project.path,
                name: project.name,
                pinned: project.pinned,
                created_at: project.created_at as f64,
            })
        }
        method::PROJECTS_PIN => {
            let params: ProjectPinParams = decode(method_name, params)?;
            lock_store(state)?.set_project_pinned(&params.path, params.pinned)?;
            empty_result()
        }
        method::PROJECTS_RENAME => {
            let params: ProjectRenameParams = decode(method_name, params)?;
            lock_store(state)?.rename_project(&params.path, &params.name)?;
            empty_result()
        }
        method::PROJECTS_REMOVE => {
            let params: ProjectPathParams = decode(method_name, params)?;
            let store = lock_store(state)?;
            let isolated = store
                .threads(Some(&params.path))?
                .into_iter()
                .filter(|thread| thread.worktree_path.is_some())
                .count();
            if isolated > 0 {
                return Err(RouteError::internal(format!(
                    "{isolated} isolated session{} in this project still own a private checkout. Discard or keep those first.",
                    if isolated == 1 { "" } else { "s" }
                )));
            }
            store.remove_project(&params.path)?;
            empty_result()
        }
        method::THREAD_RENAME => {
            let params: ThreadRenameParams = decode(method_name, params)?;
            lock_store(state)?.rename_thread(&params.thread_id, &params.title)?;
            empty_result()
        }
        method::THREAD_PIN => {
            let params: ThreadPinParams = decode(method_name, params)?;
            require_non_empty(method_name, "threadId", &params.thread_id)?;
            lock_store(state)?.set_thread_pinned(&params.thread_id, params.pinned)?;
            empty_result()
        }
        method::THREAD_SETTLE => {
            let params: ThreadIdParams = decode(method_name, params)?;
            require_non_empty(method_name, "threadId", &params.thread_id)?;
            let store = lock_store(state)?;
            assert_lifecycle(&store, &params.thread_id, LifecycleState::Active)?;
            assert_can_hide(state, &store, &params.thread_id)?;
            let lifecycle = store.settle_thread(&params.thread_id, SettleReason::Manual, None)?;
            broadcast_lifecycle(state, &params.thread_id, &lifecycle)?;
            encoded(ThreadLifecycleResult { lifecycle })
        }
        method::THREAD_UNSETTLE => {
            let params: ThreadIdParams = decode(method_name, params)?;
            require_non_empty(method_name, "threadId", &params.thread_id)?;
            let store = lock_store(state)?;
            assert_lifecycle(&store, &params.thread_id, LifecycleState::Settled)?;
            let lifecycle = store.activate_thread(&params.thread_id, None)?;
            broadcast_lifecycle(state, &params.thread_id, &lifecycle)?;
            encoded(ThreadLifecycleResult { lifecycle })
        }
        method::THREAD_SNOOZE => {
            let params: ThreadSnoozeParams = decode(method_name, params)?;
            require_non_empty(method_name, "threadId", &params.thread_id)?;
            let wake_at = i64::try_from(params.wake_at)
                .map_err(|_| RouteError::bad_params(method_name, "wakeAt is too large"))?;
            if wake_at <= now_ms().map_err(RouteError::internal)? {
                return Err(RouteError::internal("wake time must be in the future"));
            }
            let store = lock_store(state)?;
            assert_lifecycle(&store, &params.thread_id, LifecycleState::Active)?;
            assert_can_hide(state, &store, &params.thread_id)?;
            let lifecycle = store.snooze_thread(&params.thread_id, wake_at, None)?;
            broadcast_lifecycle(state, &params.thread_id, &lifecycle)?;
            encoded(ThreadLifecycleResult { lifecycle })
        }
        method::THREAD_UNSNOOZE => {
            let params: ThreadIdParams = decode(method_name, params)?;
            require_non_empty(method_name, "threadId", &params.thread_id)?;
            let store = lock_store(state)?;
            assert_lifecycle(&store, &params.thread_id, LifecycleState::Snoozed)?;
            let lifecycle = store.activate_thread(&params.thread_id, None)?;
            broadcast_lifecycle(state, &params.thread_id, &lifecycle)?;
            encoded(ThreadLifecycleResult { lifecycle })
        }
        method::THREAD_SET_KEEP_ACTIVE => {
            let params: ThreadKeepActiveParams = decode(method_name, params)?;
            require_non_empty(method_name, "threadId", &params.thread_id)?;
            let store = lock_store(state)?;
            assert_lifecycle(&store, &params.thread_id, LifecycleState::Active)?;
            let lifecycle =
                store.set_thread_keep_active(&params.thread_id, params.keep_active, None)?;
            broadcast_lifecycle(state, &params.thread_id, &lifecycle)?;
            encoded(ThreadLifecycleResult { lifecycle })
        }
        method::THREAD_DELETE => {
            let params: ThreadIdParams = decode(method_name, params)?;
            lock_store(state)?.delete_thread(&params.thread_id)?;
            state
                .inbox
                .lock()
                .map_err(|_| RouteError::internal("inbox projection mutex poisoned"))?
                .remove(&params.thread_id);
            empty_result()
        }
        method::THREAD_HISTORY => {
            let params: ThreadHistoryParams = decode(method_name, params)?;
            let after_seq = params.after_seq.unwrap_or(0.0).max(0.0).floor() as u64;
            let store = lock_store(state)?;
            let result = ThreadHistoryResult {
                events: store.history(&params.thread_id, after_seq)?,
                running: false,
            };
            store.mark_thread_read(&params.thread_id)?;
            encoded(result)
        }
        method::THREAD_CLOSE => {
            let params: ThreadIdParams = decode(method_name, params)?;
            lock_store(state)?.close_thread(&params.thread_id)?;
            empty_result()
        }
        method::THREAD_CHECKPOINTS => {
            let params: ThreadIdParams = decode(method_name, params)?;
            require_non_empty(method_name, "threadId", &params.thread_id)?;
            encoded(ThreadCheckpointsResult {
                checkpoints: lock_store(state)?
                    .checkpoints(&params.thread_id)?
                    .into_iter()
                    .map(|checkpoint| CheckpointSummary {
                        id: checkpoint.id,
                        seq: checkpoint.seq,
                        label: checkpoint.label,
                        created_at: checkpoint.created_at as f64,
                    })
                    .collect(),
            })
        }
        method::THREAD_UNSAVED_WORK => {
            let params: ThreadIdParams = decode(method_name, params)?;
            let stored = lock_store(state)?.thread(&params.thread_id)?;
            let worktree_path = stored.and_then(|thread| thread.worktree_path);
            encoded(ThreadUnsavedWorkResult {
                isolated: worktree_path.is_some(),
                uncommitted: worktree_path
                    .as_deref()
                    .is_some_and(harness_workspace::has_uncommitted_changes),
            })
        }
        method::THREAD_DISCARD_WORKTREE => {
            let params: ThreadDiscardWorktreeParams = decode(method_name, params)?;
            let stored = lock_store(state)?.thread(&params.thread_id)?;
            let Some(stored) = stored else {
                return empty_result();
            };
            let (Some(path), Some(branch)) = (stored.worktree_path, stored.worktree_branch) else {
                return empty_result();
            };
            harness_workspace::remove_worktree(
                &harness_workspace::Worktree {
                    path: path.into(),
                    branch,
                    repo_path: stored.project_path.into(),
                },
                params.force.unwrap_or(false),
            )
            .map_err(RouteError::internal)?;
            lock_store(state)?.forget_worktree(&params.thread_id)?;
            empty_result()
        }
        method::SIDEBAR_SETTINGS => {
            let _: EmptyParams = decode(method_name, params)?;
            encoded(lock_store(state)?.sidebar_settings()?)
        }
        method::SIDEBAR_UPDATE_SETTINGS => {
            let params: SidebarUpdateParams = decode(method_name, params)?;
            if params
                .auto_settle_days
                .flatten()
                .is_some_and(|days| !(1..=90).contains(&days))
            {
                return Err(RouteError::bad_params(
                    method_name,
                    "autoSettleDays must be null or between 1 and 90",
                ));
            }
            let settings = lock_store(state)?.update_sidebar_settings(SidebarSettingsUpdate {
                mode: params.mode,
                auto_settle_days: params.auto_settle_days,
            })?;
            state
                .push
                .broadcast(channel::SIDEBAR_SETTINGS, &settings)
                .map_err(RouteError::internal)?;
            encoded(settings)
        }
        method::USAGE_SUMMARY => {
            let params: UsageParams = decode(method_name, params)?;
            let store = lock_store(state)?;
            let summary = match params {
                UsageParams::Thread(UsageThreadParams { thread_id }) => {
                    if store.thread(&thread_id)?.is_none() {
                        return Err(RouteError::internal("thread not found"));
                    }
                    store.usage_summary(&thread_id, start_of_today_ms())?
                }
                UsageParams::Provider(UsageProviderParams { provider }) => {
                    let _provider = provider;
                    harness_store::UsageSummary {
                        session: empty_usage(),
                        today: empty_usage(),
                    }
                }
            };
            encoded(UsageSummaryResult {
                session: summary.session,
                today: summary.today,
                limits: Vec::new(),
            })
        }
        _ => Err(RouteError::unknown_method(method_name)),
    }
}

pub(crate) fn welcome() -> ServerWelcome {
    ServerWelcome {
        server_version: SERVER_VERSION.into(),
        protocol_version: harness_protocol::PROTOCOL_VERSION,
    }
}

fn decode<T: DeserializeOwned>(method: &str, params: Value) -> Result<T, RouteError> {
    serde_json::from_value(params)
        .map_err(|error| RouteError::bad_params(method, error.to_string()))
}

fn encoded<T: serde::Serialize>(value: T) -> Result<Value, RouteError> {
    serde_json::to_value(value).map_err(RouteError::internal)
}

fn empty_result() -> Result<Value, RouteError> {
    Ok(json!({}))
}

fn lock_store(state: &ServerState) -> Result<MutexGuard<'_, Store>, RouteError> {
    state
        .store
        .lock()
        .map_err(|_| RouteError::internal("store mutex poisoned"))
}

fn inbox_status(
    state: &ServerState,
    store: &Store,
    thread_id: &str,
    unread: bool,
) -> Result<ThreadInboxStatus, RouteError> {
    state
        .inbox
        .lock()
        .map_err(|_| RouteError::internal("inbox projection mutex poisoned"))?
        .status(store, thread_id, unread)
        .map_err(Into::into)
}

fn require_non_empty(method: &str, field: &str, value: &str) -> Result<(), RouteError> {
    if value.is_empty() {
        return Err(RouteError::bad_params(
            method,
            format!("{field}: expected a non-empty string"),
        ));
    }
    Ok(())
}

fn broadcast_lifecycle(
    state: &ServerState,
    thread_id: &str,
    lifecycle: &ThreadLifecycle,
) -> Result<(), RouteError> {
    state
        .push
        .broadcast(
            channel::THREAD_LIFECYCLE,
            ThreadLifecyclePush {
                thread_id: thread_id.into(),
                lifecycle: lifecycle.clone(),
            },
        )
        .map_err(RouteError::internal)
}

#[derive(Clone, Copy)]
enum LifecycleState {
    Active,
    Settled,
    Snoozed,
}

impl LifecycleState {
    fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Settled => "settled",
            Self::Snoozed => "snoozed",
        }
    }
}

fn assert_lifecycle(
    store: &Store,
    thread_id: &str,
    expected: LifecycleState,
) -> Result<(), RouteError> {
    let thread = store
        .thread(thread_id)?
        .ok_or_else(|| RouteError::internal("thread not found"))?;
    if thread.closed_at.is_some() {
        return Err(RouteError::internal(
            "archived threads cannot change inbox shelf",
        ));
    }
    let actual = match thread.lifecycle {
        ThreadLifecycle::Active { .. } => LifecycleState::Active,
        ThreadLifecycle::Settled { .. } => LifecycleState::Settled,
        ThreadLifecycle::Snoozed { .. } => LifecycleState::Snoozed,
    };
    if actual.name() != expected.name() {
        return Err(RouteError::internal(format!(
            "thread is {}, expected {}",
            actual.name(),
            expected.name()
        )));
    }
    Ok(())
}

fn assert_can_hide(state: &ServerState, store: &Store, thread_id: &str) -> Result<(), RouteError> {
    let thread = store
        .thread(thread_id)?
        .ok_or_else(|| RouteError::internal("thread not found"))?;
    let status = inbox_status(state, store, thread_id, thread.unread)?;
    if matches!(
        status,
        ThreadInboxStatus::Starting
            | ThreadInboxStatus::Working
            | ThreadInboxStatus::Queued
            | ThreadInboxStatus::Approval
            | ThreadInboxStatus::Input
    ) {
        let status = serde_json::to_value(status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "active".into());
        return Err(RouteError::internal(format!(
            "cannot hide a thread while its status is {status}"
        )));
    }
    Ok(())
}

fn now_ms() -> Result<i64, &'static str> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch")?;
    i64::try_from(duration.as_millis()).map_err(|_| "system clock is outside the supported range")
}

fn start_of_today_ms() -> i64 {
    let now = Local::now();
    Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .earliest()
        .map(|value| value.timestamp_millis())
        .unwrap_or_else(|| now.timestamp_millis())
}

fn empty_usage() -> Usage {
    Usage {
        input_tokens: 0.0,
        cached_input_tokens: 0.0,
        output_tokens: 0.0,
        reasoning_tokens: 0.0,
        total_tokens: 0.0,
        cost_usd: None,
        context_window: None,
    }
}

fn current_platform() -> SystemPlatform {
    #[cfg(target_os = "windows")]
    return SystemPlatform::Windows;
    #[cfg(target_os = "macos")]
    return SystemPlatform::MacOs;
    #[cfg(target_os = "linux")]
    return SystemPlatform::Linux;
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientCapabilitiesParams {
    #[allow(dead_code)]
    preview_capture: bool,
}

#[derive(Deserialize)]
struct EmptyParams {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchParams {
    query: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    project_path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    provider: Option<ProviderId>,
    #[serde(default, deserialize_with = "deserialize_present")]
    cursor: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present")]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ProjectAddParams {
    path: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    name: Option<String>,
}

#[derive(Deserialize)]
struct WorkspacePathParams {
    path: String,
}

#[derive(Deserialize)]
struct WorkspaceSwitchParams {
    path: String,
    branch: String,
}

#[derive(Deserialize)]
struct ProjectPinParams {
    path: String,
    pinned: bool,
}

#[derive(Deserialize)]
struct ProjectRenameParams {
    path: String,
    name: String,
}

#[derive(Deserialize)]
struct ProjectPathParams {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadRenameParams {
    thread_id: String,
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadPinParams {
    thread_id: String,
    pinned: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadIdParams {
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSnoozeParams {
    thread_id: String,
    wake_at: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadKeepActiveParams {
    thread_id: String,
    keep_active: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadDiscardWorktreeParams {
    thread_id: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    force: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadHistoryParams {
    thread_id: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    after_seq: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SidebarUpdateParams {
    #[serde(default, deserialize_with = "deserialize_present")]
    mode: Option<SidebarMode>,
    #[serde(default, deserialize_with = "deserialize_present_optional")]
    auto_settle_days: Option<Option<u8>>,
}

fn deserialize_present_optional<'de, D>(deserializer: D) -> Result<Option<Option<u8>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u8>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum UsageParams {
    Thread(UsageThreadParams),
    Provider(UsageProviderParams),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageThreadParams {
    thread_id: String,
}

#[derive(Deserialize)]
struct UsageProviderParams {
    provider: ProviderId,
}

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_sidebar_updates_distinguish_missing_from_null() {
        let missing: SidebarUpdateParams = serde_json::from_value(json!({})).unwrap();
        let null: SidebarUpdateParams =
            serde_json::from_value(json!({ "autoSettleDays": null })).unwrap();
        let days: SidebarUpdateParams =
            serde_json::from_value(json!({ "autoSettleDays": 14 })).unwrap();
        assert_eq!(missing.auto_settle_days, None);
        assert_eq!(null.auto_settle_days, Some(None));
        assert_eq!(days.auto_settle_days, Some(Some(14)));
        assert!(serde_json::from_value::<SidebarUpdateParams>(json!({ "mode": null })).is_err());
    }

    #[test]
    fn optional_protocol_fields_reject_null_like_the_zod_contract() {
        assert!(
            serde_json::from_value::<SearchParams>(json!({ "query": "result", "limit": null }))
                .is_err()
        );
        assert!(
            serde_json::from_value::<ProjectAddParams>(json!({ "path": "/repo", "name": null }))
                .is_err()
        );
        assert!(
            serde_json::from_value::<ThreadHistoryParams>(
                json!({ "threadId": "thread", "afterSeq": null })
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<ThreadDiscardWorktreeParams>(
                json!({ "threadId": "thread", "force": null })
            )
            .is_err()
        );
    }

    #[test]
    fn usage_union_keeps_the_thread_branch_first() {
        let params: UsageParams = serde_json::from_value(json!({
            "threadId": "thread",
            "provider": "codex"
        }))
        .unwrap();
        assert!(matches!(params, UsageParams::Thread(_)));

        let params: UsageParams =
            serde_json::from_value(json!({ "threadId": null, "provider": "codex" })).unwrap();
        assert!(matches!(params, UsageParams::Provider(_)));
    }
}
