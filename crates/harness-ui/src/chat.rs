mod diff;
mod presentation;
mod search;
pub(crate) mod terminal;
mod transcript;
mod voice;

use crate::client_state::{ChatUpdate, ModelChoice};
use crate::theme::{CHAT_WIDTH, Theme};
use diff::DiffUiState;
use gpui::{
    Animation, AnimationExt, AnyElement, App, ClipboardEntry, Context, Entity, EventEmitter,
    Focusable, FontWeight, Image, ImageFormat, ListAlignment, ListOffset, ListState, ObjectFit,
    Render, SharedString, StyledImage, Window, div, ease_out_quint, img, prelude::*, px, relative,
    svg,
};
use gpui_component::RopeExt;
use gpui_component::input::{Input, InputEvent, InputState};
use harness_protocol::{
    ApprovalDecision, ApprovalKind, ApprovalMode, ApprovalReview, ApprovalReviewStatus,
    CheckpointSummary, DiffDecision, DomainEvent, Item, ProviderId, QueueDirection, RiskLevel,
    ThreadEventPush, ThreadQueueResult, UserInputQuestion, VoiceMimeType, VoiceTranscribeParams,
};
use harness_state::{ApplyOutcome, HistoryError, ThreadState};
use presentation::TranscriptPresentation;
use search::ThreadSearchState;
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use terminal::TerminalUiState;
use voice::{MAX_RECORDING_DURATION, VOICE_SAMPLE_RATE, VoiceRecorder};

const LIVE_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
const VOICE_LEVEL_INTERVAL: Duration = Duration::from_millis(45);
const MAX_WAVEFORM_LEVELS: usize = 160;
const MAX_PASTED_IMAGE_BYTES: usize = 25 * 1024 * 1024;
const TRANSCRIPT_BOTTOM_SLACK: f32 = 80.0;

#[derive(Clone)]
pub(crate) struct SessionContext {
    pub(crate) thread_id: Option<String>,
    pub(crate) title: String,
    pub(crate) project_path: String,
    pub(crate) project_name: String,
    pub(crate) provider: ProviderId,
    pub(crate) branch: Option<String>,
}

pub(crate) enum ChatEvent {
    NeedHistory {
        thread_id: String,
        after_seq: Option<u64>,
    },
    Submit {
        thread_id: String,
        text: String,
        attachments: Vec<String>,
        steer: bool,
    },
    Interrupt {
        thread_id: String,
    },
    DeleteQueuedTurn {
        thread_id: String,
        queued_turn_id: String,
    },
    MoveQueuedTurn {
        thread_id: String,
        queued_turn_id: String,
        direction: QueueDirection,
    },
    SteerQueuedTurn {
        thread_id: String,
        queued_turn_id: String,
    },
    Create {
        project_path: String,
        text: String,
        attachments: Vec<String>,
    },
    SelectModel {
        key: String,
    },
    SelectEffort {
        effort: String,
    },
    ToggleFast,
    SelectApproval {
        approval: ApprovalMode,
    },
    ToggleIsolation,
    ToggleDesign,
    SelectProject {
        path: String,
    },
    SelectBranch {
        branch: String,
    },
    OpenRollback,
    OpenCheckpoint {
        checkpoint_id: u64,
    },
    OpenImage {
        image: Arc<Image>,
        path: Option<String>,
        name: String,
    },
    PickAttachments,
    TranscribeVoice {
        params: VoiceTranscribeParams,
    },
    CancelVoice {
        request_id: String,
    },
    RespondApproval {
        thread_id: String,
        approval_id: String,
        decision: ApprovalDecision,
    },
    RespondUserInput {
        thread_id: String,
        request_id: String,
        answers: HashMap<String, Vec<String>>,
    },
    RequestDiff {
        thread_id: String,
    },
    ReviewHunk {
        thread_id: String,
        version: String,
        path: String,
        hunk_id: String,
        decision: DiffDecision,
    },
    TerminalOpen {
        thread_id: String,
        columns: u16,
        rows: u16,
    },
    TerminalInput {
        terminal_id: String,
        data: String,
    },
    TerminalResize {
        terminal_id: String,
        columns: u16,
        rows: u16,
    },
    TerminalClose {
        terminal_id: String,
    },
}

impl EventEmitter<ChatEvent> for ChatView {}

#[derive(Clone)]
pub(crate) struct ComposerSettings {
    pub(crate) models: Vec<ModelChoice>,
    pub(crate) selected_model_key: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) service_tier: Option<String>,
    pub(crate) approval: ApprovalMode,
    pub(crate) auto_review_supported: bool,
    pub(crate) isolate: bool,
    pub(crate) design_mode: bool,
    pub(crate) voice_available: bool,
}

impl Default for ComposerSettings {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            selected_model_key: None,
            effort: None,
            service_tier: None,
            approval: ApprovalMode::Ask,
            auto_review_supported: false,
            isolate: false,
            design_mode: false,
            voice_available: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StageProject {
    pub(crate) path: String,
    pub(crate) name: String,
}

#[derive(Clone, Default)]
pub(crate) struct StageSettings {
    pub(crate) projects: Vec<StageProject>,
    pub(crate) workspace_branch: Option<String>,
    pub(crate) branches: Vec<String>,
    pub(crate) checkpoints: Vec<CheckpointSummary>,
    pub(crate) branch_switching: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComposerMenu {
    Permissions,
    Model,
    Project,
    Branch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderMenu {
    Project,
}

struct InputFieldSync {
    value: String,
    masked: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum VoicePhase {
    #[default]
    Idle,
    Recording,
    Transcribing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TranscriptScrollMode {
    #[default]
    FollowEnd,
    AnchorTurn {
        start_row: usize,
    },
    Free,
}

struct PendingTranscript {
    text: String,
    cursor: usize,
    send_after: bool,
}

#[derive(Clone)]
struct ComposerAttachment {
    id: String,
    name: String,
    path: Option<String>,
    preview: Option<Arc<Image>>,
}

impl ComposerAttachment {
    fn file(path: String) -> Self {
        Self {
            id: path.clone(),
            name: path_label(&path).to_owned(),
            path: Some(path),
            preview: None,
        }
    }
}

pub(crate) struct ChatView {
    theme: Theme,
    session: Option<SessionContext>,
    state: ThreadState,
    queue: ThreadQueueResult,
    loading: bool,
    error: Option<String>,
    list_state: ListState,
    transcript_scroll_mode: Rc<Cell<TranscriptScrollMode>>,
    pending_anchor_turn: Option<String>,
    presentation: TranscriptPresentation,
    expanded_transcript_items: HashSet<String>,
    expanded_activities: HashSet<String>,
    copied_transcript_item: Option<String>,
    copy_generation: u64,
    working_tick_scheduled: bool,
    last_work_turn_id: Option<String>,
    last_specific_work_label: Option<String>,
    thread_search: ThreadSearchState,
    composer: Entity<InputState>,
    user_input_custom: Entity<InputState>,
    clear_composer: bool,
    restore_composer: Option<String>,
    creating: bool,
    history_in_flight: bool,
    pending_live: Vec<ThreadEventPush>,
    delta_flush_scheduled: bool,
    composer_settings: ComposerSettings,
    stage_settings: StageSettings,
    composer_menu: Option<ComposerMenu>,
    header_menu: Option<HeaderMenu>,
    attachments: Vec<ComposerAttachment>,
    attachment_error: Option<String>,
    active_user_input_id: Option<String>,
    user_input_step: usize,
    user_input_answers: HashMap<String, String>,
    user_input_custom_question: Option<String>,
    user_input_field_sync: Option<InputFieldSync>,
    pending_approvals: HashSet<String>,
    pending_user_inputs: HashSet<String>,
    action_errors: HashMap<String, String>,
    diff_ui: DiffUiState,
    terminal_ui: TerminalUiState,
    voice_recorder: VoiceRecorder,
    voice_phase: VoicePhase,
    voice_error: Option<String>,
    voice_request_id: Option<String>,
    voice_cursor: usize,
    voice_send_after: bool,
    voice_levels: Vec<f32>,
    voice_tick_scheduled: bool,
    pending_transcript: Option<PendingTranscript>,
}

impl ChatView {
    pub(crate) fn new(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 11)
                .placeholder("Do anything")
        });
        let user_input_custom =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type your answer…"));
        let thread_search = ThreadSearchState::new(window, cx);
        cx.subscribe(&composer, |this, _composer, event, cx| match event {
            InputEvent::PressEnter { secondary } => this.submit(*secondary, cx),
            InputEvent::Change | InputEvent::Focus | InputEvent::Blur => cx.notify(),
        })
        .detach();
        cx.subscribe(&user_input_custom, |this, input, event, cx| match event {
            InputEvent::PressEnter { .. } => this.advance_user_input(cx),
            InputEvent::Change => {
                if let Some(question_id) = this.user_input_custom_question.clone() {
                    let value = input.read(cx).value().to_string();
                    this.user_input_answers.insert(question_id, value);
                }
                cx.notify();
            }
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        })
        .detach();
        let transcript_scroll_mode = Rc::new(Cell::new(TranscriptScrollMode::FollowEnd));
        let list_state = ListState::new(0, ListAlignment::Bottom, px(500.0));
        let scroll_mode = transcript_scroll_mode.clone();
        let scroll_list = list_state.clone();
        list_state.set_scroll_handler(move |event, window, cx| {
            scroll_mode.set(if event.is_scrolled {
                TranscriptScrollMode::Free
            } else {
                TranscriptScrollMode::FollowEnd
            });
            let list = scroll_list.clone();
            let mode = scroll_mode.clone();
            window.defer(cx, move |_window, _cx| {
                let maximum = list.max_offset_for_scrollbar().height;
                let current = (-list.scroll_px_offset_for_scrollbar().y)
                    .max(px(0.0))
                    .min(maximum);
                let next = transcript_mode_for_bottom_gap(maximum - current);
                if next == TranscriptScrollMode::FollowEnd {
                    list.scroll_to(ListOffset {
                        item_ix: list.item_count(),
                        offset_in_item: px(0.0),
                    });
                }
                mode.set(next);
            });
        });

        Self {
            theme,
            session: None,
            state: ThreadState::default(),
            queue: ThreadQueueResult {
                items: Vec::new(),
                can_steer: false,
            },
            loading: false,
            error: None,
            list_state,
            transcript_scroll_mode,
            pending_anchor_turn: None,
            presentation: TranscriptPresentation::default(),
            expanded_transcript_items: HashSet::new(),
            expanded_activities: HashSet::new(),
            copied_transcript_item: None,
            copy_generation: 0,
            working_tick_scheduled: false,
            last_work_turn_id: None,
            last_specific_work_label: None,
            thread_search,
            composer,
            user_input_custom,
            clear_composer: false,
            restore_composer: None,
            creating: false,
            history_in_flight: false,
            pending_live: Vec::new(),
            delta_flush_scheduled: false,
            composer_settings: ComposerSettings::default(),
            stage_settings: StageSettings::default(),
            composer_menu: None,
            header_menu: None,
            attachments: Vec::new(),
            attachment_error: None,
            active_user_input_id: None,
            user_input_step: 0,
            user_input_answers: HashMap::new(),
            user_input_custom_question: None,
            user_input_field_sync: None,
            pending_approvals: HashSet::new(),
            pending_user_inputs: HashSet::new(),
            action_errors: HashMap::new(),
            diff_ui: DiffUiState::default(),
            terminal_ui: TerminalUiState::new(cx),
            voice_recorder: VoiceRecorder::default(),
            voice_phase: VoicePhase::Idle,
            voice_error: None,
            voice_request_id: None,
            voice_cursor: 0,
            voice_send_after: false,
            voice_levels: Vec::new(),
            voice_tick_scheduled: false,
            pending_transcript: None,
        }
    }

    pub(crate) fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
    }

    pub(crate) fn text_input_focused(&self, window: &Window, cx: &App) -> bool {
        self.composer.read(cx).focus_handle(cx).is_focused(window)
            || self
                .user_input_custom
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self
                .thread_search
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
            || self.terminal_ui.is_focused(window)
    }

    pub(crate) fn begin_session(&mut self, session: SessionContext, cx: &mut Context<Self>) {
        self.cancel_voice(cx);
        self.release_terminal_for_session_change(cx);
        self.session = Some(session);
        self.state = ThreadState::default();
        self.queue.items.clear();
        self.queue.can_steer = false;
        self.loading = true;
        self.error = None;
        self.list_state.reset(0);
        self.transcript_scroll_mode
            .set(TranscriptScrollMode::FollowEnd);
        self.pending_anchor_turn = None;
        self.presentation.clear();
        self.expanded_transcript_items.clear();
        self.expanded_activities.clear();
        self.copied_transcript_item = None;
        self.copy_generation = self.copy_generation.wrapping_add(1);
        self.working_tick_scheduled = false;
        self.last_work_turn_id = None;
        self.last_specific_work_label = None;
        self.thread_search.close();
        self.clear_composer = true;
        self.restore_composer = None;
        self.creating = false;
        self.history_in_flight = self
            .session
            .as_ref()
            .is_some_and(|session| session.thread_id.is_some());
        self.pending_live.clear();
        self.delta_flush_scheduled = false;
        self.attachments.clear();
        self.attachment_error = None;
        self.composer_menu = None;
        self.header_menu = None;
        self.active_user_input_id = None;
        self.user_input_step = 0;
        self.user_input_answers.clear();
        self.user_input_custom_question = None;
        self.user_input_field_sync = Some(InputFieldSync {
            value: String::new(),
            masked: false,
        });
        self.pending_approvals.clear();
        self.pending_user_inputs.clear();
        self.action_errors.clear();
        self.diff_ui.reset();
        self.reopen_visible_terminal(cx);
        cx.notify();
    }

    pub(crate) fn begin_draft(&mut self, session: SessionContext, cx: &mut Context<Self>) {
        debug_assert!(session.thread_id.is_none());
        self.begin_session(session, cx);
        self.loading = false;
    }

    pub(crate) fn promote_draft(&mut self, session: SessionContext, cx: &mut Context<Self>) {
        debug_assert!(session.thread_id.is_some());
        self.session = Some(session);
        self.creating = false;
        self.loading = true;
        self.history_in_flight = true;
        self.error = None;
        self.reopen_visible_terminal(cx);
        cx.notify();
    }

    pub(crate) fn update_composer_settings(
        &mut self,
        settings: ComposerSettings,
        cx: &mut Context<Self>,
    ) {
        self.composer_settings = settings;
        if !self.composer_settings.voice_available && self.voice_phase != VoicePhase::Idle {
            self.cancel_voice(cx);
        }
        cx.notify();
    }

    pub(crate) fn update_stage_settings(
        &mut self,
        settings: StageSettings,
        cx: &mut Context<Self>,
    ) {
        self.stage_settings = settings;
        cx.notify();
    }

    pub(crate) fn update_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        if self.theme != theme {
            self.theme = theme;
            cx.notify();
        }
    }

    pub(crate) fn update_draft_provider(&mut self, provider: ProviderId, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session
            && session.thread_id.is_none()
        {
            session.provider = provider;
            cx.notify();
        }
    }

    pub(crate) fn add_attachments(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        for path in paths {
            if !self
                .attachments
                .iter()
                .any(|attachment| attachment.path.as_deref() == Some(path.as_str()))
            {
                self.attachments.push(ComposerAttachment::file(path));
            }
        }
        self.attachment_error = None;
        cx.notify();
    }

    pub(crate) fn reveal_turn(&mut self, turn_id: &str, cx: &mut Context<Self>) {
        if let Some(row) = self.state.first_row_for_turn(turn_id) {
            self.transcript_scroll_mode.set(TranscriptScrollMode::Free);
            self.list_state.scroll_to_reveal_item(row);
            cx.notify();
        }
    }

    fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.transcript_scroll_mode
            .set(TranscriptScrollMode::FollowEnd);
        self.list_state.scroll_to(ListOffset {
            item_ix: self.list_state.item_count(),
            offset_in_item: px(0.0),
        });
        cx.notify();
    }

    pub(crate) fn rename_session(
        &mut self,
        thread_id: &str,
        title: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = &mut self.session
            && session.thread_id.as_deref() == Some(thread_id)
        {
            session.title = title;
            cx.notify();
        }
    }

    pub(crate) fn apply_update(&mut self, update: ChatUpdate, cx: &mut Context<Self>) {
        match update {
            ChatUpdate::History {
                thread_id,
                history,
                replace,
            } if self.is_selected(&thread_id) => {
                let old_len = self.state.timeline_len();
                let result = if replace {
                    self.state.replace_history(history)
                } else {
                    self.state.merge_history(history.events)
                };
                match result {
                    Ok(()) => {
                        let new_len = self.state.timeline_len();
                        let presentation_rows = self.presentation.rebuild(&self.state);
                        if replace {
                            self.list_state.reset(new_len);
                            self.transcript_scroll_mode
                                .set(TranscriptScrollMode::FollowEnd);
                            self.pending_anchor_turn = None;
                        } else if new_len > old_len {
                            self.list_state.splice(old_len..old_len, new_len - old_len);
                            for row in presentation_rows.into_iter().filter(|row| *row < old_len) {
                                self.list_state.splice(row..row + 1, 1);
                            }
                        } else {
                            for row in presentation_rows {
                                self.list_state.splice(row..row + 1, 1);
                            }
                        }
                        self.loading = false;
                        self.history_in_flight = false;
                        self.error = None;
                        self.sync_structured_requests();
                        self.sync_diff_summary();
                        self.refresh_work_label();
                        self.refresh_thread_search_hits(cx);
                        self.flush_pending_live(cx);
                    }
                    Err(error) => self.reconcile_after_error(error, cx),
                }
                cx.notify();
            }
            ChatUpdate::Queue { thread_id, queue } if self.is_selected(&thread_id) => {
                self.queue = queue;
                cx.notify();
            }
            ChatUpdate::Event(push) if self.is_selected(&push.thread_id) => {
                if self.history_in_flight {
                    self.pending_live.push(push);
                } else if matches!(&push.event, harness_protocol::DomainEvent::ItemDelta { .. }) {
                    self.queue_delta(push, cx);
                } else {
                    self.flush_pending_live(cx);
                    self.apply_live_events(vec![push], cx);
                }
            }
            ChatUpdate::Refresh => {
                if let Some(thread_id) = self
                    .session
                    .as_ref()
                    .and_then(|session| session.thread_id.as_ref())
                {
                    self.history_in_flight = true;
                    cx.emit(ChatEvent::NeedHistory {
                        thread_id: thread_id.clone(),
                        after_seq: Some(self.state.last_seq()),
                    });
                }
            }
            ChatUpdate::Error { thread_id, message } if self.is_selected(&thread_id) => {
                self.loading = false;
                self.history_in_flight = false;
                self.error = Some(message);
                self.flush_pending_live(cx);
                cx.notify();
            }
            ChatUpdate::DraftError {
                message,
                restore_text,
                restore_attachments,
            } if self
                .session
                .as_ref()
                .is_some_and(|session| session.thread_id.is_none()) =>
            {
                self.loading = false;
                self.creating = false;
                self.error = Some(message);
                self.restore_composer = Some(restore_text);
                self.attachments = restore_attachments
                    .into_iter()
                    .map(ComposerAttachment::file)
                    .collect();
                cx.notify();
            }
            ChatUpdate::TurnError {
                thread_id,
                message,
                restore_text,
                restore_attachments,
            } if self.is_selected(&thread_id) => {
                self.loading = false;
                self.error = Some(message);
                self.restore_composer = Some(restore_text);
                self.attachments = restore_attachments
                    .into_iter()
                    .map(ComposerAttachment::file)
                    .collect();
                cx.notify();
            }
            ChatUpdate::ApprovalError {
                thread_id,
                approval_id,
                message,
            } if self.is_selected(&thread_id) => {
                self.pending_approvals.remove(&approval_id);
                self.action_errors.insert(approval_id, message);
                cx.notify();
            }
            ChatUpdate::UserInputError {
                thread_id,
                request_id,
                message,
            } if self.is_selected(&thread_id) => {
                self.pending_user_inputs.remove(&request_id);
                self.action_errors.insert(request_id, message);
                cx.notify();
            }
            ChatUpdate::DiffSnapshot { thread_id, diff } if self.is_selected(&thread_id) => {
                self.diff_ui.apply_snapshot(diff);
                cx.notify();
            }
            ChatUpdate::DiffError {
                thread_id,
                message,
                stale,
            } if self.is_selected(&thread_id) => {
                let refresh = self.diff_ui.apply_error(message, stale);
                if refresh {
                    cx.emit(ChatEvent::RequestDiff { thread_id });
                }
                cx.notify();
            }
            ChatUpdate::VoiceTranscribed { request_id, text }
                if self.voice_request_id.as_deref() == Some(request_id.as_str()) =>
            {
                self.voice_request_id = None;
                self.voice_phase = VoicePhase::Idle;
                self.pending_transcript = Some(PendingTranscript {
                    text,
                    cursor: self.voice_cursor,
                    send_after: self.voice_send_after,
                });
                cx.notify();
            }
            ChatUpdate::VoiceTranscriptionError {
                request_id,
                message,
            } if self.voice_request_id.as_deref() == Some(request_id.as_str()) => {
                self.voice_request_id = None;
                self.voice_phase = VoicePhase::Idle;
                self.voice_error = Some(message);
                cx.notify();
            }
            ChatUpdate::Connection(connection) => {
                self.apply_terminal_connection(connection, cx);
            }
            ChatUpdate::TerminalOpened {
                thread_id,
                terminal_id,
            } => self.apply_terminal_opened(thread_id, terminal_id, cx),
            ChatUpdate::TerminalOutput(push) => {
                self.apply_terminal_output(push.terminal_id, push.data, cx);
            }
            ChatUpdate::TerminalExit(push) => {
                self.apply_terminal_exit(push.terminal_id, push.exit_code, cx);
            }
            ChatUpdate::TerminalOpenError { thread_id, message } => {
                self.apply_terminal_open_error(thread_id, message, cx);
            }
            ChatUpdate::TerminalError {
                terminal_id,
                message,
            } => self.apply_terminal_error(terminal_id, message, cx),
            ChatUpdate::History { .. }
            | ChatUpdate::Queue { .. }
            | ChatUpdate::Event(_)
            | ChatUpdate::Error { .. }
            | ChatUpdate::DraftError { .. }
            | ChatUpdate::TurnError { .. }
            | ChatUpdate::ApprovalError { .. }
            | ChatUpdate::UserInputError { .. }
            | ChatUpdate::DiffSnapshot { .. }
            | ChatUpdate::DiffError { .. }
            | ChatUpdate::VoiceTranscribed { .. }
            | ChatUpdate::VoiceTranscriptionError { .. } => {}
        }
    }

    fn queue_delta(&mut self, push: ThreadEventPush, cx: &mut Context<Self>) {
        self.pending_live.push(push);
        if self.delta_flush_scheduled {
            return;
        }
        self.delta_flush_scheduled = true;
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(LIVE_FLUSH_INTERVAL).await;
            let _ = view.update(cx, |this, cx| this.flush_pending_live(cx));
        })
        .detach();
    }

    fn flush_pending_live(&mut self, cx: &mut Context<Self>) {
        self.delta_flush_scheduled = false;
        if self.history_in_flight || self.pending_live.is_empty() {
            return;
        }
        let events = std::mem::take(&mut self.pending_live);
        self.apply_live_events(events, cx);
    }

    fn apply_live_events(&mut self, events: Vec<ThreadEventPush>, cx: &mut Context<Self>) {
        let old_len = self.state.timeline_len();
        let mut changed_items = HashSet::new();
        let mut transcript_changed = false;
        let mut presentation_changed = false;
        let mut applied = false;
        let mut reconcile_after = None;

        let mut events = events.into_iter();
        while let Some(push) = events.next() {
            let replay = push.clone();
            let started_turn = match &push.event {
                DomainEvent::TurnStarted { turn } => Some(turn.id.clone()),
                _ => None,
            };
            presentation_changed |= matches!(
                &push.event,
                DomainEvent::TurnStarted { .. }
                    | DomainEvent::ItemStarted { .. }
                    | DomainEvent::ItemCompleted { .. }
                    | DomainEvent::TurnCompleted { .. }
                    | DomainEvent::ThreadError { .. }
            );
            let changed_item = match &push.event {
                DomainEvent::ItemDelta {
                    turn_id, item_id, ..
                }
                | DomainEvent::ItemCompleted {
                    item:
                        Item {
                            turn_id,
                            id: item_id,
                            ..
                        },
                } => Some((turn_id.clone(), item_id.clone())),
                _ => None,
            };
            match self.state.apply_live(push.seq, push.event) {
                ApplyOutcome::Applied(changes) => {
                    applied = true;
                    transcript_changed |= changes.transcript;
                    if let Some(turn_id) = started_turn
                        && self.transcript_scroll_mode.get() != TranscriptScrollMode::Free
                    {
                        self.pending_anchor_turn = Some(turn_id);
                    }
                    if changes.transcript
                        && let Some(changed_item) = changed_item
                    {
                        changed_items.insert(changed_item);
                    }
                }
                ApplyOutcome::Duplicate => {}
                ApplyOutcome::NeedsHistory { after_seq } => {
                    reconcile_after = Some(after_seq);
                    self.history_in_flight = true;
                    self.pending_live.push(replay);
                    self.pending_live.extend(events);
                    break;
                }
            }
        }

        if applied {
            let new_len = self.state.timeline_len();
            let presentation_rows = if presentation_changed {
                self.presentation.rebuild(&self.state)
            } else {
                Vec::new()
            };
            let mut changed_rows = BTreeSet::new();
            if new_len > old_len {
                self.list_state.splice(old_len..old_len, new_len - old_len);
                changed_rows.extend(old_len..new_len);
            }
            if transcript_changed {
                changed_rows.extend(
                    changed_items.into_iter().filter_map(|(turn_id, item_id)| {
                        self.state.row_for_item(&turn_id, &item_id)
                    }),
                );
            }
            changed_rows.extend(presentation_rows);
            for row in changed_rows.iter().copied().filter(|row| *row < old_len) {
                self.list_state.splice(row..row + 1, 1);
            }
            self.anchor_pending_turn();
            let search_rows = changed_rows.into_iter().collect::<Vec<_>>();
            self.loading = false;
            self.error = None;
            self.sync_structured_requests();
            self.sync_diff_summary();
            self.refresh_work_label();
            self.refresh_thread_search_rows(&search_rows, cx);
            cx.notify();
        }

        if let Some(after_seq) = reconcile_after
            && let Some(thread_id) = self
                .session
                .as_ref()
                .and_then(|session| session.thread_id.as_ref())
        {
            cx.emit(ChatEvent::NeedHistory {
                thread_id: thread_id.clone(),
                after_seq: Some(after_seq),
            });
        }
    }

    fn anchor_pending_turn(&mut self) {
        let Some(turn_id) = self.pending_anchor_turn.clone() else {
            return;
        };
        if self.transcript_scroll_mode.get() == TranscriptScrollMode::Free {
            self.pending_anchor_turn = None;
            return;
        }
        let Some(start_row) = self.state.first_row_for_turn(&turn_id) else {
            return;
        };
        self.list_state.scroll_to(ListOffset {
            item_ix: start_row,
            offset_in_item: px(0.0),
        });
        self.transcript_scroll_mode
            .set(TranscriptScrollMode::AnchorTurn { start_row });
        self.pending_anchor_turn = None;
    }

    fn release_transcript_anchor_if_needed(&mut self, cx: &mut Context<Self>) {
        let TranscriptScrollMode::AnchorTurn { start_row } = self.transcript_scroll_mode.get()
        else {
            return;
        };
        let viewport = self.list_state.viewport_bounds();
        if viewport.size.height <= px(0.0) {
            return;
        }
        let Some(start_item) = self.state.item_at_row(start_row) else {
            return;
        };
        let turn_id = start_item.turn_id.as_str();
        let Some(last_row) = (start_row..self.state.timeline_len()).rev().find(|row| {
            self.state
                .item_at_row(*row)
                .is_some_and(|item| item.turn_id == turn_id)
        }) else {
            return;
        };
        let Some(start_bounds) = self.list_state.bounds_for_item(start_row) else {
            return;
        };
        let outgrew_viewport = self
            .list_state
            .bounds_for_item(last_row)
            .map_or(last_row > start_row, |last_bounds| {
                last_bounds.bottom() - start_bounds.top() > viewport.size.height
            });
        if outgrew_viewport {
            self.jump_to_latest(cx);
        }
    }

    fn reconcile_after_error(&mut self, error: HistoryError, cx: &mut Context<Self>) {
        self.error = Some(error.to_string());
        self.history_in_flight = true;
        if let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.as_ref())
        {
            cx.emit(ChatEvent::NeedHistory {
                thread_id: thread_id.clone(),
                after_seq: None,
            });
        }
    }

    fn is_selected(&self, thread_id: &str) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session
                .thread_id
                .as_deref()
                .is_some_and(|id| id == thread_id)
        })
    }

    fn sync_structured_requests(&mut self) {
        let live_ids = self
            .state
            .approvals
            .iter()
            .map(|request| request.id.as_str())
            .chain(
                self.state
                    .user_inputs
                    .iter()
                    .map(|request| request.id.as_str()),
            )
            .collect::<HashSet<_>>();
        self.pending_approvals
            .retain(|request_id| live_ids.contains(request_id.as_str()));
        self.pending_user_inputs
            .retain(|request_id| live_ids.contains(request_id.as_str()));
        self.action_errors
            .retain(|request_id, _| live_ids.contains(request_id.as_str()));

        let next_id = self
            .state
            .user_inputs
            .first()
            .map(|request| request.id.clone());
        if self.active_user_input_id != next_id {
            self.active_user_input_id = next_id;
            self.user_input_step = 0;
            self.user_input_answers.clear();
            self.user_input_custom_question = None;
            self.user_input_field_sync = Some(InputFieldSync {
                value: String::new(),
                masked: false,
            });
        }
    }

    fn active_user_input(&self) -> Option<&harness_protocol::UserInputRequest> {
        let request_id = self.active_user_input_id.as_deref()?;
        self.state
            .user_inputs
            .iter()
            .find(|request| request.id == request_id)
    }

    fn current_user_input_question(&self) -> Option<&UserInputQuestion> {
        self.active_user_input()?
            .questions
            .get(self.user_input_step)
    }

    fn prepare_user_input_field(&mut self) {
        let Some(question) = self.current_user_input_question().cloned() else {
            self.user_input_custom_question = None;
            return;
        };
        let answer = self.user_input_answers.get(&question.id).cloned();
        let is_option = answer.as_ref().is_some_and(|answer| {
            question
                .options
                .as_ref()
                .is_some_and(|options| options.iter().any(|option| option.label == *answer))
        });
        if answer.is_some() && !is_option {
            self.user_input_custom_question = Some(question.id);
            self.user_input_field_sync = Some(InputFieldSync {
                value: answer.unwrap_or_default(),
                masked: question.secret,
            });
        } else {
            self.user_input_custom_question = None;
        }
    }

    fn select_user_input_option(
        &mut self,
        question_id: String,
        answer: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .current_user_input_question()
            .is_none_or(|question| question.id != question_id)
        {
            return;
        }
        self.user_input_answers.insert(question_id, answer);
        self.user_input_custom_question = None;
        if let Some(request_id) = &self.active_user_input_id {
            self.action_errors.remove(request_id);
        }
        cx.notify();
    }

    fn select_user_input_custom(&mut self, question_id: String, cx: &mut Context<Self>) {
        let Some(question) = self.current_user_input_question().cloned() else {
            return;
        };
        if question.id != question_id {
            return;
        }
        if self.user_input_custom_question.as_deref() == Some(question.id.as_str()) {
            return;
        }
        let value =
            self.user_input_answers
                .get(&question.id)
                .filter(|answer| {
                    question.options.as_ref().is_none_or(|options| {
                        !options.iter().any(|option| option.label == **answer)
                    })
                })
                .cloned()
                .unwrap_or_default();
        self.user_input_answers
            .insert(question.id.clone(), value.clone());
        self.user_input_custom_question = Some(question.id);
        self.user_input_field_sync = Some(InputFieldSync {
            value,
            masked: question.secret,
        });
        if let Some(request_id) = &self.active_user_input_id {
            self.action_errors.remove(request_id);
        }
        cx.notify();
    }

    fn back_user_input(&mut self, cx: &mut Context<Self>) {
        if self.user_input_step == 0 || self.pending_user_input() {
            return;
        }
        self.user_input_step -= 1;
        self.prepare_user_input_field();
        cx.notify();
    }

    fn advance_user_input(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.active_user_input().cloned() else {
            return;
        };
        if self.pending_user_inputs.contains(&request.id) {
            return;
        }
        let Some(question) = request.questions.get(self.user_input_step) else {
            return;
        };
        let Some(_answer) = self
            .user_input_answers
            .get(&question.id)
            .map(|answer| answer.trim())
            .filter(|answer| !answer.is_empty())
        else {
            return;
        };
        if self.user_input_step + 1 < request.questions.len() {
            self.user_input_step += 1;
            self.prepare_user_input_field();
            cx.notify();
            return;
        }

        let answers = request
            .questions
            .iter()
            .map(|question| {
                self.user_input_answers
                    .get(&question.id)
                    .map(|answer| (question.id.clone(), vec![answer.trim().to_owned()]))
                    .filter(|(_, answers)| !answers[0].is_empty())
            })
            .collect::<Option<HashMap<_, _>>>();
        let Some(answers) = answers else {
            return;
        };
        let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
        else {
            return;
        };
        self.pending_user_inputs.insert(request.id.clone());
        self.action_errors.remove(&request.id);
        cx.emit(ChatEvent::RespondUserInput {
            thread_id,
            request_id: request.id,
            answers,
        });
        cx.notify();
    }

    fn pending_user_input(&self) -> bool {
        self.active_user_input_id
            .as_ref()
            .is_some_and(|request_id| self.pending_user_inputs.contains(request_id))
    }

    fn decide_approval(
        &mut self,
        approval_id: String,
        decision: ApprovalDecision,
        cx: &mut Context<Self>,
    ) {
        if self.pending_approvals.contains(&approval_id)
            || !self
                .state
                .approvals
                .iter()
                .any(|request| request.id == approval_id)
        {
            return;
        }
        let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
        else {
            return;
        };
        self.pending_approvals.insert(approval_id.clone());
        self.action_errors.remove(&approval_id);
        cx.emit(ChatEvent::RespondApproval {
            thread_id,
            approval_id,
            decision,
        });
        cx.notify();
    }

    fn submit(&mut self, steer: bool, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let text = self.composer.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        if self.creating {
            return;
        }
        if self
            .attachments
            .iter()
            .any(|attachment| attachment.path.is_none())
        {
            return;
        }
        self.clear_composer = true;
        self.composer_menu = None;
        let attachments = std::mem::take(&mut self.attachments)
            .into_iter()
            .filter_map(|attachment| attachment.path)
            .collect();
        self.attachment_error = None;
        match &session.thread_id {
            Some(thread_id) => cx.emit(ChatEvent::Submit {
                thread_id: thread_id.clone(),
                text,
                attachments,
                steer,
            }),
            None => {
                self.creating = true;
                self.loading = true;
                cx.emit(ChatEvent::Create {
                    project_path: session.project_path.clone(),
                    text,
                    attachments,
                });
            }
        }
        cx.notify();
    }

    fn primary_action(&mut self, cx: &mut Context<Self>) {
        if self.voice_phase != VoicePhase::Idle {
            return;
        }
        let has_text = !self.composer.read(cx).value().trim().is_empty();
        let has_draft = has_text || !self.attachments.is_empty();
        if self.state.running && !has_draft {
            if let Some(thread_id) = self
                .session
                .as_ref()
                .and_then(|session| session.thread_id.as_ref())
            {
                cx.emit(ChatEvent::Interrupt {
                    thread_id: thread_id.clone(),
                });
            }
        } else {
            self.submit(false, cx);
        }
    }

    fn emit_delete_queued_turn(&self, queued_turn_id: String, cx: &mut Context<Self>) {
        if let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
        {
            cx.emit(ChatEvent::DeleteQueuedTurn {
                thread_id,
                queued_turn_id,
            });
        }
    }

    fn emit_move_queued_turn(
        &self,
        queued_turn_id: String,
        direction: QueueDirection,
        cx: &mut Context<Self>,
    ) {
        if let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
        {
            cx.emit(ChatEvent::MoveQueuedTurn {
                thread_id,
                queued_turn_id,
                direction,
            });
        }
    }

    fn emit_steer_queued_turn(&self, queued_turn_id: String, cx: &mut Context<Self>) {
        if let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
        {
            cx.emit(ChatEvent::SteerQueuedTurn {
                thread_id,
                queued_turn_id,
            });
        }
    }

    fn edit_queued_turn(
        &mut self,
        queued_turn_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(queued_turn) = self
            .queue
            .items
            .iter()
            .find(|queued_turn| queued_turn.id == queued_turn_id)
            .cloned()
        else {
            return;
        };
        let draft = self.composer.read(cx).value().to_string();
        let text = merge_queued_draft(&queued_turn.text, &draft);
        self.composer.update(cx, |composer, cx| {
            composer.set_value(text, window, cx);
            composer.focus(window, cx);
        });
        merge_unique_attachments(&mut self.attachments, queued_turn.attachments);
        self.emit_delete_queued_turn(queued_turn.id, cx);
        cx.notify();
    }

    fn start_voice(&mut self, cx: &mut Context<Self>) {
        if self.voice_phase != VoicePhase::Idle
            || !self.composer_settings.voice_available
            || self.state.running
        {
            return;
        }
        self.voice_error = None;
        match self.voice_recorder.start() {
            Ok(()) => {
                self.voice_phase = VoicePhase::Recording;
                self.voice_levels.clear();
                self.schedule_voice_tick(cx);
            }
            Err(error) => self.voice_error = Some(error),
        }
        cx.notify();
    }

    fn stop_voice(&mut self, send_after: bool, cx: &mut Context<Self>) {
        if self.voice_phase != VoicePhase::Recording {
            return;
        }
        self.voice_phase = VoicePhase::Transcribing;
        self.voice_error = None;
        self.voice_cursor = self.composer.read(cx).cursor();
        self.voice_send_after = send_after;
        match self.voice_recorder.stop() {
            Ok(Some(recording)) => {
                let request_id = uuid::Uuid::new_v4().to_string();
                self.voice_request_id = Some(request_id.clone());
                cx.emit(ChatEvent::TranscribeVoice {
                    params: VoiceTranscribeParams {
                        request_id,
                        provider: ProviderId::Codex,
                        audio_base64: recording.audio_base64,
                        mime_type: VoiceMimeType::Wav,
                        sample_rate_hz: VOICE_SAMPLE_RATE,
                        duration_ms: recording.duration_ms,
                    },
                });
            }
            Ok(None) => {
                self.voice_phase = VoicePhase::Idle;
                self.voice_error = Some(
                    "No audio was captured. Check the selected microphone and try again.".into(),
                );
            }
            Err(error) => {
                self.voice_phase = VoicePhase::Idle;
                self.voice_error = Some(error);
            }
        }
        cx.notify();
    }

    fn cancel_voice(&mut self, cx: &mut Context<Self>) {
        if self.voice_phase == VoicePhase::Idle && self.voice_request_id.is_none() {
            return;
        }
        if let Some(request_id) = self.voice_request_id.take() {
            cx.emit(ChatEvent::CancelVoice { request_id });
        }
        self.voice_recorder.cancel();
        self.voice_phase = VoicePhase::Idle;
        self.voice_error = None;
        self.voice_levels.clear();
        cx.notify();
    }

    fn schedule_voice_tick(&mut self, cx: &mut Context<Self>) {
        if self.voice_tick_scheduled {
            return;
        }
        self.voice_tick_scheduled = true;
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(VOICE_LEVEL_INTERVAL).await;
                let keep_ticking = view
                    .update(cx, |this, cx| this.voice_tick(cx))
                    .unwrap_or(false);
                if !keep_ticking {
                    break;
                }
            }
        })
        .detach();
    }

    fn voice_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.voice_phase != VoicePhase::Recording {
            self.voice_tick_scheduled = false;
            return false;
        }
        if let Some(error) = self.voice_recorder.take_error() {
            self.voice_recorder.cancel();
            self.voice_phase = VoicePhase::Idle;
            self.voice_error = Some(error);
            self.voice_tick_scheduled = false;
            cx.notify();
            return false;
        }
        self.voice_levels.push(self.voice_recorder.level());
        if self.voice_levels.len() > MAX_WAVEFORM_LEVELS {
            let excess = self.voice_levels.len() - MAX_WAVEFORM_LEVELS;
            self.voice_levels.drain(..excess);
        }
        if self.voice_recorder.elapsed() >= MAX_RECORDING_DURATION {
            self.voice_tick_scheduled = false;
            self.stop_voice(false, cx);
            return false;
        }
        cx.notify();
        true
    }

    fn toggle_composer_menu(&mut self, menu: ComposerMenu, cx: &mut Context<Self>) {
        if self.state.running {
            return;
        }
        self.composer_menu = if self.composer_menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }

    fn toggle_header_menu(&mut self, menu: HeaderMenu, cx: &mut Context<Self>) {
        self.header_menu = if self.header_menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        self.composer_menu = None;
        cx.notify();
    }

    fn choose_project(&mut self, path: String, cx: &mut Context<Self>) {
        self.header_menu = None;
        self.composer_menu = None;
        cx.emit(ChatEvent::SelectProject { path });
        cx.notify();
    }

    fn choose_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        self.composer_menu = None;
        cx.emit(ChatEvent::SelectBranch { branch });
        cx.notify();
    }

    fn choose_model(&mut self, key: String, cx: &mut Context<Self>) {
        cx.emit(ChatEvent::SelectModel { key });
    }

    fn choose_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        cx.emit(ChatEvent::SelectEffort { effort });
    }

    fn choose_approval(&mut self, approval: ApprovalMode, cx: &mut Context<Self>) {
        self.composer_menu = None;
        cx.emit(ChatEvent::SelectApproval { approval });
        cx.notify();
    }

    fn selected_model(&self) -> Option<&ModelChoice> {
        let key = self.composer_settings.selected_model_key.as_ref()?;
        self.composer_settings
            .models
            .iter()
            .find(|choice| choice.key == *key)
    }

    fn header(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let session = self.session.clone();
        div()
            .relative()
            .h(px(44.0))
            .min_h(px(44.0))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(16.0))
            .child(self.header_project_picker(cx))
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(theme.text_3.hsla())
                    .child(session.as_ref().map_or_else(
                        || SharedString::from(""),
                        |value| {
                            if value.thread_id.is_some() {
                                value.title.clone().into()
                            } else {
                                SharedString::from("")
                            }
                        },
                    )),
            )
            .when_some(
                session.as_ref().and_then(|value| value.branch.clone()),
                |row, branch| {
                    row.child(
                        div()
                            .max_w(px(190.0))
                            .truncate()
                            .font_family("Geist Mono")
                            .text_size(px(10.5))
                            .text_color(theme.text_3.hsla())
                            .child(branch),
                    )
                },
            )
            .child(self.terminal_header_button(cx))
            .child(self.review_header_button(cx))
            .when(
                !self.state.running && !self.stage_settings.checkpoints.is_empty(),
                |row| row.child(self.rollback_header_button(cx)),
            )
            .when(self.header_menu == Some(HeaderMenu::Project), |row| {
                row.child(self.header_project_menu(cx))
            })
    }

    fn header_project_picker(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let enabled = !self.stage_settings.projects.is_empty();
        let open = self.header_menu == Some(HeaderMenu::Project);
        let label = self.session.as_ref().map_or_else(
            || SharedString::from("No project"),
            |session| SharedString::from(path_label(&session.project_path).to_owned()),
        );
        div()
            .id("header-project")
            .h(px(28.0))
            .max_w(px(190.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(7.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(if open {
                theme.line_strong.hsla()
            } else {
                theme.line.hsla().opacity(0.0)
            })
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text.hsla())
            .opacity(if enabled { 1.0 } else { 0.48 })
            .when(enabled, |picker| {
                picker
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface.hsla()))
                    .on_click(cx.listener(|this, event, _window, cx| {
                        cx.stop_propagation();
                        let _ = event;
                        this.toggle_header_menu(HeaderMenu::Project, cx);
                    }))
            })
            .child(div().min_w(px(0.0)).truncate().child(label))
            .child(
                div()
                    .flex_none()
                    .text_color(theme.text_3.hsla())
                    .child(svg_icon("icons/chevron-down.svg", 11.0)),
            )
            .into_any_element()
    }

    fn header_project_menu(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let active_path = self
            .session
            .as_ref()
            .map(|session| session.project_path.clone());
        div()
            .id("header-project-menu")
            .occlude()
            .absolute()
            .left(px(16.0))
            .top(px(38.0))
            .w(px(330.0))
            .max_h(px(300.0))
            .overflow_y_scroll()
            .rounded(px(11.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface_2.hsla())
            .shadow_lg()
            .p(px(5.0))
            .children(
                self.stage_settings
                    .projects
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, project)| {
                        let selected = active_path.as_deref() == Some(project.path.as_str());
                        let path = project.path.clone();
                        div()
                            .id(("header-project-option", index))
                            .min_h(px(45.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(8.0))
                            .rounded(px(8.0))
                            .when(selected, |row| row.bg(theme.surface_3.hsla()))
                            .cursor_pointer()
                            .hover(move |style| style.bg(theme.surface_3.hsla()))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.choose_project(path.clone(), cx);
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(theme.text_3.hsla())
                                    .child(svg_icon("icons/folder.svg", 14.0)),
                            )
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text.hsla())
                                            .child(path_label(&project.path).to_owned()),
                                    )
                                    .child(
                                        div()
                                            .mt(px(1.0))
                                            .truncate()
                                            .font_family("Geist Mono")
                                            .text_size(px(9.5))
                                            .text_color(theme.text_3.hsla())
                                            .child(project.path),
                                    ),
                            )
                            .when(selected, |row| row.child(svg_icon("icons/check.svg", 12.0)))
                    }),
            )
            .with_animation(
                "header-project-menu",
                Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                |menu, delta| menu.top(px(34.0 + 4.0 * delta)).opacity(delta),
            )
            .into_any_element()
    }

    fn rollback_header_button(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let count = self.stage_settings.checkpoints.len();
        div()
            .id("header-checkpoints")
            .h(px(28.0))
            .px(px(7.0))
            .flex()
            .items_center()
            .gap(px(5.0))
            .rounded(px(7.0))
            .text_size(px(11.5))
            .text_color(theme.text_3.hsla())
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface.hsla()).text_color(theme.text.hsla()))
            .active(|style| style.opacity(0.72))
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.header_menu = None;
                this.composer_menu = None;
                cx.emit(ChatEvent::OpenRollback);
                cx.notify();
            }))
            .child(svg_icon("icons/history.svg", 12.0))
            .child(format!(
                "{count} checkpoint{}",
                if count == 1 { "" } else { "s" }
            ))
            .into_any_element()
    }

    fn structured_surfaces(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.state.approvals.is_empty() && self.state.reviews.is_empty() {
            return None;
        }
        let approval_count = self.state.approvals.len();
        let approvals = self
            .state
            .approvals
            .iter()
            .enumerate()
            .map(|(index, request)| self.approval_card(request, index, cx));
        let reviews = self
            .state
            .reviews
            .iter()
            .enumerate()
            .map(|(index, review)| self.approval_review_card(review, approval_count + index));

        Some(
            div()
                .id("structured-surfaces")
                .flex_none()
                .max_h(px(290.0))
                .overflow_y_scroll()
                .px(px(24.0))
                .pb(px(7.0))
                .child(
                    div()
                        .w_full()
                        .max_w(px(CHAT_WIDTH))
                        .mx_auto()
                        .children(approvals)
                        .children(reviews),
                )
                .into_any_element(),
        )
    }

    fn approval_card(
        &self,
        request: &harness_protocol::ApprovalRequest,
        card_index: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let pending = self.pending_approvals.contains(&request.id);
        let weak = cx.weak_entity();
        let actions = [
            ("Deny", ApprovalDecision::Deny, false),
            ("Stop the turn", ApprovalDecision::Abort, true),
            (
                "Always this session",
                ApprovalDecision::ApproveSession,
                true,
            ),
            ("Allow once", ApprovalDecision::Approve, false),
        ]
        .into_iter()
        .enumerate()
        .map(|(action_index, (label, decision, quiet))| {
            let approval_id = request.id.clone();
            let weak = weak.clone();
            let action: Option<UiAction> = (!pending).then(|| {
                Rc::new(move |cx: &mut App| {
                    let approval_id = approval_id.clone();
                    let _ = weak.update(cx, |this, cx| {
                        this.decide_approval(approval_id, decision, cx);
                    });
                }) as UiAction
            });
            approval_action_button(
                card_index * 4 + action_index,
                label,
                action_index == 3,
                quiet,
                action_index == 2,
                theme,
                action,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();

        div()
            .w_full()
            .mb(px(8.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface.hsla())
            .px(px(14.0))
            .py(px(12.0))
            .child(
                div()
                    .mb(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(theme.text_2.hsla())
                    .child(svg_icon("icons/shield-question.svg", 14.0))
                    .child(
                        div()
                            .text_color(theme.text.hsla())
                            .font_weight(FontWeight::MEDIUM)
                            .child(approval_title(request.kind)),
                    )
                    .when(pending, |head| {
                        head.child(
                            div()
                                .ml_auto()
                                .text_size(px(11.0))
                                .text_color(theme.text_3.hsla())
                                .child("Submitting…"),
                        )
                    }),
            )
            .when_some(request.command.clone(), |card, command| {
                card.child(
                    div()
                        .w_full()
                        .mb(px(8.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(theme.line.hsla())
                        .bg(theme.background.hsla())
                        .px(px(10.0))
                        .py(px(8.0))
                        .font_family("Geist Mono")
                        .text_size(px(11.5))
                        .line_height(relative(1.45))
                        .whitespace_normal()
                        .child(command),
                )
            })
            .when_some(request.path.clone(), |card, path| {
                card.child(
                    div()
                        .mb(px(8.0))
                        .font_family("Geist Mono")
                        .text_size(px(11.5))
                        .line_height(relative(1.45))
                        .whitespace_normal()
                        .child(path),
                )
            })
            .when_some(request.cwd.clone(), |card, cwd| {
                card.child(
                    div()
                        .mb(px(8.0))
                        .flex()
                        .gap(px(5.0))
                        .text_size(px(11.5))
                        .text_color(theme.text_3.hsla())
                        .child("in")
                        .child(
                            div()
                                .font_family("Geist Mono")
                                .whitespace_normal()
                                .child(cwd),
                        ),
                )
            })
            .when_some(request.reason.clone(), |card, reason| {
                card.child(
                    div()
                        .mb(px(12.0))
                        .rounded(px(6.0))
                        .bg(theme.surface_2.hsla())
                        .px(px(10.0))
                        .py(px(8.0))
                        .text_size(px(11.5))
                        .line_height(relative(1.55))
                        .text_color(theme.text_2.hsla())
                        .whitespace_normal()
                        .child(reason),
                )
            })
            .when_some(
                self.action_errors.get(&request.id).cloned(),
                |card, error| {
                    card.child(
                        div()
                            .mb(px(9.0))
                            .text_size(px(11.0))
                            .text_color(theme.error.hsla())
                            .child(error),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .children(actions),
            )
            .into_any_element()
    }

    fn approval_review_card(&self, review: &ApprovalReview, card_index: usize) -> AnyElement {
        let theme = self.theme;
        let (status, icon_path, tone) = review_status(review.status, theme);
        div()
            .id(("approval-review", card_index))
            .w_full()
            .mb(px(8.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface.hsla())
            .px(px(14.0))
            .py(px(12.0))
            .child(
                div()
                    .mb(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(tone)
                    .child(svg_icon(icon_path, 14.0))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text.hsla())
                            .child(status),
                    )
                    .when_some(review.risk_level, |head, risk| {
                        head.child(
                            div()
                                .ml_auto()
                                .text_size(px(10.5))
                                .text_color(theme.text_3.hsla())
                                .child(format!("{} risk", risk_label(risk))),
                        )
                    }),
            )
            .child(
                div()
                    .font_family("Geist Mono")
                    .text_size(px(11.5))
                    .line_height(relative(1.5))
                    .whitespace_normal()
                    .child(review.description.clone()),
            )
            .when_some(review.rationale.clone(), |card, rationale| {
                card.child(
                    div()
                        .mt(px(6.0))
                        .text_size(px(11.5))
                        .line_height(relative(1.5))
                        .text_color(theme.text_2.hsla())
                        .whitespace_normal()
                        .child(rationale),
                )
            })
            .into_any_element()
    }

    fn user_input_card(&self, is_new_session: bool, cx: &Context<Self>) -> Option<AnyElement> {
        let request = self.active_user_input()?;
        let question = request.questions.get(self.user_input_step)?;
        let theme = self.theme;
        let pending = self.pending_user_inputs.contains(&request.id);
        let attachment_offset = self.attachment_shelf_height();
        let bottom = (if is_new_session { 150.0 } else { 114.0 }) + attachment_offset;

        if pending {
            return Some(
                div()
                    .absolute()
                    .left(px(0.0))
                    .bottom(px(bottom))
                    .h(px(36.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .rounded(px(18.0))
                    .border_1()
                    .border_color(theme.line_strong.hsla())
                    .bg(theme.surface.hsla())
                    .shadow_lg()
                    .px(px(13.0))
                    .text_size(px(11.5))
                    .text_color(theme.text_2.hsla())
                    .child(
                        div()
                            .size(px(8.0))
                            .rounded(px(4.0))
                            .bg(theme.attention.hsla()),
                    )
                    .child("Submitting answers…")
                    .into_any_element(),
            );
        }

        let selected = self.user_input_answers.get(&question.id);
        let options = question.options.as_deref().unwrap_or_default();
        let custom_available = question.allow_other || options.is_empty();
        let custom_selected = self.user_input_custom_question.as_deref() == Some(&question.id);
        let weak = cx.weak_entity();
        let option_rows = options.iter().enumerate().map(|(index, option)| {
            let active = selected.is_some_and(|answer| answer == &option.label);
            let question_id = question.id.clone();
            let answer = option.label.clone();
            let weak = weak.clone();
            div()
                .id(("brief-option", index))
                .min_h(px(38.0))
                .w_full()
                .flex()
                .items_center()
                .gap(px(9.0))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(if active {
                    theme.text_2.hsla().opacity(0.74)
                } else {
                    theme.line_strong.hsla().opacity(0.68)
                })
                .bg(if active {
                    theme.surface_3.hsla()
                } else {
                    theme.surface_2.hsla().opacity(0.58)
                })
                .cursor_pointer()
                .hover(move |style| {
                    style
                        .border_color(theme.line_strong.hsla())
                        .bg(theme.surface_3.hsla())
                        .text_color(theme.text.hsla())
                })
                .active(|style| style.opacity(0.78))
                .on_click(move |_event, _window, cx| {
                    let question_id = question_id.clone();
                    let answer = answer.clone();
                    let _ = weak.update(cx, |this, cx| {
                        this.select_user_input_option(question_id, answer, cx);
                    });
                })
                .child(radio_mark(active, theme))
                .child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if active {
                            theme.text.hsla()
                        } else {
                            theme.text_2.hsla()
                        })
                        .child(option.label.clone()),
                )
                .into_any_element()
        });

        let custom_row = custom_available.then(|| {
            let question_id = question.id.clone();
            let weak = cx.weak_entity();
            div()
                .id("brief-custom-option")
                .min_h(px(38.0))
                .w_full()
                .flex()
                .items_center()
                .gap(px(9.0))
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(if custom_selected {
                    theme.text_2.hsla().opacity(0.74)
                } else {
                    theme.line_strong.hsla().opacity(0.68)
                })
                .bg(if custom_selected {
                    theme.surface_3.hsla()
                } else {
                    theme.surface_2.hsla().opacity(0.58)
                })
                .cursor_pointer()
                .on_click(move |_event, _window, cx| {
                    let question_id = question_id.clone();
                    let _ = weak.update(cx, |this, cx| {
                        this.select_user_input_custom(question_id, cx);
                    });
                })
                .child(radio_mark(custom_selected, theme))
                .when(custom_selected, |row| {
                    row.child(
                        Input::new(&self.user_input_custom)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .h(px(28.0))
                            .w_full()
                            .text_size(px(11.5))
                            .text_color(theme.text.hsla()),
                    )
                })
                .when(!custom_selected, |row| {
                    row.child(
                        div()
                            .flex_1()
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(theme.line.hsla())
                            .bg(theme.surface.hsla().opacity(0.72))
                            .px(px(8.0))
                            .py(px(5.0))
                            .text_size(px(11.5))
                            .text_color(theme.text_3.hsla())
                            .child("Write your own answer…"),
                    )
                })
                .into_any_element()
        });

        let answer_ready = selected.is_some_and(|answer| !answer.trim().is_empty());
        let last_step = self.user_input_step + 1 == request.questions.len();
        let back_action: Option<UiAction> = (self.user_input_step > 0).then(|| {
            let weak = cx.weak_entity();
            Rc::new(move |cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.back_user_input(cx));
            }) as UiAction
        });
        let next_action: Option<UiAction> = answer_ready.then(|| {
            let weak = cx.weak_entity();
            Rc::new(move |cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.advance_user_input(cx));
            }) as UiAction
        });
        let request_animation_id = request.created_at.max(0.0) as u64;
        let error = self.action_errors.get(&request.id).cloned();

        Some(
            div()
                .absolute()
                .left(px(0.0))
                .right(px(0.0))
                .bottom(px(bottom))
                .w_full()
                .overflow_hidden()
                .rounded(px(18.0))
                .border_1()
                .border_color(theme.line_strong.hsla())
                .bg(theme.surface.hsla().opacity(0.98))
                .shadow_lg()
                .child(
                    div()
                        .px(px(18.0))
                        .pt(px(16.0))
                        .pb(px(14.0))
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .line_height(relative(1.35))
                                .text_color(theme.text.hsla())
                                .whitespace_normal()
                                .child(question.question.clone()),
                        )
                        .child(
                            div()
                                .mt(px(12.0))
                                .flex()
                                .flex_col()
                                .gap(px(5.0))
                                .children(option_rows)
                                .when_some(custom_row, |options, custom| options.child(custom)),
                        )
                        .when_some(error, |body, error| {
                            body.child(
                                div()
                                    .mt(px(9.0))
                                    .text_size(px(11.0))
                                    .text_color(theme.error.hsla())
                                    .child(error),
                            )
                        }),
                )
                .child(
                    div()
                        .min_h(px(48.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .border_t_1()
                        .border_color(theme.line.hsla())
                        .pl(px(18.0))
                        .pr(px(9.0))
                        .py(px(7.0))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(10.5))
                                .text_color(theme.text_3.hsla())
                                .child(format!(
                                    "Question {} of {}",
                                    self.user_input_step + 1,
                                    request.questions.len()
                                )),
                        )
                        .when_some(back_action, |footer, action| {
                            footer.child(input_nav_button(
                                "brief-back",
                                "Back",
                                false,
                                theme,
                                Some(action),
                            ))
                        })
                        .child(input_nav_button(
                            "brief-next",
                            if last_step { "Submit" } else { "Next" },
                            true,
                            theme,
                            next_action,
                        )),
                )
                .with_animation(
                    ("brief-input", request_animation_id),
                    Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                    move |card, delta| {
                        card.bottom(px(bottom - (8.0 * (1.0 - delta))))
                            .opacity(delta)
                    },
                )
                .into_any_element(),
        )
    }

    fn queue_panel(&self, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        if self.queue.items.is_empty() {
            return None;
        }
        let theme = self.theme;
        let can_steer = self.queue.can_steer;
        let queue_len = self.queue.items.len();
        let maximum_height = (window.viewport_size().height * 0.4).min(px(280.0));
        let rows = self
            .queue
            .items
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, queued_turn)| {
                let move_up_id = queued_turn.id.clone();
                let move_down_id = queued_turn.id.clone();
                let steer_id = queued_turn.id.clone();
                let edit_id = queued_turn.id.clone();
                let delete_id = queued_turn.id.clone();
                let move_up = div()
                    .id(("queue-move-up", index))
                    .w(px(20.0))
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.queue_action.hsla())
                    .opacity(if index == 0 { 0.25 } else { 1.0 })
                    .when(index > 0, |button| {
                        button
                            .cursor_pointer()
                            .hover(move |style| style.text_color(theme.text.hsla()))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.emit_move_queued_turn(
                                    move_up_id.clone(),
                                    QueueDirection::Up,
                                    cx,
                                );
                            }))
                    })
                    .child(svg_icon("icons/arrow-up.svg", 12.0));
                let move_down = div()
                    .id(("queue-move-down", index))
                    .w(px(20.0))
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.queue_action.hsla())
                    .opacity(if index + 1 == queue_len { 0.25 } else { 1.0 })
                    .when(index + 1 < queue_len, |button| {
                        button
                            .cursor_pointer()
                            .hover(move |style| style.text_color(theme.text.hsla()))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.emit_move_queued_turn(
                                    move_down_id.clone(),
                                    QueueDirection::Down,
                                    cx,
                                );
                            }))
                    })
                    .child(svg_icon("icons/arrow-down.svg", 12.0));
                let steer = can_steer.then(|| {
                    div()
                        .id(("queue-steer", index))
                        .h(px(28.0))
                        .flex_none()
                        .px(px(6.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(5.0))
                        .rounded(px(8.0))
                        .text_size(px(13.5))
                        .text_color(theme.queue_action.hsla())
                        .cursor_pointer()
                        .hover(move |style| {
                            style
                                .bg(theme.queue_hover.hsla())
                                .text_color(theme.text.hsla())
                        })
                        .active(|style| style.opacity(0.72))
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.emit_steer_queued_turn(steer_id.clone(), cx);
                        }))
                        .child(svg_icon("icons/corner-down-right.svg", 15.0))
                        .child("Steer")
                });
                let edit = div()
                    .id(("queue-edit", index))
                    .size(px(28.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .text_color(theme.queue_action.hsla())
                    .cursor_pointer()
                    .hover(move |style| {
                        style
                            .bg(theme.queue_hover.hsla())
                            .text_color(theme.text.hsla())
                    })
                    .active(|style| style.opacity(0.72))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.edit_queued_turn(&edit_id, window, cx);
                    }))
                    .child(svg_icon("icons/pencil.svg", 14.0));
                let delete = div()
                    .id(("queue-delete", index))
                    .size(px(28.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .text_color(theme.queue_action.hsla())
                    .cursor_pointer()
                    .hover(move |style| {
                        style
                            .bg(theme.queue_hover.hsla())
                            .text_color(theme.text.hsla())
                    })
                    .active(|style| style.opacity(0.72))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.emit_delete_queued_turn(delete_id.clone(), cx);
                    }))
                    .child(svg_icon("icons/trash-2.svg", 15.0));
                let row_animation_id = SharedString::from(format!("queue-row-{}", queued_turn.id));
                div()
                    .relative()
                    .min_w(px(0.0))
                    .min_h(px(38.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(7.0))
                    .text_color(theme.queue_text.hsla())
                    .child(
                        div()
                            .h(px(38.0))
                            .w(px(25.0))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .pl(px(5.0))
                            .border_l_2()
                            .border_color(theme.queue_line.hsla())
                            .child(move_up)
                            .child(move_down),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .truncate()
                            .text_size(px(13.5))
                            .child(queued_turn.text),
                    )
                    .when_some(steer, |row, steer| row.child(steer))
                    .child(edit)
                    .child(delete)
                    .with_animation(
                        row_animation_id,
                        Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()),
                        |row, delta| row.top(px(4.0 * (1.0 - delta))).opacity(delta),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        Some(
            div()
                .id("composer-queue-scroll")
                .absolute()
                .left(px(28.0))
                .right(px(28.0))
                .bottom(relative(1.0))
                .mb(px(-9.0))
                .min_w(px(0.0))
                .max_h(maximum_height)
                .overflow_y_scroll()
                .pt(px(5.0))
                .px(px(9.0))
                .pb(px(14.0))
                .rounded_t(px(20.0))
                .border_t_1()
                .border_l_1()
                .border_r_1()
                .border_color(theme.queue_line.hsla())
                .bg(theme.queue_background.hsla())
                .children(rows)
                .with_animation(
                    "composer-queue-panel",
                    Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                    |panel, delta| panel.opacity(delta),
                )
                .into_any_element(),
        )
    }

    fn composer(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let session = self.session.clone();
        let has_text = !self.composer.read(cx).value().trim().is_empty();
        let has_draft = has_text || !self.attachments.is_empty();
        let show_stop = self.state.running && !has_draft;
        let send_disabled = !has_text
            || self
                .attachments
                .iter()
                .any(|attachment| attachment.path.is_none())
            || self.creating;
        let running = self.state.running;
        let is_new_session = session
            .as_ref()
            .is_some_and(|session| session.thread_id.is_none());
        let popover = self.composer_popover(is_new_session, cx);
        let user_input = self.user_input_card(is_new_session, cx);
        let queue_panel = self.queue_panel(window, cx);
        let attach_view = cx.weak_entity();
        let attach_action: UiAction = Rc::new(move |cx| {
            let _ = attach_view.update(cx, |_this, cx| cx.emit(ChatEvent::PickAttachments));
        });
        let design_view = cx.weak_entity();
        let design_action: UiAction = Rc::new(move |cx| {
            let _ = design_view.update(cx, |_this, cx| cx.emit(ChatEvent::ToggleDesign));
        });
        div()
            .flex_none()
            .px(px(24.0))
            .pt(px(4.0))
            .pb(px(12.0))
            .child(
                div()
                    .relative()
                    .w_full()
                    .max_w(px(CHAT_WIDTH))
                    .mx_auto()
                    .when_some(user_input, |composer, user_input| {
                        composer.child(user_input)
                    })
                    .when_some(popover, |composer, popover| composer.child(popover))
                    .when_some(queue_panel, |composer, queue| composer.child(queue))
                    .child(
                        div()
                            .rounded(px(18.0))
                            .border_1()
                            .border_color(theme.line.hsla())
                            .bg(theme.prompt.hsla())
                            .overflow_hidden()
                            .when(is_new_session, |prompt| {
                                prompt.child(
                                    div()
                                        .h(px(36.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .px(px(8.0))
                                        .bg(theme.surface_2.hsla())
                                        .text_size(px(12.0))
                                        .text_color(theme.text_2.hsla())
                                        .child(self.project_shelf_trigger(cx))
                                        .child(
                                            div()
                                                .id("composer-isolation")
                                                .h(px(28.0))
                                                .px(px(8.0))
                                                .flex()
                                                .items_center()
                                                .gap(px(6.0))
                                                .rounded(px(7.0))
                                                .cursor_pointer()
                                                .hover(move |style| {
                                                    style
                                                        .bg(theme.surface_3.hsla())
                                                        .text_color(theme.text.hsla())
                                                })
                                                .on_click(cx.listener(
                                                    |_this, _event, _window, cx| {
                                                        cx.emit(ChatEvent::ToggleIsolation);
                                                    },
                                                ))
                                                .child(svg_icon(
                                                    if self.composer_settings.isolate {
                                                        "icons/git-branch.svg"
                                                    } else {
                                                        "icons/folder.svg"
                                                    },
                                                    14.0,
                                                ))
                                                .child(if self.composer_settings.isolate {
                                                    "Isolated"
                                                } else {
                                                    "Local"
                                                }),
                                        )
                                        .when(!self.stage_settings.branches.is_empty(), |shelf| {
                                            shelf.child(self.branch_shelf_trigger(cx))
                                        }),
                                )
                            })
                            .when_some(self.attachment_chips(cx), |prompt, chips| {
                                prompt.child(chips)
                            })
                            .child(
                                Input::new(&self.composer)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .h(px(68.0))
                                    .px(px(14.0))
                                    .py(px(10.0))
                                    .text_size(px(14.0))
                                    .line_height(relative(1.55))
                                    .text_color(theme.text.hsla()),
                            )
                            .child(
                                div()
                                    .h(px(38.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(5.0))
                                    .px(px(10.0))
                                    .pb(px(6.0))
                                    .child(icon_tool_button(
                                        "composer-attach",
                                        "icons/plus.svg",
                                        None,
                                        false,
                                        theme,
                                        Some(attach_action),
                                    ))
                                    .child(self.permission_trigger(running, cx))
                                    .child(icon_tool_button(
                                        "composer-design",
                                        "icons/palette.svg",
                                        Some("Design"),
                                        self.composer_settings.design_mode,
                                        theme,
                                        Some(design_action),
                                    ))
                                    .when(self.voice_phase == VoicePhase::Idle, |tools| {
                                        tools
                                            .child(div().flex_1())
                                            .when_some(
                                                self.model_trigger(running, cx),
                                                |tools, trigger| tools.child(trigger),
                                            )
                                            .when(
                                                self.composer_settings.voice_available && !running,
                                                |tools| tools.child(self.voice_button(cx)),
                                            )
                                            .child(
                                                div()
                                                    .id("composer-primary-action")
                                                    .size(px(28.0))
                                                    .rounded(px(9.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .bg(theme.text.hsla())
                                                    .text_color(theme.background.hsla())
                                                    .text_size(px(13.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .opacity(if !show_stop && send_disabled {
                                                        0.42
                                                    } else {
                                                        1.0
                                                    })
                                                    .when(show_stop || !send_disabled, |button| {
                                                        button
                                                            .cursor_pointer()
                                                            .active(|style| style.opacity(0.72))
                                                            .on_click(cx.listener(
                                                                |this, _event, _window, cx| {
                                                                    this.primary_action(cx);
                                                                },
                                                            ))
                                                    })
                                                    .child(if show_stop { "■" } else { "↑" }),
                                            )
                                    })
                                    .when(self.voice_phase != VoicePhase::Idle, |tools| {
                                        tools.child(self.voice_bar(running, cx))
                                    }),
                            ),
                    ),
            )
            .when_some(self.voice_error.clone(), |composer, error| {
                composer.child(
                    div()
                        .w_full()
                        .max_w(px(CHAT_WIDTH))
                        .mx_auto()
                        .mt(px(5.0))
                        .px(px(12.0))
                        .text_size(px(10.5))
                        .line_height(relative(1.4))
                        .text_color(theme.error.hsla())
                        .child(error),
                )
            })
    }

    fn voice_button(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        div()
            .id("composer-voice")
            .size(px(30.0))
            .ml(px(2.0))
            .rounded(px(15.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface.hsla())
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_2.hsla())
            .cursor_pointer()
            .hover(move |style| {
                style
                    .bg(theme.surface_2.hsla())
                    .text_color(theme.text.hsla())
            })
            .active(|style| style.opacity(0.72))
            .on_click(cx.listener(|this, _event, _window, cx| this.start_voice(cx)))
            .child(svg_icon("icons/mic.svg", 15.0))
            .into_any_element()
    }

    fn voice_bar(&self, running: bool, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let transcribing = self.voice_phase == VoicePhase::Transcribing;
        let duration = format_voice_duration(self.voice_recorder.elapsed());
        let stop = div()
            .id("composer-voice-stop")
            .size(px(28.0))
            .rounded(px(14.0))
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.surface_2.hsla())
            .text_color(theme.text_2.hsla())
            .opacity(if running { 0.5 } else { 1.0 })
            .when(!running, |button| {
                button
                    .cursor_pointer()
                    .hover(move |style| {
                        style
                            .bg(theme.surface_3.hsla())
                            .text_color(theme.text.hsla())
                    })
                    .active(|style| style.opacity(0.72))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        if transcribing {
                            this.cancel_voice(cx);
                        } else {
                            this.stop_voice(false, cx);
                        }
                    }))
            })
            .child(if transcribing { "×" } else { "■" });
        let submit = div()
            .id("composer-voice-submit")
            .size(px(28.0))
            .rounded(px(14.0))
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.text.hsla())
            .text_color(theme.background.hsla())
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .opacity(if running || transcribing { 0.5 } else { 1.0 })
            .when(!running && !transcribing, |button| {
                button
                    .cursor_pointer()
                    .hover(|style| style.opacity(0.9))
                    .active(|style| style.opacity(0.72))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.stop_voice(true, cx);
                    }))
            })
            .child(if transcribing { "…" } else { "↑" });
        div()
            .min_w(px(0.0))
            .flex_1()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(self.voice_waveform(transcribing))
            .child(
                div()
                    .flex_none()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_3.hsla())
                    .child(duration),
            )
            .child(stop)
            .child(submit)
            .into_any_element()
    }

    fn voice_waveform(&self, transcribing: bool) -> AnyElement {
        let theme = self.theme;
        let start = self.voice_levels.len().saturating_sub(52);
        let mut levels = vec![0.035_f32; 52_usize.saturating_sub(self.voice_levels.len())];
        levels.extend_from_slice(&self.voice_levels[start..]);
        div()
            .h(px(28.0))
            .min_w(px(0.0))
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(2.0))
            .overflow_hidden()
            .opacity(if transcribing { 0.5 } else { 1.0 })
            .children(levels.into_iter().enumerate().map(|(index, level)| {
                let smoothed = level.clamp(0.0, 1.0).powf(0.72);
                div()
                    .id(("voice-level", index))
                    .w(px(2.0))
                    .h(px(2.0 + smoothed * 22.0))
                    .rounded(px(1.0))
                    .bg(theme.text_2.hsla().opacity(0.16 + smoothed * 0.72))
            }))
            .into_any_element()
    }

    fn attachment_shelf_height(&self) -> f32 {
        if self.attachments.is_empty() && self.attachment_error.is_none() {
            0.0
        } else if self
            .attachments
            .iter()
            .any(|attachment| attachment.preview.is_some())
        {
            81.0
        } else {
            34.0
        }
    }

    fn attach_pasted_image(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(image) = cx
            .read_from_clipboard()
            .into_iter()
            .flat_map(|clipboard| clipboard.into_entries())
            .find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(Arc::new(image)),
                ClipboardEntry::String(_) => None,
            })
        else {
            return false;
        };
        let Some(extension) = pasted_image_extension(&image) else {
            self.attachment_error = Some("Couldn’t attach that image.".into());
            cx.notify();
            return true;
        };
        if image.bytes().is_empty() || image.bytes().len() > MAX_PASTED_IMAGE_BYTES {
            self.attachment_error = Some("Couldn’t attach that image.".into());
            cx.notify();
            return true;
        }

        let id = uuid::Uuid::new_v4().to_string();
        self.attachments.push(ComposerAttachment {
            id: id.clone(),
            name: format!("Pasted image.{extension}"),
            path: None,
            preview: Some(image.clone()),
        });
        self.attachment_error = None;
        let save = cx.background_spawn(async move { materialize_pasted_image(&image) });
        cx.spawn(async move |view, cx| {
            let result = save.await;
            let _ = view.update(cx, |this, cx| {
                let Some(index) = this
                    .attachments
                    .iter()
                    .position(|attachment| attachment.id == id)
                else {
                    return;
                };
                match result {
                    Ok(path) => {
                        this.attachments[index].path = Some(path.to_string_lossy().into_owned());
                    }
                    Err(_) => {
                        this.attachments.remove(index);
                        this.attachment_error = Some("Couldn’t attach that image.".into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        true
    }

    fn attachment_chips(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.attachments.is_empty() && self.attachment_error.is_none() {
            return None;
        }
        let theme = self.theme;
        let attachments = self
            .attachments
            .iter()
            .enumerate()
            .filter(|(_, attachment)| attachment.preview.is_some())
            .chain(
                self.attachments
                    .iter()
                    .enumerate()
                    .filter(|(_, attachment)| attachment.preview.is_none()),
            );
        Some(
            div()
                .min_h(px(self.attachment_shelf_height()))
                .w_full()
                .flex()
                .flex_wrap()
                .items_start()
                .gap(px(5.0))
                .px(px(10.0))
                .pt(px(9.0))
                .children(attachments.map(|(index, attachment)| {
                    if let Some(preview) = attachment.preview.clone() {
                        let image = preview.clone();
                        let path = attachment.path.clone();
                        let name = attachment.name.clone();
                        let loading = path.is_none();
                        div()
                            .id(("attachment-preview", index))
                            .relative()
                            .size(px(72.0))
                            .flex_none()
                            .rounded(px(12.0))
                            .border_1()
                            .border_color(theme.line_strong.hsla())
                            .overflow_hidden()
                            .cursor_pointer()
                            .on_click(cx.listener(move |_this, _event, _window, cx| {
                                cx.emit(ChatEvent::OpenImage {
                                    image: image.clone(),
                                    path: path.clone(),
                                    name: name.clone(),
                                });
                            }))
                            .child(
                                img(preview)
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .opacity(if loading { 0.68 } else { 1.0 }),
                            )
                            .child(
                                div()
                                    .id(("attachment-preview-remove", index))
                                    .absolute()
                                    .top(px(4.0))
                                    .right(px(4.0))
                                    .size(px(22.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(gpui::white().opacity(0.12))
                                    .bg(gpui::rgba(0x121212eb))
                                    .text_color(gpui::white())
                                    .cursor_pointer()
                                    .hover(|style| style.bg(gpui::rgb(0x222222)))
                                    .active(|style| style.opacity(0.78))
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        cx.stop_propagation();
                                        if index < this.attachments.len() {
                                            this.attachments.remove(index);
                                            cx.notify();
                                        }
                                    }))
                                    .child(svg_icon("icons/x.svg", 13.0)),
                            )
                            .when(loading, |preview| {
                                preview.child(
                                    svg()
                                        .path("icons/loader-circle.svg")
                                        .absolute()
                                        .right(px(8.0))
                                        .bottom(px(8.0))
                                        .size(px(12.0))
                                        .text_color(gpui::white())
                                        .with_animation(
                                            ("pasted-image-loading", index),
                                            Animation::new(Duration::from_millis(700)).repeat(),
                                            |spinner, delta| {
                                                spinner.with_transformation(
                                                    gpui::Transformation::rotate(gpui::percentage(
                                                        delta,
                                                    )),
                                                )
                                            },
                                        ),
                                )
                            })
                            .with_animation(
                                ("attachment-preview-in", index),
                                Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                                |preview, delta| preview.opacity(delta),
                            )
                            .into_any_element()
                    } else {
                        let path = attachment.path.as_deref().unwrap_or_default();
                        div()
                            .id(("attachment-chip", index))
                            .h(px(25.0))
                            .max_w(px(220.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(7.0))
                            .pr(px(3.0))
                            .rounded(px(8.0))
                            .bg(theme.surface_2.hsla())
                            .text_size(px(12.5))
                            .text_color(theme.text_2.hsla())
                            .child(svg_icon(
                                if is_image_path(path) {
                                    "icons/image.svg"
                                } else {
                                    "icons/file.svg"
                                },
                                13.0,
                            ))
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .truncate()
                                    .child(attachment.name.clone()),
                            )
                            .child(
                                div()
                                    .id(("attachment-remove", index))
                                    .size(px(17.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.0))
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style
                                            .bg(theme.surface_3.hsla())
                                            .text_color(theme.text.hsla())
                                    })
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        if index < this.attachments.len() {
                                            this.attachments.remove(index);
                                            cx.notify();
                                        }
                                    }))
                                    .child(svg_icon("icons/x.svg", 10.0)),
                            )
                            .with_animation(
                                ("attachment-chip-in", index),
                                Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                                |chip, delta| chip.opacity(delta),
                            )
                            .into_any_element()
                    }
                }))
                .when_some(self.attachment_error.clone(), |chips, error| {
                    chips.child(
                        div()
                            .h(px(25.0))
                            .flex()
                            .items_center()
                            .px(px(8.0))
                            .rounded(px(8.0))
                            .bg(theme.surface_2.hsla())
                            .text_size(px(12.5))
                            .text_color(theme.error.hsla())
                            .child(error),
                    )
                })
                .into_any_element(),
        )
    }

    fn project_shelf_trigger(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let open = self.composer_menu == Some(ComposerMenu::Project);
        let label = self.session.as_ref().map_or_else(
            || SharedString::from("Choose project"),
            |session| session.project_name.clone().into(),
        );
        div()
            .id("composer-project")
            .h(px(28.0))
            .max_w(px(230.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(if open {
                theme.line_strong.hsla()
            } else {
                theme.line.hsla().opacity(0.0)
            })
            .text_color(theme.text_2.hsla())
            .cursor_pointer()
            .hover(move |style| {
                style
                    .bg(theme.surface_3.hsla())
                    .text_color(theme.text.hsla())
            })
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.toggle_composer_menu(ComposerMenu::Project, cx);
            }))
            .child(svg_icon("icons/folder.svg", 14.0))
            .child(div().min_w(px(0.0)).truncate().child(label))
            .child(svg_icon("icons/chevron-down.svg", 11.0))
            .into_any_element()
    }

    fn branch_shelf_trigger(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let open = self.composer_menu == Some(ComposerMenu::Branch);
        let label = self
            .stage_settings
            .workspace_branch
            .clone()
            .or_else(|| self.stage_settings.branches.first().cloned())
            .unwrap_or_else(|| "No branch".into());
        div()
            .id("composer-branch")
            .h(px(28.0))
            .max_w(px(220.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(if open {
                theme.line_strong.hsla()
            } else {
                theme.line.hsla().opacity(0.0)
            })
            .text_color(theme.text_2.hsla())
            .opacity(if self.stage_settings.branch_switching {
                0.48
            } else {
                1.0
            })
            .when(!self.stage_settings.branch_switching, |trigger| {
                trigger
                    .cursor_pointer()
                    .hover(move |style| {
                        style
                            .bg(theme.surface_3.hsla())
                            .text_color(theme.text.hsla())
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.toggle_composer_menu(ComposerMenu::Branch, cx);
                    }))
            })
            .child(svg_icon("icons/git-branch.svg", 14.0))
            .child(div().min_w(px(0.0)).truncate().child(label))
            .child(svg_icon("icons/chevron-down.svg", 11.0))
            .into_any_element()
    }

    fn permission_trigger(&self, running: bool, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let approval = self.composer_settings.approval;
        let open = self.composer_menu == Some(ComposerMenu::Permissions);
        let (icon_path, label) = approval_meta(approval);
        div()
            .id("composer-permissions")
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(if open {
                theme.line_strong.hsla()
            } else {
                theme.line.hsla()
            })
            .text_size(px(11.5))
            .text_color(if approval == ApprovalMode::Full {
                theme.error.hsla()
            } else {
                theme.text_2.hsla()
            })
            .opacity(if running { 0.42 } else { 1.0 })
            .when(!running, |button| {
                button
                    .cursor_pointer()
                    .hover(move |style| {
                        style
                            .bg(theme.surface_2.hsla())
                            .border_color(theme.line_strong.hsla())
                            .text_color(theme.text.hsla())
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.toggle_composer_menu(ComposerMenu::Permissions, cx);
                    }))
            })
            .child(svg_icon(icon_path, 13.0))
            .child(label)
            .into_any_element()
    }

    fn model_trigger(&self, running: bool, cx: &Context<Self>) -> Option<AnyElement> {
        let selected = self.selected_model()?.clone();
        let theme = self.theme;
        let open = self.composer_menu == Some(ComposerMenu::Model);
        let effort = self
            .composer_settings
            .effort
            .as_deref()
            .map(title_case)
            .unwrap_or_default();
        let fast = self.composer_settings.service_tier.is_some();
        Some(
            div()
                .id("composer-model")
                .h(px(28.0))
                .max_w(px(270.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded(px(9.0))
                .border_1()
                .border_color(if open {
                    theme.line_strong.hsla()
                } else {
                    theme.line.hsla()
                })
                .text_size(px(11.5))
                .text_color(theme.text.hsla())
                .opacity(if running { 0.42 } else { 1.0 })
                .when(!running, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |style| {
                            style
                                .bg(theme.surface_2.hsla())
                                .border_color(theme.line_strong.hsla())
                        })
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.toggle_composer_menu(ComposerMenu::Model, cx);
                        }))
                })
                .when(fast, |button| {
                    button.child(
                        div()
                            .text_color(theme.attention.hsla())
                            .child(svg_icon("icons/zap.svg", 12.0)),
                    )
                })
                .child(provider_mark(selected.provider, theme, 13.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .max_w(px(165.0))
                        .truncate()
                        .child(selected.model.display_name),
                )
                .when(!effort.is_empty(), |button| {
                    button.child(div().text_color(theme.text_3.hsla()).child(effort))
                })
                .child(
                    div()
                        .text_color(theme.text_3.hsla())
                        .child(svg_icon("icons/chevron-down.svg", 12.0)),
                )
                .into_any_element(),
        )
    }

    fn composer_popover(&self, is_new_session: bool, cx: &Context<Self>) -> Option<AnyElement> {
        match self.composer_menu {
            Some(ComposerMenu::Permissions) => Some(self.permission_popover(is_new_session, cx)),
            Some(ComposerMenu::Model) => Some(self.model_popover(is_new_session, cx)),
            Some(ComposerMenu::Project) => Some(self.project_popover(cx)),
            Some(ComposerMenu::Branch) => Some(self.branch_popover(cx)),
            None => None,
        }
    }

    fn project_popover(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let active_path = self
            .session
            .as_ref()
            .map(|session| session.project_path.clone());
        div()
            .id("composer-project-menu")
            .occlude()
            .absolute()
            .left(px(8.0))
            .bottom(px(183.0))
            .w(px(350.0))
            .max_h(px(290.0))
            .overflow_y_scroll()
            .rounded(px(13.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface_2.hsla())
            .shadow_lg()
            .p(px(5.0))
            .children(
                self.stage_settings
                    .projects
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, project)| {
                        let selected = active_path.as_deref() == Some(project.path.as_str());
                        let path = project.path.clone();
                        div()
                            .id(("composer-project-option", index))
                            .min_h(px(45.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(8.0))
                            .rounded(px(8.0))
                            .when(selected, |row| row.bg(theme.surface_3.hsla()))
                            .cursor_pointer()
                            .hover(move |style| style.bg(theme.surface_3.hsla()))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.choose_project(path.clone(), cx);
                            }))
                            .child(svg_icon("icons/folder.svg", 14.0))
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text.hsla())
                                            .child(project.name),
                                    )
                                    .child(
                                        div()
                                            .mt(px(1.0))
                                            .truncate()
                                            .font_family("Geist Mono")
                                            .text_size(px(9.5))
                                            .text_color(theme.text_3.hsla())
                                            .child(project.path),
                                    ),
                            )
                            .when(selected, |row| row.child(svg_icon("icons/check.svg", 12.0)))
                    }),
            )
            .with_animation(
                "composer-project-menu",
                Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                |menu, delta| menu.opacity(delta),
            )
            .into_any_element()
    }

    fn branch_popover(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let selected = self.stage_settings.workspace_branch.clone();
        div()
            .id("composer-branch-menu")
            .occlude()
            .absolute()
            .left(px(175.0))
            .bottom(px(183.0))
            .w(px(280.0))
            .max_h(px(290.0))
            .overflow_y_scroll()
            .rounded(px(13.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface_2.hsla())
            .shadow_lg()
            .p(px(5.0))
            .children(
                self.stage_settings
                    .branches
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, branch)| {
                        let active = selected.as_deref() == Some(branch.as_str());
                        let value = branch.clone();
                        div()
                            .id(("composer-branch-option", index))
                            .h(px(34.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(8.0))
                            .rounded(px(8.0))
                            .when(active, |row| row.bg(theme.surface_3.hsla()))
                            .text_size(px(11.5))
                            .font_family("Geist Mono")
                            .text_color(if active {
                                theme.text.hsla()
                            } else {
                                theme.text_2.hsla()
                            })
                            .cursor_pointer()
                            .hover(move |style| {
                                style
                                    .bg(theme.surface_3.hsla())
                                    .text_color(theme.text.hsla())
                            })
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.choose_branch(value.clone(), cx);
                            }))
                            .child(svg_icon("icons/git-branch.svg", 13.0))
                            .child(div().min_w(px(0.0)).flex_1().truncate().child(branch))
                            .when(active, |row| row.child(svg_icon("icons/check.svg", 12.0)))
                    }),
            )
            .with_animation(
                "composer-branch-menu",
                Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                |menu, delta| menu.opacity(delta),
            )
            .into_any_element()
    }

    fn permission_popover(&self, is_new_session: bool, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let selected = self.composer_settings.approval;
        let auto_review = self.composer_settings.auto_review_supported;
        let attachment_offset = self.attachment_shelf_height();
        let options = [
            (
                ApprovalMode::Ask,
                "Ask first",
                "Read-only until you approve each action",
            ),
            (
                ApprovalMode::Auto,
                "Auto-approve",
                "Edits and commands inside this folder",
            ),
            (
                ApprovalMode::AutoReview,
                "Auto-review",
                "Reviews elevated actions before they run",
            ),
            (
                ApprovalMode::Full,
                "Full access",
                "No sandbox, no prompts, no undo. Use with care.",
            ),
        ];
        div()
            .absolute()
            .left(px(40.0))
            .bottom(px(
                (if is_new_session { 183.0 } else { 147.0 }) + attachment_offset
            ))
            .w(px(315.0))
            .rounded(px(13.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface_2.hsla())
            .shadow_lg()
            .p(px(5.0))
            .children(
                options
                    .into_iter()
                    .filter(|(mode, _, _)| *mode != ApprovalMode::AutoReview || auto_review)
                    .enumerate()
                    .map(|(index, (mode, title, detail))| {
                        let active = mode == selected;
                        let (icon_path, _) = approval_meta(mode);
                        div()
                            .id(("permission-option", index))
                            .min_h(px(48.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .px(px(8.0))
                            .rounded(px(8.0))
                            .when(active, |row| row.bg(theme.surface_3.hsla()))
                            .cursor_pointer()
                            .hover(move |style| style.bg(theme.surface_3.hsla()))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.choose_approval(mode, cx);
                            }))
                            .child(
                                div()
                                    .size(px(20.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(if mode == ApprovalMode::Full {
                                        theme.error.hsla()
                                    } else {
                                        theme.text_2.hsla()
                                    })
                                    .child(svg_icon(icon_path, 14.0)),
                            )
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text.hsla())
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .mt(px(2.0))
                                            .truncate()
                                            .text_size(px(10.5))
                                            .text_color(theme.text_3.hsla())
                                            .child(detail),
                                    ),
                            )
                            .when(active, |row| row.child(svg_icon("icons/check.svg", 12.0)))
                    }),
            )
            .into_any_element()
    }

    fn model_popover(&self, is_new_session: bool, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let selected_key = self.composer_settings.selected_model_key.clone();
        let selected = self.selected_model().cloned();
        let mut last_source = None::<String>;
        let attachment_offset = self.attachment_shelf_height();
        let mut rows = Vec::new();
        for (index, choice) in self.composer_settings.models.iter().cloned().enumerate() {
            if last_source.as_deref() != Some(choice.source_name.as_str()) {
                last_source = Some(choice.source_name.clone());
                rows.push(
                    div()
                        .mt(if rows.is_empty() { px(0.0) } else { px(4.0) })
                        .h(px(27.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_3.hsla())
                        .child(provider_mark(choice.provider, theme, 13.0))
                        .child(choice.source_name.clone())
                        .into_any_element(),
                );
            }
            let active = selected_key.as_deref() == Some(choice.key.as_str());
            let key = choice.key.clone();
            rows.push(
                div()
                    .id(("model-option", index))
                    .min_h(px(32.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .when(active, |row| row.bg(theme.surface_3.hsla()))
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_3.hsla()))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.choose_model(key.clone(), cx);
                    }))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(theme.text.hsla())
                            .child(choice.model.display_name),
                    )
                    .when(active, |row| row.child(svg_icon("icons/check.svg", 13.0)))
                    .into_any_element(),
            );
        }

        div()
            .absolute()
            .right(px(38.0))
            .bottom(px(
                (if is_new_session { 183.0 } else { 147.0 }) + attachment_offset
            ))
            .w(px(330.0))
            .max_h(px(520.0))
            .rounded(px(16.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface_2.hsla())
            .shadow_lg()
            .overflow_hidden()
            .child(
                div()
                    .id("model-options-scroll")
                    .max_h(px(246.0))
                    .overflow_y_scroll()
                    .p(px(6.0))
                    .children(rows),
            )
            .when_some(selected, |menu, selected| {
                menu.child(self.model_controls(selected, cx))
            })
            .into_any_element()
    }

    fn model_controls(&self, selected: ModelChoice, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let selected_effort = self.composer_settings.effort.clone();
        let fast = self.composer_settings.service_tier.is_some();
        let efforts = selected.model.reasoning_efforts;
        let has_fast = !selected.model.service_tiers.is_empty();
        div()
            .border_t_1()
            .border_color(theme.line.hsla())
            .bg(theme.surface.hsla())
            .p(px(8.0))
            .child(
                div()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .pl(px(6.0))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_3.hsla())
                            .child("Effort: ")
                            .child(
                                div().text_color(theme.text.hsla()).child(
                                    selected_effort
                                        .as_deref()
                                        .map_or_else(|| "Default".into(), title_case),
                                ),
                            ),
                    )
                    .when(has_fast, |row| {
                        row.child(
                            div()
                                .id("model-fast-toggle")
                                .size(px(30.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(15.0))
                                .border_1()
                                .border_color(if fast {
                                    theme.attention.hsla()
                                } else {
                                    theme.line_strong.hsla()
                                })
                                .bg(if fast {
                                    theme.attention.hsla().opacity(0.14)
                                } else {
                                    theme.surface_2.hsla()
                                })
                                .text_color(if fast {
                                    theme.attention.hsla()
                                } else {
                                    theme.text_2.hsla()
                                })
                                .cursor_pointer()
                                .active(|style| style.opacity(0.72))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    cx.emit(ChatEvent::ToggleFast);
                                    this.composer_menu = Some(ComposerMenu::Model);
                                }))
                                .child(svg_icon("icons/zap.svg", 15.0)),
                        )
                    }),
            )
            .when(!efforts.is_empty(), |controls| {
                controls.child(
                    div()
                        .h(px(36.0))
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(12.0))
                        .rounded(px(18.0))
                        .border_1()
                        .border_color(theme.line_strong.hsla())
                        .bg(theme.surface_2.hsla())
                        .children(efforts.into_iter().enumerate().map(|(index, effort)| {
                            let active = selected_effort.as_deref() == Some(effort.as_str());
                            let value = effort.clone();
                            div()
                                .id(("effort-stop", index))
                                .size(px(20.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                    this.choose_effort(value.clone(), cx);
                                }))
                                .child(
                                    div()
                                        .size(px(if active { 7.0 } else { 5.0 }))
                                        .rounded(px(4.0))
                                        .bg(if active {
                                            theme.text_2.hsla()
                                        } else {
                                            theme.text_3.hsla().opacity(0.55)
                                        }),
                                )
                        })),
                )
            })
            .into_any_element()
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_working_tick(cx);
        if matches!(
            self.transcript_scroll_mode.get(),
            TranscriptScrollMode::AnchorTurn { .. }
        ) {
            cx.on_next_frame(window, |this, _window, cx| {
                this.release_transcript_anchor_if_needed(cx);
            });
        }
        self.prepare_thread_search_input(window, cx);
        if let Some(text) = self.restore_composer.take() {
            self.composer.update(cx, |composer, cx| {
                composer.set_value(text, window, cx);
            });
            self.clear_composer = false;
        } else if self.clear_composer {
            self.composer.update(cx, |composer, cx| {
                composer.set_value("", window, cx);
            });
            self.clear_composer = false;
        }
        let mut send_transcript = false;
        if let Some(pending) = self.pending_transcript.take() {
            let current = self.composer.read(cx).value().to_string();
            if let Some(insertion) =
                insert_transcript_at_cursor(&current, &pending.text, pending.cursor)
            {
                self.composer.update(cx, |composer, cx| {
                    composer.set_value(insertion.text, window, cx);
                    let position = composer.text().offset_to_position(insertion.cursor);
                    composer.set_cursor_position(position, window, cx);
                });
                send_transcript = pending.send_after;
            }
        }
        if send_transcript {
            self.submit(false, cx);
        }
        if let Some(sync) = self.user_input_field_sync.take() {
            self.user_input_custom.update(cx, |input, cx| {
                input.set_masked(sync.masked, window, cx);
                input.set_value(sync.value, window, cx);
            });
        }
        let structured_surfaces = self.structured_surfaces(cx);
        let control_surface = self.control_surface(cx);
        let terminal_pane = self.terminal_pane(window, cx);
        let thread_search = self.thread_search_overlay(cx);
        div()
            .size_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .bg(self.theme.background.hsla())
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.update_terminal_resize(event, window, cx);
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.finish_terminal_resize(cx);
                }),
            )
            .capture_key_down(cx.listener(|this, event, window, cx| {
                this.handle_composer_paste_key(event, window, cx);
            }))
            .on_key_down(cx.listener(|this, event, window, cx| {
                this.handle_thread_navigation_key(event, window, cx);
            }))
            .child(self.header(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(self.timeline(cx))
                    .when_some(thread_search, |thread, search| thread.child(search)),
            )
            .when_some(structured_surfaces, |view, surfaces| view.child(surfaces))
            .when_some(control_surface, |view, surface| view.child(surface))
            .when_some(terminal_pane, |view, terminal| view.child(terminal))
            .child(self.composer(window, cx))
    }
}

fn approval_title(kind: ApprovalKind) -> &'static str {
    match kind {
        ApprovalKind::Command => "Run this command?",
        ApprovalKind::FileChange => "Write to your files?",
        ApprovalKind::Permissions => "Grant extra access?",
    }
}

fn review_status(
    status: ApprovalReviewStatus,
    theme: Theme,
) -> (&'static str, &'static str, gpui::Hsla) {
    match status {
        ApprovalReviewStatus::InProgress => (
            "Reviewing access",
            "icons/scan-eye.svg",
            theme.attention.hsla(),
        ),
        ApprovalReviewStatus::Approved => (
            "Access approved",
            "icons/shield-check.svg",
            theme.success.hsla(),
        ),
        ApprovalReviewStatus::Denied => (
            "Access denied",
            "icons/shield-question.svg",
            theme.error.hsla(),
        ),
        ApprovalReviewStatus::TimedOut => (
            "Review timed out",
            "icons/shield-question.svg",
            theme.text_3.hsla(),
        ),
        ApprovalReviewStatus::Aborted => (
            "Review stopped",
            "icons/shield-question.svg",
            theme.text_3.hsla(),
        ),
    }
}

fn risk_label(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Low => "Low",
        RiskLevel::Medium => "Medium",
        RiskLevel::High => "High",
        RiskLevel::Critical => "Critical",
    }
}

fn radio_mark(active: bool, theme: Theme) -> impl IntoElement {
    div()
        .size(px(14.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .border_1()
        .border_color(if active {
            theme.text.hsla()
        } else {
            theme.text_3.hsla()
        })
        .when(active, |radio| {
            radio.child(div().size(px(6.0)).rounded(px(3.0)).bg(theme.text.hsla()))
        })
}

fn approval_action_button(
    id: usize,
    label: &'static str,
    primary: bool,
    quiet: bool,
    push_right: bool,
    theme: Theme,
    action: Option<UiAction>,
) -> impl IntoElement {
    let enabled = action.is_some();
    div()
        .id(("approval-action", id))
        .h(px(30.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .when(push_right, |button| button.ml_auto())
        .when(!quiet, |button| {
            button
                .border_1()
                .border_color(theme.line_strong.hsla())
                .bg(if primary {
                    theme.text.hsla()
                } else {
                    theme.surface_2.hsla()
                })
        })
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if primary {
            theme.background.hsla()
        } else {
            theme.text_2.hsla()
        })
        .opacity(if enabled { 1.0 } else { 0.46 })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| {
                    style
                        .bg(if primary {
                            theme.text.hsla().opacity(0.88)
                        } else {
                            theme.surface_3.hsla()
                        })
                        .text_color(if primary {
                            theme.background.hsla()
                        } else {
                            theme.text.hsla()
                        })
                })
                .active(|style| style.opacity(0.72))
        })
        .when_some(action, |button, action| {
            button.on_click(move |_event, _window, cx| action(cx))
        })
        .child(label)
}

fn input_nav_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    theme: Theme,
    action: Option<UiAction>,
) -> impl IntoElement {
    let enabled = action.is_some();
    div()
        .id(id)
        .min_w(px(58.0))
        .h(px(32.0))
        .px(px(11.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .border_1()
        .border_color(if primary && enabled {
            theme.text.hsla().opacity(0.54)
        } else {
            theme.line_strong.hsla().opacity(0.7)
        })
        .bg(if primary && enabled {
            theme.text.hsla()
        } else {
            theme.surface_2.hsla()
        })
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if primary && enabled {
            theme.background.hsla()
        } else {
            theme.text_2.hsla()
        })
        .opacity(if enabled { 1.0 } else { 0.38 })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| {
                    style.border_color(theme.line_strong.hsla()).bg(if primary {
                        theme.text.hsla().opacity(0.88)
                    } else {
                        theme.surface_3.hsla()
                    })
                })
                .active(|style| style.opacity(0.72))
        })
        .when_some(action, |button, action| {
            button.on_click(move |_event, _window, cx| action(cx))
        })
        .child(label)
}

fn icon_tool_button(
    id: &'static str,
    icon_path: &'static str,
    label: Option<&'static str>,
    active: bool,
    theme: Theme,
    action: Option<UiAction>,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.0))
        .min_w(px(28.0))
        .px(px(if label.is_some() { 8.0 } else { 6.0 }))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(if active {
            theme.attention.hsla().opacity(0.6)
        } else {
            theme.line.hsla()
        })
        .text_size(px(11.5))
        .text_color(if active {
            theme.attention.hsla()
        } else {
            theme.text_2.hsla()
        })
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(theme.surface_2.hsla())
                .border_color(theme.line_strong.hsla())
                .text_color(theme.text.hsla())
        })
        .when_some(action, |button, action| {
            button.on_click(move |_event, _window, cx| action(cx))
        })
        .child(svg_icon(icon_path, 14.0))
        .when_some(label, |button, label| button.child(label))
}

type UiAction = Rc<dyn Fn(&mut App)>;

fn svg_icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}

fn approval_meta(approval: ApprovalMode) -> (&'static str, &'static str) {
    match approval {
        ApprovalMode::Ask => ("icons/shield-question.svg", "Ask first"),
        ApprovalMode::Auto => ("icons/shield-check.svg", "Auto"),
        ApprovalMode::AutoReview => ("icons/scan-eye.svg", "Auto-review"),
        ApprovalMode::Full => ("icons/lock-open.svg", "Full access"),
    }
}

fn provider_mark(provider: ProviderId, theme: Theme, size: f32) -> AnyElement {
    if provider == ProviderId::Codex {
        return div()
            .text_color(theme.text_3.hsla())
            .child(svg_icon("icons/openai.svg", size))
            .into_any_element();
    }
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .bg(theme.surface_3.hsla())
        .font_family("Geist Mono")
        .text_size(px(8.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text_2.hsla())
        .child(match provider {
            ProviderId::ClaudeCode => "A",
            ProviderId::Cursor => "C",
            ProviderId::OpenCode => "O",
            ProviderId::Acp => "A",
            ProviderId::Api => "↔",
            ProviderId::Codex => unreachable!(),
        })
        .into_any_element()
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().chain(chars).collect()
}

fn path_label(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
        .unwrap_or(path)
}

fn design_phase_label(text: &str) -> Option<&'static str> {
    let text = text.to_ascii_lowercase();
    if text.contains("design:brief") {
        Some("Preparing questions")
    } else if text.contains("design:brand") {
        Some("Creating brand direction")
    } else if text.contains("design:page") {
        Some("Planning the page")
    } else if text.contains("design:assets") {
        Some("Gathering assets")
    } else if text.contains("design:build") {
        Some("Building the website")
    } else if text.contains("design:preview") {
        Some("Starting the preview")
    } else if text.contains("design:review") {
        Some("Reviewing the design")
    } else if text.contains("design:repair") {
        Some("Refining the website")
    } else {
        None
    }
}

fn merge_queued_draft(queued: &str, draft: &str) -> String {
    let draft = draft.trim();
    if draft.is_empty() {
        queued.into()
    } else {
        format!("{queued}\n\n{draft}")
    }
}

fn merge_unique_attachments(current: &mut Vec<ComposerAttachment>, queued: Vec<String>) {
    for path in queued {
        if !current
            .iter()
            .any(|attachment| attachment.path.as_deref() == Some(path.as_str()))
        {
            current.push(ComposerAttachment::file(path));
        }
    }
}

fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"
            )
        })
}

fn pasted_image_extension(image: &Image) -> Option<&'static str> {
    let bytes = image.bytes();
    match image.format() {
        ImageFormat::Png
            if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) =>
        {
            Some("png")
        }
        ImageFormat::Jpeg if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Some("jpg"),
        ImageFormat::Gif if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => {
            Some("gif")
        }
        ImageFormat::Webp
            if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) =>
        {
            Some("webp")
        }
        ImageFormat::Bmp if bytes.starts_with(b"BM") => Some("bmp"),
        _ => None,
    }
}

fn materialize_pasted_image(image: &Image) -> anyhow::Result<PathBuf> {
    let extension =
        pasted_image_extension(image).ok_or_else(|| anyhow::anyhow!("unsupported pasted image"))?;
    if image.bytes().is_empty() || image.bytes().len() > MAX_PASTED_IMAGE_BYTES {
        anyhow::bail!("pasted image is empty or too large");
    }
    let directory = std::env::temp_dir()
        .join("Personal Harness")
        .join("pasted-images");
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let destination = directory.join(format!("pasted-{}.{}", uuid::Uuid::new_v4(), extension));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&destination)?;
    file.write_all(image.bytes())?;
    file.sync_all()?;
    Ok(destination)
}

fn transcript_mode_for_bottom_gap(gap: gpui::Pixels) -> TranscriptScrollMode {
    if gap < px(TRANSCRIPT_BOTTOM_SLACK) {
        TranscriptScrollMode::FollowEnd
    } else {
        TranscriptScrollMode::Free
    }
}

fn format_voice_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

struct TranscriptInsertion {
    text: String,
    cursor: usize,
}

fn insert_transcript_at_cursor(
    current: &str,
    transcript: &str,
    cursor: usize,
) -> Option<TranscriptInsertion> {
    let spoken = transcript.trim();
    if spoken.is_empty() {
        return None;
    }
    let mut position = cursor.min(current.len());
    while !current.is_char_boundary(position) {
        position = position.saturating_sub(1);
    }
    let (before, after) = current.split_at(position);
    let leading = if before
        .chars()
        .next_back()
        .is_some_and(|character| !character.is_whitespace())
    {
        " "
    } else {
        ""
    };
    let trailing = if after
        .chars()
        .next()
        .is_some_and(|character| !character.is_whitespace())
    {
        " "
    } else {
        ""
    };
    let insertion = format!("{leading}{spoken}{trailing}");
    Some(TranscriptInsertion {
        text: format!("{before}{insertion}{after}"),
        cursor: before.len() + insertion.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_insertion_preserves_both_sides_of_the_draft() {
        let inserted = insert_transcript_at_cursor("hello world", " spoken words ", 5).unwrap();
        assert_eq!(inserted.text, "hello spoken words world");
        assert_eq!(inserted.cursor, 18);
    }

    #[test]
    fn queued_prompt_edit_preserves_the_draft_and_deduplicates_attachments() {
        assert_eq!(
            merge_queued_draft("Polish the queue", "  keep this draft  "),
            "Polish the queue\n\nkeep this draft"
        );
        assert_eq!(
            merge_queued_draft("Polish the queue", "  "),
            "Polish the queue"
        );

        let mut attachments = vec![ComposerAttachment::file("/work/existing.png".into())];
        merge_unique_attachments(
            &mut attachments,
            vec!["/work/reference.png".into(), "/work/existing.png".into()],
        );
        assert_eq!(
            attachments
                .iter()
                .filter_map(|attachment| attachment.path.as_deref())
                .collect::<Vec<_>>(),
            ["/work/existing.png", "/work/reference.png"]
        );
    }

    #[test]
    fn voice_duration_uses_the_web_minutes_and_seconds_format() {
        assert_eq!(format_voice_duration(Duration::from_millis(999)), "0:00");
        assert_eq!(format_voice_duration(Duration::from_secs(65)), "1:05");
    }

    #[test]
    fn design_activity_labels_match_the_web_timeline() {
        assert_eq!(
            design_phase_label("design:brand"),
            Some("Creating brand direction")
        );
        assert_eq!(
            design_phase_label("DESIGN:REVIEW"),
            Some("Reviewing the design")
        );
        assert_eq!(design_phase_label("ordinary tool"), None);
    }

    #[test]
    fn pasted_images_require_a_matching_supported_signature() {
        let png = Image::from_bytes(
            ImageFormat::Png,
            vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        );
        let spoofed = Image::from_bytes(ImageFormat::Png, b"not a png".to_vec());
        let svg = Image::from_bytes(ImageFormat::Svg, b"<svg/>".to_vec());

        assert_eq!(pasted_image_extension(&png), Some("png"));
        assert_eq!(pasted_image_extension(&spoofed), None);
        assert_eq!(pasted_image_extension(&svg), None);
    }

    #[test]
    fn attachment_image_detection_is_case_insensitive() {
        assert!(is_image_path("C:\\work\\REFERENCE.PNG"));
        assert!(is_image_path("/work/reference.webp"));
        assert!(!is_image_path("/work/notes.md"));
    }

    #[test]
    fn transcript_bottom_slack_matches_the_web_scroll_contract() {
        assert_eq!(
            transcript_mode_for_bottom_gap(px(79.0)),
            TranscriptScrollMode::FollowEnd
        );
        assert_eq!(
            transcript_mode_for_bottom_gap(px(80.0)),
            TranscriptScrollMode::Free
        );
    }
}
