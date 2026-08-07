mod command_palette;
mod image_viewer;
mod onboarding;
mod provider_terminal;
mod session_search;
mod settings;
mod sidebar_controls;
mod stage_controls;
mod zoom_hud;

use crate::assets::{HarnessAssets, register_fonts};
use crate::chat::{ChatEvent, ChatView, ComposerSettings, SessionContext};
use crate::client_state::{
    ChatUpdate, ClientState, ClientUpdate, NewThreadRequest, ReviewHunkRequest, SendTurnRequest,
    ShellEvent,
};
use crate::model_selection::{
    fast_mode_off_value, fast_service_tier, is_fast_mode_enabled, next_service_tier, source_key,
};
use crate::preferences::{NativePreferences, SourceSelection, ThemePreference};
use crate::preview_capture::PreviewCaptureRuntime;
use crate::sidebar::{
    SelectionModifiers, SidebarActions, SidebarMenuRequest, SidebarProps, ordered_inbox_ids,
    sidebar,
};
use crate::theme::{TITLEBAR_HEIGHT, Theme, ThemeMode};
use crate::zoom::{self, px};
use anyhow::Result;
use command_palette::{CommandPaletteState, CommandScope};
use gpui::{
    Animation, AnimationExt, App, Application, Bounds, Context, Entity, FocusHandle, Focusable,
    FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, PathPromptOptions,
    Pixels, Render, TitlebarOptions, Window, WindowAppearance, WindowBackgroundAppearance,
    WindowBounds, WindowOptions, div, point, prelude::*, size, svg,
};
use gpui_component::Root;
use gpui_component::input::{InputEvent, InputState};
use harness_client::Endpoint;
use harness_protocol::{
    ApprovalMode, Model, ModelConnectionPreset, ProviderId, SidebarMode, ThreadInboxStatus,
};
use provider_terminal::{ProviderTerminalKey, ProviderTerminalView};
use session_search::SessionSearchState;
use sidebar_controls::SidebarControlsState;
use stage_controls::StageControlsState;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const APP_WIDTH: f32 = 1180.0;
const APP_HEIGHT: f32 = 820.0;
const DESIGN_BRIEF_ATTACHMENT: &str = "personal-harness://design-brief-v1";
const MIN_RAIL_PREVIEW_WIDTH: f32 = 148.0;
const COLLAPSE_RAIL_WIDTH: f32 = 176.0;
const MAX_RAIL_WIDTH: f32 = 420.0;

#[derive(Clone, Copy)]
struct SidebarResizeDrag {
    start_x: Pixels,
    start_width: f32,
}

pub fn run() -> Result<()> {
    run_with_endpoint(Endpoint::from_environment()?)
}

pub fn run_with_endpoint(endpoint: Endpoint) -> Result<()> {
    Application::new()
        .with_assets(HarnessAssets)
        .run(move |cx: &mut App| {
            zoom::set_factor(1.0);
            gpui_component::init(cx);
            register_fonts(cx).expect("failed to register bundled Geist fonts");

            let bounds = Bounds::centered(None, size(px(APP_WIDTH), px(APP_HEIGHT)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(720.0), px(520.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Personal Harness".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.0), px(11.0))),
                }),
                window_background: WindowBackgroundAppearance::Opaque,
                ..Default::default()
            };

            cx.open_window(options, move |window, cx| {
                window.set_window_title("Personal Harness");
                let app = cx.new(|cx| HarnessApp::new(endpoint, window, cx));
                cx.new(|cx| Root::new(app, window, cx))
            })
            .expect("failed to open the Harness window");
            cx.activate(true);
        });
    Ok(())
}

struct HarnessApp {
    theme: Theme,
    sidebar_collapsed: bool,
    sidebar_transition: u64,
    sidebar_width: f32,
    sidebar_resize_drag: Option<SidebarResizeDrag>,
    sidebar_resize_focus: FocusHandle,
    state: ClientState,
    chat: Entity<ChatView>,
    selected_thread_id: Option<String>,
    active_project_path: Option<String>,
    chat_visible: bool,
    selected_model_key: Option<String>,
    effort: Option<String>,
    service_tier: Option<String>,
    approval: ApprovalMode,
    isolate_session: bool,
    design_mode: bool,
    sidebar_scope: Option<String>,
    scope_open: bool,
    new_thread_picker: bool,
    pending_new_chat_path: Option<String>,
    settings_open: bool,
    settings_return_to_chat: bool,
    settings_section: settings::SettingsSection,
    settings_transition: u64,
    settings_open_transition: u64,
    settings_focus: FocusHandle,
    settings_focus_pending: bool,
    command_palette: CommandPaletteState,
    focus_composer_pending: bool,
    session_search: SessionSearchState,
    pending_reveal_turn: Option<(String, String)>,
    sidebar_controls: SidebarControlsState,
    stage_controls: StageControlsState,
    collapsed_projects: HashSet<String>,
    expanded_project_sessions: HashSet<String>,
    sidebar_search: Entity<InputState>,
    sidebar_status_clocks: HashMap<String, (ThreadInboxStatus, f64)>,
    snoozed_expanded: bool,
    settled_expanded: bool,
    settled_limit: usize,
    account_menu_open: bool,
    onboarding: Option<onboarding::OnboardingState>,
    onboarding_api_key: Entity<InputState>,
    system_theme_mode: ThemeMode,
    reduced_motion: bool,
    preferences: NativePreferences,
    connection_editor_open: bool,
    connection_submission_id: Option<String>,
    connection_preset: ModelConnectionPreset,
    connection_name: Entity<InputState>,
    connection_base_url: Entity<InputState>,
    connection_default_model: Entity<InputState>,
    connection_api_key: Entity<InputState>,
    mcp_editor: Option<settings::McpEditorState>,
    mcp_editor_submission_id: Option<String>,
    mcp_expanded_servers: HashSet<(ProviderId, String, String)>,
    mcp_editor_id: Entity<InputState>,
    mcp_editor_name: Entity<InputState>,
    mcp_editor_transport: Entity<InputState>,
    auto_settle_days_input: Entity<InputState>,
    provider_terminals: HashMap<ProviderTerminalKey, Entity<ProviderTerminalView>>,
    provider_terminal_ids: HashMap<String, ProviderTerminalKey>,
    copied_provider_code: Option<String>,
    preview_capture: Option<PreviewCaptureRuntime>,
    image_viewer: Option<image_viewer::ImageViewerState>,
    app_zoom: zoom_hud::AppZoomState,
    fixture: bool,
}

impl HarnessApp {
    fn new(endpoint: Endpoint, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fixture = std::env::var_os("HARNESS_NATIVE_FIXTURE").is_some();
        let mut state = ClientState::new(fixture);
        let preview_capture = PreviewCaptureRuntime::discover();
        state.set_preview_capture_available(preview_capture.is_some());
        let preferences = match NativePreferences::load() {
            Ok(preferences) => preferences,
            Err(error) => {
                state.notice = Some(format!("Could not load native preferences: {error}"));
                NativePreferences::default()
            }
        };
        let sidebar_width = f32::from(preferences.rail_width);
        let system_theme_mode = theme_mode_for_appearance(window.appearance());
        let mode = match preferences.theme {
            ThemePreference::System => system_theme_mode,
            ThemePreference::Light => ThemeMode::Light,
            ThemePreference::Dark => ThemeMode::Dark,
        };
        let reduced_motion = crate::accessibility::prefers_reduced_motion();
        let theme = Theme::new(mode, preferences.backdrop, preferences.accent)
            .with_reduced_motion(reduced_motion);
        sync_component_theme(theme, cx);
        let chat = cx.new(|cx| ChatView::new(theme, window, cx));
        let connection_name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("OpenAI API")
                .placeholder("Connection name")
        });
        let connection_base_url = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("https://api.openai.com/v1")
                .placeholder("https://api.example.com/v1")
        });
        let connection_default_model =
            cx.new(|cx| InputState::new(window, cx).placeholder("Optional model ID"));
        let connection_api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("API key")
                .masked(true)
        });
        let onboarding_api_key =
            cx.new(|cx| InputState::new(window, cx).placeholder("sk-…").masked(true));
        let mcp_editor_id = cx.new(|cx| InputState::new(window, cx).placeholder("docs-server"));
        let mcp_editor_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("Optional display name"));
        let mcp_editor_transport = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(false)
                .rows(8)
                .placeholder("MCP transport JSON")
        });
        let initial_auto_settle_days = state
            .sidebar_settings
            .auto_settle_days
            .unwrap_or(3)
            .to_string();
        let auto_settle_days_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial_auto_settle_days.clone())
                .validate(|value, _cx| {
                    value
                        .parse::<u8>()
                        .is_ok_and(|days| (1..=90).contains(&days))
                })
        });
        let session_search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search every chat…"));
        let command_palette_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search commands, projects, chats…"));
        let sidebar_search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search threads")
                .clean_on_escape()
        });
        let sidebar_editor_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));

        for input in [
            &connection_name,
            &connection_base_url,
            &connection_default_model,
            &connection_api_key,
            &onboarding_api_key,
            &mcp_editor_id,
            &mcp_editor_name,
            &mcp_editor_transport,
        ] {
            cx.subscribe(input, |_this, _input, event, cx| {
                if matches!(
                    event,
                    InputEvent::Change | InputEvent::Focus | InputEvent::Blur
                ) {
                    cx.notify();
                }
            })
            .detach();
        }
        cx.subscribe(
            &session_search_input,
            |this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.schedule_session_search(cx);
                }
            },
        )
        .detach();
        cx.subscribe(
            &command_palette_input,
            |this, _input, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => this.run_selected_palette_command(cx),
                InputEvent::Change => this.command_palette_query_changed(cx),
                InputEvent::Focus | InputEvent::Blur => cx.notify(),
            },
        )
        .detach();
        cx.subscribe(
            &sidebar_search_input,
            |_this, _input, event: &InputEvent, cx| {
                if matches!(
                    event,
                    InputEvent::Change | InputEvent::Focus | InputEvent::Blur
                ) {
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &sidebar_editor_input,
            |this, _input, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => this.commit_sidebar_dialog(cx),
                InputEvent::Change | InputEvent::Focus | InputEvent::Blur => cx.notify(),
            },
        )
        .detach();
        cx.subscribe(
            &auto_settle_days_input,
            |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && this.state.sidebar_settings.auto_settle_days.is_some()
                    && let Ok(days) = input.read(cx).value().parse::<u8>()
                {
                    this.set_auto_settle_days(Some(days), cx);
                }
                cx.notify();
            },
        )
        .detach();

        cx.subscribe(&chat, |this, _chat, event, cx| match event {
            ChatEvent::NeedHistory {
                thread_id,
                after_seq,
            } => this.state.request_history(thread_id, *after_seq),
            ChatEvent::Submit {
                thread_id,
                text,
                attachments,
                steer,
            } => {
                let mut attachments = attachments.clone();
                if this.design_mode
                    && !attachments
                        .iter()
                        .any(|path| path == DESIGN_BRIEF_ATTACHMENT)
                {
                    attachments.push(DESIGN_BRIEF_ATTACHMENT.into());
                }
                let model = this
                    .selected_model_choice()
                    .map(|choice| choice.model.id.clone())
                    .filter(|model| !model.is_empty());
                this.state.send_turn(
                    thread_id,
                    SendTurnRequest {
                        text: text.clone(),
                        steer: *steer,
                        attachments,
                        model,
                        effort: this.effort.clone(),
                        service_tier: this.service_tier.clone(),
                    },
                );
            }
            ChatEvent::Interrupt { thread_id } => this.state.interrupt(thread_id),
            ChatEvent::DeleteQueuedTurn {
                thread_id,
                queued_turn_id,
            } => {
                let update = this.state.delete_queued_turn(thread_id, queued_turn_id);
                this.apply_client_update(update, cx);
            }
            ChatEvent::MoveQueuedTurn {
                thread_id,
                queued_turn_id,
                direction,
            } => {
                let update = this
                    .state
                    .move_queued_turn(thread_id, queued_turn_id, *direction);
                this.apply_client_update(update, cx);
            }
            ChatEvent::SteerQueuedTurn {
                thread_id,
                queued_turn_id,
            } => {
                let update = this.state.steer_queued_turn(thread_id, queued_turn_id);
                this.apply_client_update(update, cx);
            }
            ChatEvent::Create {
                project_path,
                text,
                attachments,
            } => {
                this.create_thread(project_path.clone(), text.clone(), attachments.clone(), cx);
            }
            ChatEvent::SelectModel { key } => this.select_model(key, cx),
            ChatEvent::SelectEffort { effort } => {
                this.effort = Some(effort.clone());
                this.remember_model_selection();
                this.sync_composer_settings(cx);
            }
            ChatEvent::ToggleFast => this.toggle_fast(cx),
            ChatEvent::SelectApproval { approval } => {
                this.approval = *approval;
                this.sync_composer_settings(cx);
            }
            ChatEvent::ToggleIsolation => {
                this.isolate_session = !this.isolate_session;
                this.sync_composer_settings(cx);
            }
            ChatEvent::ToggleDesign => {
                this.design_mode = !this.design_mode;
                this.sync_composer_settings(cx);
            }
            ChatEvent::SelectProject { path } => {
                if this.active_project_path.as_deref() != Some(path.as_str()) {
                    this.begin_new_chat(path.clone(), cx);
                }
            }
            ChatEvent::SelectBranch { branch } => {
                this.select_workspace_branch(branch.clone(), cx);
            }
            ChatEvent::OpenRollback => this.open_rollback(cx),
            ChatEvent::OpenCheckpoint { checkpoint_id } => {
                this.open_rollback_checkpoint(*checkpoint_id, cx);
            }
            ChatEvent::OpenImage { image, path, name } => {
                this.open_image_viewer(image.clone(), path.clone(), name.clone(), cx);
            }
            ChatEvent::PickAttachments => this.pick_attachments(cx),
            ChatEvent::TranscribeVoice { params } => {
                let update = this.state.transcribe_voice(params.clone());
                this.apply_client_update(update, cx);
            }
            ChatEvent::CancelVoice { request_id } => {
                this.state.cancel_voice(request_id.clone());
            }
            ChatEvent::RespondApproval {
                thread_id,
                approval_id,
                decision,
            } => {
                let update = this
                    .state
                    .respond_to_approval(thread_id, approval_id, *decision);
                this.apply_client_update(update, cx);
            }
            ChatEvent::RespondUserInput {
                thread_id,
                request_id,
                answers,
            } => {
                let update =
                    this.state
                        .respond_to_user_input(thread_id, request_id, answers.clone());
                this.apply_client_update(update, cx);
            }
            ChatEvent::RequestDiff { thread_id } => {
                let update = this.state.request_diff(thread_id);
                this.apply_client_update(update, cx);
            }
            ChatEvent::ReviewHunk {
                thread_id,
                version,
                path,
                hunk_id,
                decision,
            } => {
                let update = this.state.review_hunk(
                    thread_id,
                    ReviewHunkRequest {
                        version: version.clone(),
                        path: path.clone(),
                        hunk_id: hunk_id.clone(),
                        decision: *decision,
                    },
                );
                this.apply_client_update(update, cx);
            }
            ChatEvent::TerminalOpen {
                thread_id,
                columns,
                rows,
            } => {
                let update = this.state.open_terminal(thread_id, *columns, *rows);
                this.apply_client_update(update, cx);
            }
            ChatEvent::TerminalInput { terminal_id, data } => {
                let update = this.state.write_terminal(terminal_id, data.clone());
                this.apply_client_update(update, cx);
            }
            ChatEvent::TerminalResize {
                terminal_id,
                columns,
                rows,
            } => {
                let update = this.state.resize_terminal(terminal_id, *columns, *rows);
                this.apply_client_update(update, cx);
            }
            ChatEvent::TerminalClose { terminal_id } => {
                let update = this.state.close_terminal(terminal_id);
                this.apply_client_update(update, cx);
            }
        })
        .detach();

        if !fixture && let Some(events) = state.connect(endpoint) {
            cx.spawn(async move |view, cx| {
                while let Ok(event) = events.recv().await {
                    let result = view.update(cx, |this, cx| {
                        let update = this.state.handle_event(event);
                        this.apply_client_update(update, cx);
                    });
                    if result.is_err() {
                        break;
                    }
                }
            })
            .detach();
        }

        cx.observe_window_appearance(window, |this, window, cx| {
            let mode = theme_mode_for_appearance(window.appearance());
            if this.system_theme_mode != mode {
                this.system_theme_mode = mode;
                if this.preferences.theme == ThemePreference::System {
                    this.apply_native_theme(cx);
                }
            }
        })
        .detach();

        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let result = view.update(cx, |this, cx| {
                    let reduced_motion = crate::accessibility::prefers_reduced_motion();
                    if this.reduced_motion != reduced_motion {
                        this.reduced_motion = reduced_motion;
                        this.apply_native_theme(cx);
                    }
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |view, cx| {
            let mut delay = Duration::from_secs(1);
            loop {
                cx.background_executor().timer(delay).await;
                let result = view.update(cx, |this, cx| {
                    if this.state.sidebar_settings.mode == SidebarMode::Inbox {
                        cx.notify();
                    }
                    delay = if this.state.projects.iter().any(|project| {
                        project.sessions.iter().any(|session| {
                            session.running
                                || matches!(
                                    session.status,
                                    Some(ThreadInboxStatus::Starting | ThreadInboxStatus::Working)
                                )
                        })
                    }) {
                        Duration::from_secs(1)
                    } else {
                        Duration::from_secs(30)
                    };
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            theme,
            sidebar_collapsed: false,
            sidebar_transition: 0,
            sidebar_width,
            sidebar_resize_drag: None,
            sidebar_resize_focus: cx.focus_handle(),
            state,
            chat,
            selected_thread_id: None,
            active_project_path: None,
            chat_visible: false,
            selected_model_key: preferences.selected_model_key.clone(),
            effort: None,
            service_tier: None,
            approval: ApprovalMode::Ask,
            isolate_session: false,
            design_mode: false,
            sidebar_scope: None,
            scope_open: false,
            new_thread_picker: false,
            pending_new_chat_path: None,
            settings_open: false,
            settings_return_to_chat: false,
            settings_section: settings::SettingsSection::default(),
            settings_transition: 0,
            settings_open_transition: 0,
            settings_focus: cx.focus_handle(),
            settings_focus_pending: false,
            command_palette: CommandPaletteState::new(command_palette_input),
            focus_composer_pending: false,
            session_search: SessionSearchState::new(session_search_input),
            pending_reveal_turn: None,
            sidebar_controls: SidebarControlsState::new(sidebar_editor_input),
            stage_controls: StageControlsState::default(),
            collapsed_projects: HashSet::new(),
            expanded_project_sessions: HashSet::new(),
            sidebar_search: sidebar_search_input,
            sidebar_status_clocks: HashMap::new(),
            snoozed_expanded: false,
            settled_expanded: true,
            settled_limit: 10,
            account_menu_open: false,
            onboarding: (!fixture && preferences.setup_provider.is_none())
                .then(onboarding::OnboardingState::default),
            onboarding_api_key,
            system_theme_mode,
            reduced_motion,
            preferences,
            connection_editor_open: false,
            connection_submission_id: None,
            connection_preset: ModelConnectionPreset::Openai,
            connection_name,
            connection_base_url,
            connection_default_model,
            connection_api_key,
            mcp_editor: None,
            mcp_editor_submission_id: None,
            mcp_expanded_servers: HashSet::new(),
            mcp_editor_id,
            mcp_editor_name,
            mcp_editor_transport,
            auto_settle_days_input,
            provider_terminals: HashMap::new(),
            provider_terminal_ids: HashMap::new(),
            copied_provider_code: None,
            preview_capture,
            image_viewer: None,
            app_zoom: zoom_hud::AppZoomState::default(),
            fixture,
        }
    }

    fn apply_client_update(&mut self, update: ClientUpdate, cx: &mut Context<Self>) {
        let shell_changed = update.shell_changed;
        let mut refresh_stage = false;
        for chat_update in update.chat {
            refresh_stage |= match &chat_update {
                ChatUpdate::Refresh => true,
                ChatUpdate::Event(push) => {
                    self.selected_thread_id.as_deref() == Some(push.thread_id.as_str())
                        && matches!(
                            push.event,
                            harness_protocol::DomainEvent::TurnCompleted { .. }
                        )
                }
                _ => false,
            };
            let reveal_turn = match (&chat_update, self.pending_reveal_turn.as_ref()) {
                (
                    ChatUpdate::History {
                        thread_id,
                        replace: true,
                        ..
                    },
                    Some((pending_thread_id, turn_id)),
                ) if thread_id == pending_thread_id => Some(turn_id.clone()),
                _ => None,
            };
            self.chat.update(cx, |chat, cx| {
                chat.apply_update(chat_update, cx);
                if let Some(turn_id) = reveal_turn.as_deref() {
                    chat.reveal_turn(turn_id, cx);
                }
            });
            if reveal_turn.is_some() {
                self.pending_reveal_turn = None;
            }
        }
        if shell_changed {
            self.sync_model_selection();
        }
        if self.state.model_catalog_loaded
            && self.selected_model_key.is_some()
            && let Some(path) = self.pending_new_chat_path.take()
        {
            self.begin_new_chat(path, cx);
        }
        for event in update.shell_events {
            self.apply_shell_event(event, cx);
        }
        if refresh_stage {
            self.refresh_stage_context(cx);
        }
        if self.connection_submission_id.is_some() && self.state.connection_busy.is_none() {
            if self.state.connection_error.is_none() {
                self.connection_editor_open = false;
            }
            self.connection_submission_id = None;
        }
        if self.mcp_editor_submission_id.is_some() && self.state.mcp_busy.is_none() {
            if self.state.mcp_notice.is_some() || self.state.mcp_error.is_none() {
                self.mcp_editor = None;
            }
            self.mcp_editor_submission_id = None;
        }
        if shell_changed {
            self.sync_onboarding(cx);
            self.sync_composer_settings(cx);
            self.sync_stage_settings(cx);
            cx.notify();
        }
    }

    fn sync_model_selection(&mut self) {
        if !self.state.model_catalog_loaded {
            return;
        }
        let current = self.selected_model_key.as_ref().and_then(|selected| {
            self.state.model_catalog.iter().find(|choice| {
                choice.key == *selected && !self.preferences.hidden_models.contains(&choice.key)
            })
        });
        let Some(choice) = current
            .or_else(|| {
                self.state
                    .model_catalog
                    .iter()
                    .filter(|choice| !self.preferences.hidden_models.contains(&choice.key))
                    .find(|choice| choice.model.is_default)
            })
            .or_else(|| {
                self.state
                    .model_catalog
                    .iter()
                    .find(|choice| !self.preferences.hidden_models.contains(&choice.key))
            })
            .cloned()
        else {
            self.selected_model_key = None;
            self.effort = None;
            self.service_tier = None;
            if self.preferences.selected_model_key.take().is_some() {
                self.persist_native_preferences();
            }
            return;
        };
        let (effort, service_tier) = self.remembered_model_settings(&choice);
        self.selected_model_key = Some(choice.key.clone());
        self.effort = effort;
        self.service_tier = service_tier;
        self.remember_model_selection();
    }

    fn apply_shell_event(&mut self, event: ShellEvent, cx: &mut Context<Self>) {
        match event {
            ShellEvent::ProjectAdded { path } => {
                self.sidebar_scope = Some(path.clone());
                self.settled_limit = 10;
                self.begin_new_chat(path, cx);
            }
            ShellEvent::ProjectRemoved { path } => {
                if self.sidebar_scope.as_deref() == Some(path.as_str()) {
                    self.sidebar_scope = None;
                    self.settled_limit = 10;
                }
                if self.active_project_path.as_deref() == Some(path.as_str()) {
                    self.selected_thread_id = None;
                    self.active_project_path = None;
                    self.pending_reveal_turn = None;
                    self.chat_visible = false;
                    self.stage_controls.clear_restore_undo();
                    self.refresh_stage_context(cx);
                }
                cx.notify();
            }
            ShellEvent::ThreadHidden { thread_id } => {
                if self.selected_thread_id.as_deref() == Some(thread_id.as_str()) {
                    self.select_next_active_or_draft(&thread_id, cx);
                }
            }
            ShellEvent::ArchiveNeedsConfirmation { thread_id } => {
                self.show_archive_confirmation(thread_id, cx);
            }
            ShellEvent::ThreadArchived { thread_id } => {
                self.handle_thread_archived(thread_id, cx);
            }
            ShellEvent::ThreadStarted {
                thread_id,
                project_path,
                title,
                provider,
            } => {
                let project_name = self
                    .state
                    .projects
                    .iter()
                    .find(|project| project.path == project_path)
                    .map_or_else(|| project_path.clone(), |project| project.name.clone());
                self.selected_thread_id = Some(thread_id.clone());
                self.active_project_path = Some(project_path.clone());
                self.stage_controls.clear_restore_undo();
                self.account_menu_open = false;
                self.chat_visible = true;
                self.settings_open = false;
                self.chat.update(cx, |chat, cx| {
                    chat.promote_draft(
                        SessionContext {
                            thread_id: Some(thread_id.clone()),
                            title,
                            project_path,
                            project_name,
                            provider,
                            branch: None,
                        },
                        cx,
                    );
                });
                self.state.select_thread(&thread_id);
                self.refresh_stage_context(cx);
            }
            ShellEvent::OpenUrl { url } => cx.open_url(&url),
            ShellEvent::ProviderTerminalOpened {
                target,
                kind,
                terminal_id,
                buffered_output,
                early_exit,
            } => self.apply_provider_terminal_opened(
                target,
                kind,
                terminal_id,
                buffered_output,
                early_exit,
                cx,
            ),
            ShellEvent::ProviderTerminalOutput(push) => {
                self.apply_provider_terminal_output(push.terminal_id, push.data, cx)
            }
            ShellEvent::ProviderTerminalExit(push) => {
                self.apply_provider_terminal_exit(push.terminal_id, push.exit_code, cx);
            }
            ShellEvent::ProviderTerminalError {
                target,
                kind,
                terminal_id,
                message,
            } => self.apply_provider_terminal_error(target, kind, terminal_id, message, cx),
            ShellEvent::ProviderTerminalClosed { terminal_id } => {
                self.apply_provider_terminal_closed(terminal_id, cx);
            }
            ShellEvent::PreviewCaptureRequested(request) => {
                let Some(runtime) = self.preview_capture.clone() else {
                    self.state.submit_preview_capture_result(
                        harness_protocol::PreviewCaptureResult::Failed {
                            request_id: request.request_id,
                            error: "Preview capture is unavailable.".into(),
                        },
                    );
                    return;
                };
                let capture = cx
                    .background_executor()
                    .spawn(async move { runtime.capture(request) });
                cx.spawn(async move |view, cx| {
                    let result = capture.await;
                    let _ = view.update(cx, |this, _cx| {
                        this.state.submit_preview_capture_result(result);
                    });
                })
                .detach();
            }
            ShellEvent::SessionSearchResults {
                revision,
                append,
                page,
            } => {
                if self.session_search.open && self.session_search.revision == revision {
                    self.session_search.loading = false;
                    self.session_search.error = None;
                    if append {
                        self.session_search.results.extend(page.results);
                    } else {
                        self.session_search.results = page.results;
                    }
                    self.session_search.next_cursor = page.next_cursor;
                    cx.notify();
                }
            }
            ShellEvent::SessionSearchError { revision, message } => {
                if self.session_search.open && self.session_search.revision == revision {
                    self.session_search.loading = false;
                    self.session_search.error = Some(message);
                    cx.notify();
                }
            }
            ShellEvent::WorkspaceInfo {
                path,
                generation,
                info,
            } => self.apply_workspace_info(path, generation, info, cx),
            ShellEvent::WorkspaceBranches {
                path,
                generation,
                branches,
            } => self.apply_workspace_branches(path, generation, branches, cx),
            ShellEvent::WorkspaceSwitched {
                path,
                branch,
                generation,
                info,
            } => self.apply_workspace_switched(path, branch, generation, info, cx),
            ShellEvent::WorkspaceError {
                path,
                generation,
                operation,
                message,
            } => self.apply_workspace_error(path, generation, operation, message, cx),
            ShellEvent::Checkpoints {
                thread_id,
                generation,
                checkpoints,
            } => self.apply_checkpoints(thread_id, generation, checkpoints, cx),
            ShellEvent::ChangedSince {
                thread_id,
                checkpoint_id,
                generation,
                files,
            } => self.apply_changed_since(thread_id, checkpoint_id, generation, files, cx),
            ShellEvent::CheckpointRestored {
                thread_id,
                checkpoint_id,
                generation,
                undo,
            } => self.apply_checkpoint_restored(thread_id, checkpoint_id, generation, undo, cx),
            ShellEvent::RestoreUndone {
                thread_id,
                generation,
            } => self.apply_restore_undone(thread_id, generation, cx),
            ShellEvent::RollbackError {
                thread_id,
                generation,
                operation,
                message,
            } => self.apply_rollback_error(thread_id, generation, operation, message, cx),
            ShellEvent::UsageSummary {
                scope,
                generation,
                summary,
            } => self.apply_usage_summary(scope, generation, summary, cx),
            ShellEvent::UsageError {
                scope,
                generation,
                message,
            } => self.apply_usage_error(scope, generation, message, cx),
            ShellEvent::PanicStopped { result } => {
                self.apply_panic_stopped(result.sessions, cx);
            }
            ShellEvent::PanicStopError { message } => self.apply_panic_stop_error(message, cx),
        }
    }

    fn select_session(&mut self, thread_id: String, cx: &mut Context<Self>) {
        let selection = self.state.projects.iter().find_map(|project| {
            project
                .sessions
                .iter()
                .find(|session| session.id == thread_id)
                .map(|session| {
                    (
                        SessionContext {
                            thread_id: Some(session.id.clone()),
                            title: session.title.clone(),
                            project_path: project.path.clone(),
                            project_name: project.name.clone(),
                            provider: session.provider,
                            branch: session.worktree_branch.clone(),
                        },
                        session.agent.clone(),
                    )
                })
        });
        let Some((context, agent)) = selection else {
            return;
        };
        let matching_model = self
            .state
            .model_catalog
            .iter()
            .find(|choice| {
                !self.preferences.hidden_models.contains(&choice.key)
                    && choice.provider == context.provider
                    && (context.provider != harness_protocol::ProviderId::Acp
                        || choice.agent_id == agent)
            })
            .map(|choice| choice.key.clone());
        if let Some(key) = matching_model {
            self.select_model(&key, cx);
        }

        self.selected_thread_id = Some(thread_id.clone());
        self.active_project_path = Some(context.project_path.clone());
        self.stage_controls.clear_restore_undo();
        self.account_menu_open = false;
        self.chat_visible = true;
        self.settings_open = false;
        self.chat.update(cx, |chat, cx| {
            chat.begin_session(context, cx);
        });
        self.state.select_thread(&thread_id);
        self.refresh_stage_context(cx);
        cx.notify();
    }

    fn select_next_active_or_draft(&mut self, excluded_thread_id: &str, cx: &mut Context<Self>) {
        self.select_next_active_or_draft_excluding(
            &HashSet::from([excluded_thread_id.to_owned()]),
            cx,
        );
    }

    pub(super) fn select_next_active_or_draft_excluding(
        &mut self,
        excluded_thread_ids: &HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        let project_path = self.active_project_path.clone();
        let next = self
            .state
            .projects
            .iter()
            .flat_map(|project| project.sessions.iter())
            .filter(|session| {
                !excluded_thread_ids.contains(&session.id)
                    && matches!(
                        session.lifecycle.as_ref(),
                        Some(harness_protocol::ThreadLifecycle::Active { .. }) | None
                    )
            })
            .max_by(|left, right| {
                left.created_at
                    .partial_cmp(&right.created_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|session| session.id.clone());
        if let Some(next) = next {
            self.select_session(next, cx);
        } else if let Some(project_path) = project_path
            && self
                .state
                .projects
                .iter()
                .any(|project| project.path == project_path)
        {
            self.begin_new_chat(project_path, cx);
        } else {
            self.selected_thread_id = None;
            self.active_project_path = None;
            self.chat_visible = false;
            cx.notify();
        }
    }

    fn start_new_chat(&mut self, cx: &mut Context<Self>) {
        if self.state.sidebar_settings.mode == harness_protocol::SidebarMode::Classic {
            if let Some(path) = self.active_project_path.clone().or_else(|| {
                self.state
                    .projects
                    .first()
                    .map(|project| project.path.clone())
            }) {
                self.begin_new_chat(path, cx);
            } else {
                self.pick_project(cx);
            }
            return;
        }
        match self.state.projects.as_slice() {
            [] => self.pick_project(cx),
            [project] => self.begin_new_chat(project.path.clone(), cx),
            _ => {
                self.open_command_palette(
                    CommandScope::NewThread,
                    self.active_project_path.clone(),
                    cx,
                );
            }
        }
    }

    fn begin_new_chat(&mut self, project_path: String, cx: &mut Context<Self>) {
        self.sync_model_selection();
        let Some(choice) = self.selected_model_choice().cloned() else {
            self.chat_visible = false;
            self.settings_open = false;
            if self.state.model_catalog_loaded {
                self.pending_new_chat_path = None;
                self.state.notice = Some(
                    "No signed-in provider or configured model connection is available.".into(),
                );
            } else {
                self.pending_new_chat_path = Some(project_path);
                self.state.notice = Some("Loading available models…".into());
            }
            cx.notify();
            return;
        };
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.path == project_path)
        else {
            self.state.notice = Some("That project is no longer available.".into());
            cx.notify();
            return;
        };
        let context = SessionContext {
            thread_id: None,
            title: "New chat".into(),
            project_path: project.path.clone(),
            project_name: project.name.clone(),
            provider: choice.provider,
            branch: None,
        };
        self.selected_thread_id = None;
        self.active_project_path = Some(project.path.clone());
        self.pending_new_chat_path = None;
        self.chat_visible = true;
        self.settings_open = false;
        self.account_menu_open = false;
        self.scope_open = false;
        self.new_thread_picker = false;
        self.state.notice = None;
        self.chat.update(cx, |chat, cx| {
            chat.begin_draft(context, cx);
        });
        self.stage_controls.clear_restore_undo();
        self.refresh_stage_context(cx);
        cx.notify();
    }

    fn create_thread(
        &mut self,
        project_path: String,
        text: String,
        mut attachments: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(choice) = self.selected_model_choice().cloned() else {
            self.chat.update(cx, |chat, cx| {
                chat.apply_update(
                    crate::client_state::ChatUpdate::DraftError {
                        message: "Choose a configured model before starting this chat.".into(),
                        restore_text: text,
                        restore_attachments: attachments,
                    },
                    cx,
                );
            });
            return;
        };
        if self.design_mode
            && !attachments
                .iter()
                .any(|path| path == DESIGN_BRIEF_ATTACHMENT)
        {
            attachments.push(DESIGN_BRIEF_ATTACHMENT.into());
        }
        self.state.start_thread(NewThreadRequest {
            project_path,
            title: title_from(&text),
            attachments,
            text,
            choice,
            effort: self.effort.clone(),
            service_tier: self.service_tier.clone(),
            approval: self.approval,
            isolate: self.isolate_session,
        });
    }

    fn selected_model_choice(&self) -> Option<&crate::client_state::ModelChoice> {
        let key = self.selected_model_key.as_ref()?;
        self.state.model_catalog.iter().find(|choice| {
            choice.key == *key && !self.preferences.hidden_models.contains(&choice.key)
        })
    }

    fn select_model(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(next) = self
            .state
            .model_catalog
            .iter()
            .find(|choice| {
                choice.key == key && !self.preferences.hidden_models.contains(&choice.key)
            })
            .cloned()
        else {
            return;
        };
        let current_model = self
            .selected_model_choice()
            .map(|choice| choice.model.clone());
        let remembered = self
            .preferences
            .model_by_source
            .get(&source_key(&next))
            .filter(|selection| selection.model_key == next.key)
            .cloned();
        if remembered.is_some() {
            (self.effort, self.service_tier) = self.remembered_model_settings(&next);
        } else {
            self.effort = resolve_reasoning_effort(
                self.effort.as_deref(),
                current_model.as_ref(),
                &next.model,
            );
            self.service_tier = next_service_tier(
                current_model.as_ref(),
                &next.model,
                self.service_tier.as_deref(),
            );
        }
        self.selected_model_key = Some(next.key);
        self.remember_model_selection();
        self.chat.update(cx, |chat, cx| {
            chat.update_draft_provider(next.provider, cx);
        });
        self.sync_composer_settings(cx);
    }

    fn toggle_fast(&mut self, cx: &mut Context<Self>) {
        let Some(model) = self
            .selected_model_choice()
            .map(|choice| choice.model.clone())
        else {
            return;
        };
        self.service_tier = if is_fast_mode_enabled(&model, self.service_tier.as_deref()) {
            fast_mode_off_value(&model)
        } else {
            fast_service_tier(&model).map(|tier| tier.id.clone())
        };
        self.remember_model_selection();
        self.sync_composer_settings(cx);
    }

    fn remembered_model_settings(
        &self,
        choice: &crate::client_state::ModelChoice,
    ) -> (Option<String>, Option<String>) {
        let remembered = self
            .preferences
            .model_by_source
            .get(&source_key(choice))
            .filter(|selection| selection.model_key == choice.key);
        let effort = remembered
            .and_then(|selection| selection.effort.as_ref())
            .filter(|effort| choice.model.reasoning_efforts.contains(effort))
            .cloned()
            .or_else(|| default_reasoning_effort(&choice.model));
        let service_tier = remembered
            .and_then(|selection| selection.service_tier.as_ref())
            .filter(|selected| {
                choice
                    .model
                    .service_tiers
                    .iter()
                    .any(|tier| tier.id == **selected)
            })
            .cloned()
            .or_else(|| choice.model.default_service_tier.clone());
        (effort, service_tier)
    }

    fn remember_model_selection(&mut self) {
        let Some(choice) = self.selected_model_choice().cloned() else {
            return;
        };
        let entry = SourceSelection {
            model_key: choice.key.clone(),
            effort: self.effort.clone(),
            service_tier: self.service_tier.clone(),
        };
        let source = source_key(&choice);
        let mut changed =
            self.preferences.selected_model_key.as_deref() != Some(choice.key.as_str());
        if changed {
            self.preferences.selected_model_key = Some(choice.key);
        }
        if self.preferences.model_by_source.get(&source) != Some(&entry) {
            self.preferences.model_by_source.insert(source, entry);
            changed = true;
        }
        if changed {
            self.persist_native_preferences();
        }
    }

    fn sync_composer_settings(&mut self, cx: &mut Context<Self>) {
        let selected_provider = self.selected_model_choice().map(|choice| choice.provider);
        if let Some(provider) = selected_provider {
            self.state.ensure_voice_status(provider);
        }
        let voice_available = selected_provider.is_some_and(|provider| {
            provider == ProviderId::Codex
                && self
                    .state
                    .voice_statuses
                    .get(&provider)
                    .is_some_and(|status| status.available)
        });
        let auto_review_supported = self.selected_model_choice().is_some_and(|choice| {
            self.state
                .provider_statuses
                .iter()
                .find(|provider| provider.id == choice.provider)
                .and_then(|provider| provider.capabilities.as_ref())
                .and_then(|capabilities| capabilities.auto_review)
                .unwrap_or(false)
        });
        if self.approval == ApprovalMode::AutoReview && !auto_review_supported {
            self.approval = ApprovalMode::Ask;
        }
        self.chat.update(cx, |chat, cx| {
            chat.update_composer_settings(
                ComposerSettings {
                    models: self
                        .state
                        .model_catalog
                        .iter()
                        .filter(|choice| !self.preferences.hidden_models.contains(&choice.key))
                        .cloned()
                        .collect(),
                    selected_model_key: self.selected_model_key.clone(),
                    effort: self.effort.clone(),
                    service_tier: self.service_tier.clone(),
                    approval: self.approval,
                    auto_review_supported,
                    isolate: self.isolate_session,
                    design_mode: self.design_mode,
                    voice_available,
                },
                cx,
            );
        });
    }

    fn pick_project(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add Project".into()),
        });
        self.state.notice = None;
        cx.spawn(async move |view, cx| {
            let Ok(result) = receiver.await else {
                return;
            };
            match result {
                Ok(Some(paths)) => {
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    let path = path.to_string_lossy().into_owned();
                    let _ = view.update(cx, |this, cx| {
                        this.state.add_project(path);
                        cx.notify();
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    let message = error.to_string();
                    let _ = view.update(cx, |this, cx| {
                        this.state.notice = Some(format!("Could not open that project: {message}"));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn pick_attachments(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        let chat = self.chat.clone();
        cx.spawn(async move |_view, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let paths = paths
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let _ = chat.update(cx, |chat, cx| chat.add_attachments(paths, cx));
        })
        .detach();
    }

    fn sync_sidebar_status_clocks(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |duration| duration.as_secs_f64() * 1_000.0);
        let mut live = HashSet::new();
        for session in self
            .state
            .projects
            .iter()
            .flat_map(|project| project.sessions.iter())
        {
            live.insert(session.id.clone());
            let status = session.status.unwrap_or(if session.running {
                ThreadInboxStatus::Working
            } else {
                ThreadInboxStatus::Idle
            });
            self.sidebar_status_clocks
                .entry(session.id.clone())
                .and_modify(|clock| {
                    if clock.0 != status {
                        *clock = (status, now);
                    }
                })
                .or_insert_with(|| {
                    (
                        status,
                        if session.running {
                            now
                        } else {
                            session.created_at
                        },
                    )
                });
        }
        self.sidebar_status_clocks
            .retain(|thread_id, _| live.contains(thread_id));
    }

    fn handle_sidebar_search_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.sidebar_settings.mode != SidebarMode::Inbox
            || !self
                .sidebar_search
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            return;
        }
        if event.keystroke.key.eq_ignore_ascii_case("escape") {
            if !self.sidebar_search.read(cx).value().is_empty() {
                cx.stop_propagation();
                self.sidebar_search.update(cx, |input, cx| {
                    input.set_value(String::new(), window, cx);
                });
            }
            return;
        }
        let first = event.keystroke.key.eq_ignore_ascii_case("down");
        let last = event.keystroke.key.eq_ignore_ascii_case("up");
        if !first && !last {
            return;
        }
        let query = self.sidebar_search.read(cx).value();
        let ordered = ordered_inbox_ids(
            &self.state.projects,
            self.sidebar_scope.as_deref(),
            query.as_ref(),
        );
        let target = if first {
            ordered.first()
        } else {
            ordered.last()
        };
        if let Some(focus) =
            target.and_then(|thread_id| self.sidebar_controls.row_focus.get(thread_id).cloned())
        {
            cx.stop_propagation();
            focus.focus(window);
        }
    }

    fn sidebar_actions(&self, cx: &Context<Self>) -> SidebarActions {
        let select_view = cx.weak_entity();
        let choose_view = select_view.clone();
        let new_chat_view = select_view.clone();
        let new_project_view = select_view.clone();
        let search_view = select_view.clone();
        let settings_view = select_view.clone();
        let toggle_scope_view = select_view.clone();
        let select_scope_view = select_view.clone();
        let toggle_project_view = select_view.clone();
        let toggle_project_sessions_view = select_view.clone();
        let new_chat_in_project_view = select_view.clone();
        let settle_thread_view = select_view.clone();
        let wake_thread_view = select_view.clone();
        let unsettle_thread_view = select_view.clone();
        let rename_thread_view = select_view.clone();
        let open_menu_view = select_view.clone();
        let toggle_snoozed_view = select_view.clone();
        let toggle_settled_view = select_view.clone();
        let show_more_settled_view = select_view.clone();
        let toggle_account_view = select_view.clone();
        let panic_stop_view = select_view.clone();
        SidebarActions {
            select_session: Rc::new(move |thread_id, cx| {
                let _ = select_view.update(cx, |this, cx| this.select_session(thread_id, cx));
            }),
            choose_session: Rc::new(move |thread_id, modifiers: SelectionModifiers, cx| {
                let _ = choose_view.update(cx, |this, cx| {
                    this.choose_inbox_session(thread_id, modifiers, cx);
                });
            }),
            new_chat: Rc::new(move |cx| {
                let _ = new_chat_view.update(cx, |this, cx| this.start_new_chat(cx));
            }),
            new_project: Rc::new(move |cx| {
                let _ = new_project_view.update(cx, |this, cx| this.pick_project(cx));
            }),
            open_search: Rc::new(move |cx| {
                let _ = search_view.update(cx, |this, cx| {
                    this.open_session_search(None, cx);
                });
            }),
            open_settings: Rc::new(move |cx| {
                let _ = settings_view.update(cx, |this, cx| {
                    this.open_settings(cx);
                });
            }),
            toggle_scope: Rc::new(move |cx| {
                let _ = toggle_scope_view.update(cx, |this, cx| {
                    this.new_thread_picker = false;
                    this.scope_open = !this.scope_open;
                    cx.notify();
                });
            }),
            select_scope: Rc::new(move |path, cx| {
                let _ = select_scope_view.update(cx, |this, cx| {
                    let should_start = this.new_thread_picker && path.is_some();
                    this.sidebar_scope = path.clone();
                    this.settled_limit = 10;
                    this.scope_open = false;
                    this.new_thread_picker = false;
                    if should_start {
                        this.begin_new_chat(path.expect("checked above"), cx);
                    } else {
                        cx.notify();
                    }
                });
            }),
            toggle_project: Rc::new(move |path, cx| {
                let _ = toggle_project_view.update(cx, |this, cx| {
                    if !this.collapsed_projects.remove(&path) {
                        this.collapsed_projects.insert(path);
                    }
                    cx.notify();
                });
            }),
            toggle_project_sessions: Rc::new(move |path, cx| {
                let _ = toggle_project_sessions_view.update(cx, |this, cx| {
                    if !this.expanded_project_sessions.remove(&path) {
                        this.expanded_project_sessions.insert(path);
                    }
                    cx.notify();
                });
            }),
            new_chat_in_project: Rc::new(move |path, cx| {
                let _ = new_chat_in_project_view.update(cx, |this, cx| {
                    this.begin_new_chat(path, cx);
                });
            }),
            settle_thread: Rc::new(move |thread_id, cx| {
                let _ = settle_thread_view.update(cx, |this, cx| {
                    let update = this.state.settle_thread(thread_id);
                    this.apply_client_update(update, cx);
                });
            }),
            wake_thread: Rc::new(move |thread_id, cx| {
                let _ = wake_thread_view.update(cx, |this, cx| {
                    let update = this.state.unsnooze_thread(thread_id);
                    this.apply_client_update(update, cx);
                });
            }),
            unsettle_thread: Rc::new(move |thread_id, cx| {
                let _ = unsettle_thread_view.update(cx, |this, cx| {
                    let update = this.state.unsettle_thread(thread_id);
                    this.apply_client_update(update, cx);
                });
            }),
            rename_thread: Rc::new(move |thread_id, cx| {
                let _ = rename_thread_view.update(cx, |this, cx| {
                    this.begin_rename_thread(thread_id, cx);
                });
            }),
            open_menu: Rc::new(move |request: SidebarMenuRequest, position, cx| {
                let _ = open_menu_view.update(cx, |this, cx| {
                    this.open_sidebar_menu(request, position, cx);
                });
            }),
            toggle_snoozed: Rc::new(move |cx| {
                let _ = toggle_snoozed_view.update(cx, |this, cx| {
                    this.snoozed_expanded = !this.snoozed_expanded;
                    cx.notify();
                });
            }),
            toggle_settled: Rc::new(move |cx| {
                let _ = toggle_settled_view.update(cx, |this, cx| {
                    this.settled_expanded = !this.settled_expanded;
                    cx.notify();
                });
            }),
            show_more_settled: Rc::new(move |cx| {
                let _ = show_more_settled_view.update(cx, |this, cx| {
                    this.settled_limit = this.settled_limit.saturating_add(25);
                    cx.notify();
                });
            }),
            toggle_account: Rc::new(move |cx| {
                let _ = toggle_account_view.update(cx, |this, cx| {
                    this.account_menu_open = !this.account_menu_open;
                    cx.notify();
                });
            }),
            panic_stop: Rc::new(move |cx| {
                let _ = panic_stop_view.update(cx, |this, cx| {
                    this.account_menu_open = false;
                    this.request_panic_stop(cx);
                });
            }),
        }
    }

    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;

        div()
            .h(px(TITLEBAR_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .px(px(12.0))
            .bg(theme.titlebar.hsla())
            .on_mouse_down(MouseButton::Left, |event, window, _cx| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(
                div()
                    .id("toggle-sidebar")
                    .size(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.0))
                    .text_color(theme.titlebar_symbol.hsla())
                    .opacity(0.78)
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_2.hsla()).opacity(1.0))
                    .active(|style| style.opacity(0.7))
                    .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.toggle_sidebar(cx);
                    }))
                    .child(icon("icons/panel-left.svg", 15.0)),
            )
            .child(
                div()
                    .ml(px(8.0))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.titlebar_symbol.hsla())
                    .opacity(0.78)
                    .child("Personal Harness"),
            )
    }

    fn begin_sidebar_resize(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.sidebar_resize_drag = Some(SidebarResizeDrag {
            start_x: event.position.x,
            start_width: self.sidebar_width,
        });
        cx.stop_propagation();
    }

    fn update_sidebar_resize(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.sidebar_resize_drag else {
            return;
        };
        if !event.dragging() {
            self.sidebar_resize_drag = None;
            return;
        }
        let delta = f32::from(event.position.x - drag.start_x);
        let next = clamp_rail_width(drag.start_width + delta);
        if (self.sidebar_width - next).abs() >= f32::EPSILON {
            self.sidebar_width = next;
            cx.notify();
        }
    }

    fn finish_sidebar_resize(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.sidebar_resize_drag.take() else {
            return;
        };
        if self.sidebar_width <= COLLAPSE_RAIL_WIDTH {
            self.sidebar_width = drag.start_width;
            self.sidebar_collapsed = true;
            self.sidebar_transition = self.sidebar_transition.wrapping_add(1);
        } else {
            self.preferences.rail_width = self.sidebar_width.round() as u16;
            self.persist_native_preferences();
        }
        cx.notify();
    }

    fn resize_sidebar_with_keyboard(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let direction = if event.keystroke.key.eq_ignore_ascii_case("arrowleft") {
            -8.0
        } else if event.keystroke.key.eq_ignore_ascii_case("arrowright") {
            8.0
        } else {
            return;
        };
        cx.stop_propagation();
        let next = clamp_rail_width(self.sidebar_width + direction);
        if next <= COLLAPSE_RAIL_WIDTH {
            self.sidebar_collapsed = true;
            self.sidebar_transition = self.sidebar_transition.wrapping_add(1);
        } else {
            self.sidebar_width = next;
            self.preferences.rail_width = next.round() as u16;
            self.persist_native_preferences();
        }
        cx.notify();
    }

    fn settings_titlebar(&self) -> impl IntoElement {
        div()
            .relative()
            .h(px(TITLEBAR_HEIGHT))
            .w_full()
            .flex_none()
            .bg(self.theme.titlebar.hsla())
            .border_b_1()
            .border_color(self.theme.line.hsla())
            .child(crate::chrome::top_highlight(self.theme))
            .on_mouse_down(MouseButton::Left, |event, window, _cx| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
    }

    fn stage(&self) -> impl IntoElement {
        div()
            .flex_1()
            .h_full()
            .min_w(px(0.0))
            .flex()
            .items_center()
            .justify_center()
            .bg(self.theme.background.hsla())
            .text_color(self.theme.text_3.hsla())
            .text_size(px(12.5))
            .child(self.state.stage_message(self.fixture))
    }
}

impl Render for HarnessApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.prepare_command_palette_input(window, cx);
        self.prepare_session_search_input(window, cx);
        self.prepare_sidebar_controls_input(window, cx);
        self.prepare_auto_settle_days_input(window, cx);
        self.sync_sidebar_status_clocks();
        let sidebar_query = self.sidebar_search.read(cx).value().to_string();
        let ordered_sidebar_ids = if self.state.sidebar_settings.mode == SidebarMode::Inbox {
            ordered_inbox_ids(
                &self.state.projects,
                self.sidebar_scope.as_deref(),
                &sidebar_query,
            )
        } else {
            Vec::new()
        };
        self.sidebar_controls
            .sync_inbox_rows(&ordered_sidebar_ids, cx);
        if self.focus_composer_pending {
            self.chat
                .update(cx, |chat, cx| chat.focus_composer(window, cx));
            self.focus_composer_pending = false;
        }
        if self.settings_open && self.settings_focus_pending {
            self.settings_focus.focus(window);
            self.settings_focus_pending = false;
        }
        let content = if self.chat_visible {
            self.chat.clone().into_any_element()
        } else {
            self.stage().into_any_element()
        };
        let sidebar_actions = self.sidebar_actions(cx);
        let provider_name = self
            .selected_model_choice()
            .map(|choice| choice.source_name.clone())
            .unwrap_or_else(|| "Personal Harness".into());
        let usage_limits = self.stage_controls.usage_limits();
        let rail = sidebar(
            SidebarProps {
                theme: self.theme,
                projects: &self.state.projects,
                connection: self.state.connection,
                loaded: self.state.projects_loaded,
                fixture: self.fixture,
                mode: self.state.sidebar_settings.mode,
                selected_thread_id: self.selected_thread_id.as_deref(),
                selected_scope: self.sidebar_scope.as_deref(),
                query: &sidebar_query,
                search_input: self.sidebar_search.clone(),
                scope_open: self.scope_open,
                new_thread_picker: self.new_thread_picker,
                collapsed_projects: &self.collapsed_projects,
                expanded_project_sessions: &self.expanded_project_sessions,
                status_clocks: &self.sidebar_status_clocks,
                selected_ids: &self.sidebar_controls.selected_ids,
                row_focus: &self.sidebar_controls.row_focus,
                snoozed_expanded: self.snoozed_expanded,
                settled_expanded: self.settled_expanded,
                settled_limit: self.settled_limit,
                account_menu_open: self.account_menu_open,
                provider_name: &provider_name,
                usage_limits,
                panic_stopping: self.stage_controls.panic_stopping(),
                glass: self.preferences.sidebar_glass,
                width: self.sidebar_width,
            },
            sidebar_actions,
        );
        let rail_slot = div()
            .id("rail-slot")
            .relative()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .child(rail)
            .when(!self.sidebar_collapsed, |slot| {
                let focused = self.sidebar_resize_focus.is_focused(window);
                slot.child(
                    div()
                        .id("rail-resize")
                        .absolute()
                        .top_0()
                        .right_0()
                        .h_full()
                        .w(px(6.0))
                        .group("rail-resize")
                        .cursor_ew_resize()
                        .track_focus(&self.sidebar_resize_focus)
                        .tab_index(0)
                        .on_key_down(cx.listener(|this, event, _window, cx| {
                            this.resize_sidebar_with_keyboard(event, cx);
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event, _window, cx| {
                                this.begin_sidebar_resize(event, cx);
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right(px(2.0))
                                .h_full()
                                .w(px(1.0))
                                .bg(self.theme.text_3.hsla())
                                .opacity(if focused { 0.72 } else { 0.0 })
                                .group_hover("rail-resize", |line| line.opacity(0.72)),
                        ),
                )
            });
        let rail_slot = if self.sidebar_transition == 0 {
            rail_slot
                .w(px(if self.sidebar_collapsed {
                    0.0
                } else {
                    self.sidebar_width
                }))
                .into_any_element()
        } else {
            let collapsed = self.sidebar_collapsed;
            let sidebar_width = self.sidebar_width;
            rail_slot
                .with_animation(
                    ("rail-transition", self.sidebar_transition),
                    Animation::new(self.theme.motion.slow).with_easing(crate::theme::web_ease_out),
                    move |slot, delta| {
                        let visible = if collapsed { 1.0 - delta } else { delta };
                        slot.w(px(sidebar_width * visible))
                    },
                )
                .into_any_element()
        };
        let normal_body = div()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .flex()
            .child(rail_slot)
            .child(content)
            .into_any_element();
        let onboarding_open = self.onboarding.is_some();
        let body = if onboarding_open {
            self.onboarding_panel(cx)
        } else if self.settings_open {
            self.settings_panel(cx)
        } else {
            normal_body
        };
        let settings_open = self.settings_open;
        let search_open = self.session_search.open;
        let search_overlay = self.session_search_overlay(window, cx);
        let command_palette_open = self.command_palette.is_open();
        let command_palette_overlay = self.command_palette_overlay(window, cx);
        let sidebar_controls_open = self.sidebar_controls.is_open();
        let sidebar_controls_overlay = self.sidebar_controls_overlay(window, cx);
        let rollback_open = self.stage_controls.overlay_open();
        let rollback_overlay = self.rollback_overlay(cx);
        let global_notice = self.global_notice(cx);
        let image_viewer_overlay = self.image_viewer_overlay(cx);
        let zoom_hud = self.zoom_hud(cx);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(self.interface_font())
            .text_size(px(13.5))
            .text_color(self.theme.text.hsla())
            .bg(self.theme.background.hsla())
            .on_mouse_move(cx.listener(|this, event, _window, cx| {
                this.update_sidebar_resize(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.finish_sidebar_resize(cx);
                }),
            )
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_sidebar_search_key(event, window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_global_shortcut(event, window, cx);
            }))
            .when(command_palette_open, |root| {
                root.on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    this.handle_command_palette_key(event, cx);
                }))
            })
            .when(
                settings_open
                    && !search_open
                    && !command_palette_open
                    && !sidebar_controls_open
                    && !rollback_open,
                |root| {
                    root.track_focus(&self.settings_focus)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                            if event.keystroke.key.eq_ignore_ascii_case("escape") {
                                cx.stop_propagation();
                                this.close_settings(cx);
                            }
                        }))
                },
            )
            .when(
                search_open && !command_palette_open && !sidebar_controls_open && !rollback_open,
                |root| {
                    root.on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                        if event.keystroke.key.eq_ignore_ascii_case("escape") {
                            cx.stop_propagation();
                            this.close_session_search(cx);
                        }
                    }))
                },
            )
            .when(sidebar_controls_open && !rollback_open, |root| {
                root.on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    if event.keystroke.key.eq_ignore_ascii_case("escape") {
                        cx.stop_propagation();
                        this.close_sidebar_controls(cx);
                    }
                }))
            })
            .when(rollback_open, |root| {
                root.on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    if event.keystroke.key.eq_ignore_ascii_case("escape") {
                        cx.stop_propagation();
                        this.close_rollback(cx);
                    }
                }))
            })
            .child(if settings_open || onboarding_open {
                self.settings_titlebar().into_any_element()
            } else {
                self.titlebar(cx).into_any_element()
            })
            .child(body)
            .when_some(zoom_hud, |root, hud| root.child(hud))
            .when_some(command_palette_overlay, |root, overlay| root.child(overlay))
            .when_some(search_overlay, |root, overlay| root.child(overlay))
            .when_some(sidebar_controls_overlay, |root, overlay| {
                root.child(overlay)
            })
            .when_some(rollback_overlay, |root, overlay| root.child(overlay))
            .when_some(image_viewer_overlay, |root, viewer| root.child(viewer))
            .when_some(global_notice, |root, notice| root.child(notice))
    }
}

fn theme_mode_for_appearance(appearance: WindowAppearance) -> ThemeMode {
    match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
        WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
    }
}

fn clamp_rail_width(width: f32) -> f32 {
    width.round().clamp(MIN_RAIL_PREVIEW_WIDTH, MAX_RAIL_WIDTH)
}

fn icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}

fn resolve_reasoning_effort(
    current: Option<&str>,
    current_model: Option<&Model>,
    next_model: &Model,
) -> Option<String> {
    let next = &next_model.reasoning_efforts;
    if next.is_empty() {
        return None;
    }
    let default = || {
        next_model
            .default_reasoning_effort
            .as_ref()
            .filter(|effort| next.contains(effort))
            .cloned()
            .or_else(|| next.first().cloned())
    };
    let Some(current) = current else {
        return default();
    };
    let current_efforts = current_model
        .map(|model| model.reasoning_efforts.as_slice())
        .unwrap_or_default();
    let current_index = current_efforts.iter().position(|effort| effort == current);
    if current_efforts.len() > 1 && current_index == Some(current_efforts.len() - 1) {
        return next.last().cloned();
    }
    if next.iter().any(|effort| effort == current) {
        return Some(current.into());
    }
    if let Some(current_index) = current_index
        && current_efforts.len() > 1
    {
        let relative = current_index as f32 / (current_efforts.len() - 1) as f32;
        let next_index = (relative * (next.len() - 1) as f32).round() as usize;
        return next.get(next_index).cloned();
    }
    if let Some(current_rank) = reasoning_effort_rank(current) {
        return next
            .iter()
            .filter_map(|effort| {
                let rank = reasoning_effort_rank(effort)?;
                Some((effort, current_rank.abs_diff(rank), rank))
            })
            .min_by(|left, right| left.1.cmp(&right.1).then_with(|| right.2.cmp(&left.2)))
            .map(|(effort, _, _)| effort.clone())
            .or_else(default);
    }
    default()
}

fn default_reasoning_effort(model: &Model) -> Option<String> {
    model
        .default_reasoning_effort
        .as_ref()
        .filter(|effort| model.reasoning_efforts.contains(effort))
        .cloned()
        .or_else(|| model.reasoning_efforts.first().cloned())
}

fn reasoning_effort_rank(value: &str) -> Option<u8> {
    let normalized = value
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match normalized.as_str() {
        "none" => Some(0),
        "minimal" => Some(1),
        "xlow" | "extralow" => Some(2),
        "low" => Some(3),
        "medium" => Some(4),
        "high" => Some(5),
        "xhigh" | "extrahigh" => Some(6),
        "max" | "maximum" => Some(7),
        "ultra" => Some(8),
        _ => None,
    }
}

fn title_from(text: &str) -> String {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = clean.chars();
    let title = chars.by_ref().take(40).collect::<String>();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

fn sync_component_theme(theme: Theme, cx: &mut App) {
    let mode = match theme.mode {
        ThemeMode::Dark => gpui_component::ThemeMode::Dark,
        ThemeMode::Light => gpui_component::ThemeMode::Light,
    };
    gpui_component::Theme::change(mode, None, cx);
    let component = gpui_component::Theme::global_mut(cx);
    component.font_family = "Geist".into();
    component.font_size = px(13.5);
    component.mono_font_family = "Geist Mono".into();
    component.mono_font_size = px(12.5);
    component.radius = px(crate::RADIUS_MD);
    component.radius_lg = px(crate::RADIUS_LG);
    component.background = theme.background.hsla();
    component.foreground = theme.response_text.hsla();
    component.border = theme.line.hsla();
    component.input = theme.line_strong.hsla();
    component.muted = theme.surface.hsla();
    component.muted_foreground = theme.text_3.hsla();
    component.popover = theme.surface_2.hsla();
    component.popover_foreground = theme.text.hsla();
    component.link = theme.response_text.hsla();
    component.link_hover = theme.text.hsla();
    component.link_active = theme.text_2.hsla();
    component.selection = theme.attention.hsla().opacity(0.3);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(efforts: &[&str], default: Option<&str>) -> Model {
        Model {
            id: "model".into(),
            display_name: "Model".into(),
            description: None,
            is_default: true,
            reasoning_efforts: efforts.iter().map(|effort| (*effort).into()).collect(),
            default_reasoning_effort: default.map(str::to_owned),
            service_tiers: Vec::new(),
            default_service_tier: None,
        }
    }

    #[test]
    fn highest_effort_stays_highest_across_model_vocabularies() {
        let current = model(&["low", "high"], Some("high"));
        let next = model(&["low", "medium", "high", "max"], Some("medium"));

        assert_eq!(
            resolve_reasoning_effort(Some("high"), Some(&current), &next).as_deref(),
            Some("max")
        );
    }

    #[test]
    fn first_prompt_title_is_whitespace_normalized_and_bounded() {
        assert_eq!(title_from("  build\nthis   please "), "build this please");
        assert_eq!(title_from(&"x".repeat(41)), format!("{}…", "x".repeat(40)));
    }

    #[test]
    fn sidebar_resize_uses_the_web_preview_limits() {
        assert_eq!(clamp_rail_width(100.0), MIN_RAIL_PREVIEW_WIDTH);
        assert_eq!(clamp_rail_width(248.4), 248.0);
        assert_eq!(clamp_rail_width(248.6), 249.0);
        assert_eq!(clamp_rail_width(500.0), MAX_RAIL_WIDTH);
    }
}
