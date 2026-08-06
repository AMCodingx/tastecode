use crate::client_state::ChatUpdate;
use crate::theme::{CHAT_WIDTH, Theme};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, ListAlignment, ListState, Render,
    SharedString, Window, div, list, prelude::*, px, relative,
};
use gpui_component::input::{Input, InputEvent, InputState};
use harness_protocol::{
    Item, ItemStatus, ItemType, MessageRole, ProviderId, ThreadEventPush, ThreadQueueResult,
};
use harness_state::{ApplyOutcome, HistoryError, ThreadState};

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
        steer: bool,
    },
    Interrupt {
        thread_id: String,
    },
    Create {
        project_path: String,
        text: String,
    },
}

impl EventEmitter<ChatEvent> for ChatView {}

pub(crate) struct ChatView {
    theme: Theme,
    session: Option<SessionContext>,
    state: ThreadState,
    queue: ThreadQueueResult,
    loading: bool,
    error: Option<String>,
    list_state: ListState,
    composer: Entity<InputState>,
    clear_composer: bool,
    restore_composer: Option<String>,
    creating: bool,
}

impl ChatView {
    pub(crate) fn new(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 11)
                .placeholder("Do anything")
        });
        cx.subscribe(&composer, |this, _composer, event, cx| match event {
            InputEvent::PressEnter { secondary } => this.submit(*secondary, cx),
            InputEvent::Change | InputEvent::Focus | InputEvent::Blur => cx.notify(),
        })
        .detach();

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
            list_state: ListState::new(0, ListAlignment::Bottom, px(500.0)),
            composer,
            clear_composer: false,
            restore_composer: None,
            creating: false,
        }
    }

    pub(crate) fn begin_session(&mut self, session: SessionContext, cx: &mut Context<Self>) {
        self.session = Some(session);
        self.state = ThreadState::default();
        self.queue.items.clear();
        self.queue.can_steer = false;
        self.loading = true;
        self.error = None;
        self.list_state.reset(0);
        self.clear_composer = true;
        self.restore_composer = None;
        self.creating = false;
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
        self.error = None;
        cx.notify();
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
                        if replace {
                            self.list_state.reset(new_len);
                        } else if new_len > old_len {
                            self.list_state.splice(old_len..old_len, new_len - old_len);
                        }
                        self.loading = false;
                        self.error = None;
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
                self.apply_thread_event(push, cx);
            }
            ChatUpdate::Refresh => {
                if let Some(thread_id) = self
                    .session
                    .as_ref()
                    .and_then(|session| session.thread_id.as_ref())
                {
                    cx.emit(ChatEvent::NeedHistory {
                        thread_id: thread_id.clone(),
                        after_seq: Some(self.state.last_seq()),
                    });
                }
            }
            ChatUpdate::Error { thread_id, message } if self.is_selected(&thread_id) => {
                self.loading = false;
                self.error = Some(message);
                cx.notify();
            }
            ChatUpdate::DraftError {
                message,
                restore_text,
            } if self
                .session
                .as_ref()
                .is_some_and(|session| session.thread_id.is_none()) =>
            {
                self.loading = false;
                self.creating = false;
                self.error = Some(message);
                self.restore_composer = Some(restore_text);
                cx.notify();
            }
            ChatUpdate::History { .. }
            | ChatUpdate::Queue { .. }
            | ChatUpdate::Event(_)
            | ChatUpdate::Error { .. }
            | ChatUpdate::DraftError { .. } => {}
        }
    }

    fn apply_thread_event(&mut self, push: ThreadEventPush, cx: &mut Context<Self>) {
        let changed_item = match &push.event {
            harness_protocol::DomainEvent::ItemDelta {
                turn_id, item_id, ..
            }
            | harness_protocol::DomainEvent::ItemCompleted {
                item:
                    Item {
                        turn_id,
                        id: item_id,
                        ..
                    },
            } => Some((turn_id.clone(), item_id.clone())),
            _ => None,
        };
        let old_len = self.state.timeline_len();
        match self.state.apply_live(push.seq, push.event) {
            ApplyOutcome::Applied(changes) => {
                let new_len = self.state.timeline_len();
                if new_len > old_len {
                    self.list_state.splice(old_len..old_len, new_len - old_len);
                } else if changes.transcript
                    && let Some((turn_id, item_id)) = changed_item
                    && let Some(row) = self.state.row_for_item(&turn_id, &item_id)
                {
                    self.list_state.splice(row..row + 1, 1);
                }
                self.loading = false;
                self.error = None;
                cx.notify();
            }
            ApplyOutcome::Duplicate => {}
            ApplyOutcome::NeedsHistory { after_seq } => {
                if let Some(thread_id) = self
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
        }
    }

    fn reconcile_after_error(&mut self, error: HistoryError, cx: &mut Context<Self>) {
        self.error = Some(error.to_string());
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
        self.clear_composer = true;
        match &session.thread_id {
            Some(thread_id) => cx.emit(ChatEvent::Submit {
                thread_id: thread_id.clone(),
                text,
                steer,
            }),
            None => {
                self.creating = true;
                self.loading = true;
                cx.emit(ChatEvent::Create {
                    project_path: session.project_path.clone(),
                    text,
                });
            }
        }
        cx.notify();
    }

    fn primary_action(&mut self, cx: &mut Context<Self>) {
        let has_draft = !self.composer.read(cx).value().trim().is_empty();
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

    fn header(&self) -> impl IntoElement {
        let theme = self.theme;
        let session = self.session.clone();
        div()
            .h(px(44.0))
            .min_h(px(44.0))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(16.0))
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(theme.text_3.hsla())
                    .child(session.as_ref().map_or_else(
                        || SharedString::from(""),
                        |value| value.title.clone().into(),
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
            .child(header_button("Terminal", theme))
            .child(header_button("Review", theme))
    }

    fn timeline(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.loading && self.state.timeline_len() == 0 {
            return centered_label("Loading conversation…", self.theme);
        }
        if let Some(error) = &self.error
            && self.state.timeline_len() == 0
        {
            return centered_label(error.clone(), self.theme);
        }
        if self.state.timeline_len() == 0 {
            return centered_label("Start a conversation in this project.", self.theme);
        }

        let view: Entity<Self> = cx.entity();
        list(self.list_state.clone(), move |row, _window, cx| {
            view.read(cx).render_item(row)
        })
        .size_full()
        .px(px(18.0))
        .into_any_element()
    }

    fn render_item(&self, row: usize) -> AnyElement {
        let Some(item) = self.state.item_at_row(row) else {
            return div().into_any_element();
        };
        let content: SharedString = item.text.clone().unwrap_or_default().into();
        let body = match (item.item_type, item.role) {
            (ItemType::Message, Some(MessageRole::User)) => user_message(content, self.theme),
            (ItemType::Message, _) => assistant_message(content, item.status, self.theme),
            (ItemType::Reasoning, _) => auxiliary_item("Reasoning", content, self.theme),
            (ItemType::Command, _) => auxiliary_item(
                item.command.clone().unwrap_or_else(|| "Command".into()),
                content,
                self.theme,
            ),
            (ItemType::FileChange, _) => auxiliary_item(
                item.path.clone().unwrap_or_else(|| "File changed".into()),
                content,
                self.theme,
            ),
            (ItemType::ToolCall, _) => auxiliary_item("Tool call", content, self.theme),
            (ItemType::Plan, _) => auxiliary_item("Plan", content, self.theme),
            (ItemType::Error, _) => error_item(content, self.theme),
            (ItemType::Unknown, _) => auxiliary_item("Provider event", content, self.theme),
        };

        div()
            .w_full()
            .max_w(px(CHAT_WIDTH + 48.0))
            .mx_auto()
            .pb(px(12.0))
            .child(body)
            .into_any_element()
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let session = self.session.clone();
        let has_draft = !self.composer.read(cx).value().trim().is_empty();
        let running = self.state.running;
        div()
            .flex_none()
            .px(px(24.0))
            .pt(px(4.0))
            .pb(px(12.0))
            .child(
                div()
                    .w_full()
                    .max_w(px(CHAT_WIDTH))
                    .mx_auto()
                    .rounded(px(18.0))
                    .border_1()
                    .border_color(theme.line.hsla())
                    .bg(theme.prompt.hsla())
                    .overflow_hidden()
                    .child(
                        div()
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .px(px(12.0))
                            .bg(theme.surface_2.hsla())
                            .text_size(px(12.0))
                            .text_color(theme.text_2.hsla())
                            .child(session.as_ref().map_or_else(
                                || SharedString::from("Project"),
                                |value| value.project_name.clone().into(),
                            ))
                            .child("·")
                            .child(session.as_ref().map_or_else(
                                || SharedString::from("Provider"),
                                |value| provider_label(value.provider).into(),
                            )),
                    )
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
                            .child(tool_button("+", theme))
                            .child(tool_button("Permissions", theme))
                            .child(tool_button("Model", theme))
                            .child(div().flex_1())
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
                                    .cursor_pointer()
                                    .active(|style| style.opacity(0.72))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.primary_action(cx);
                                    }))
                                    .child(if running && !has_draft { "■" } else { "↑" }),
                            ),
                    ),
            )
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        div()
            .size_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .bg(self.theme.background.hsla())
            .child(self.header())
            .child(div().flex_1().min_h(px(0.0)).child(self.timeline(cx)))
            .when(!self.queue.items.is_empty(), |view| {
                view.child(queue_summary(self.queue.items.len(), self.theme))
            })
            .child(self.composer(cx))
    }
}

fn header_button(label: &'static str, theme: Theme) -> impl IntoElement {
    div()
        .h(px(28.0))
        .px(px(7.0))
        .flex()
        .items_center()
        .rounded(px(7.0))
        .text_size(px(11.5))
        .text_color(theme.text_3.hsla())
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface.hsla()).text_color(theme.text.hsla()))
        .child(label)
}

fn centered_label(label: impl Into<SharedString>, theme: Theme) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(12.5))
        .text_color(theme.text_3.hsla())
        .child(label.into())
        .into_any_element()
}

fn user_message(text: SharedString, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_end()
        .child(
            div()
                .max_w(relative(0.88))
                .px(px(13.0))
                .py(px(9.0))
                .rounded(px(14.0))
                .bg(theme.surface_2.hsla())
                .text_size(px(14.0))
                .line_height(relative(1.52))
                .whitespace_normal()
                .child(text),
        )
        .into_any_element()
}

fn assistant_message(text: SharedString, status: ItemStatus, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .pb(px(if status == ItemStatus::Completed {
            32.0
        } else {
            0.0
        }))
        .text_size(px(14.0))
        .line_height(relative(1.52))
        .text_color(theme.response_text.hsla())
        .whitespace_normal()
        .child(text)
        .into_any_element()
}

fn auxiliary_item(label: impl Into<SharedString>, text: SharedString, theme: Theme) -> AnyElement {
    let text = text.as_str().trim().to_owned();
    div()
        .w_full()
        .rounded(px(9.0))
        .border_1()
        .border_color(theme.line.hsla())
        .bg(theme.surface.hsla())
        .px(px(11.0))
        .py(px(9.0))
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_3.hsla())
                .child(label.into()),
        )
        .when(!text.is_empty(), |element| {
            element.child(
                div()
                    .mt(px(6.0))
                    .font_family("Geist Mono")
                    .text_size(px(11.5))
                    .line_height(relative(1.45))
                    .text_color(theme.text_2.hsla())
                    .whitespace_normal()
                    .child(text),
            )
        })
        .into_any_element()
}

fn error_item(text: SharedString, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .rounded(px(9.0))
        .border_1()
        .border_color(theme.error.hsla().opacity(0.35))
        .bg(theme.error.hsla().opacity(0.08))
        .px(px(11.0))
        .py(px(9.0))
        .text_size(px(12.0))
        .text_color(theme.error.hsla())
        .whitespace_normal()
        .child(text)
        .into_any_element()
}

fn tool_button(label: &'static str, theme: Theme) -> impl IntoElement {
    div()
        .h(px(28.0))
        .px(px(7.0))
        .flex()
        .items_center()
        .rounded(px(7.0))
        .text_size(px(11.5))
        .text_color(theme.text_2.hsla())
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(theme.surface_2.hsla())
                .text_color(theme.text.hsla())
        })
        .child(label)
}

fn queue_summary(count: usize, theme: Theme) -> impl IntoElement {
    div()
        .w_full()
        .max_w(px(CHAT_WIDTH - 56.0))
        .mx_auto()
        .mb(px(-5.0))
        .px(px(12.0))
        .py(px(7.0))
        .rounded_t(px(12.0))
        .border_1()
        .border_color(theme.line.hsla())
        .bg(theme.surface.hsla())
        .text_size(px(11.5))
        .text_color(theme.text_3.hsla())
        .child(format!(
            "{count} queued prompt{}",
            if count == 1 { "" } else { "s" }
        ))
}

fn provider_label(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "Codex",
        ProviderId::ClaudeCode => "Claude Code",
        ProviderId::Cursor => "Cursor",
        ProviderId::OpenCode => "OpenCode",
        ProviderId::Acp => "ACP",
        ProviderId::Api => "API",
    }
}
