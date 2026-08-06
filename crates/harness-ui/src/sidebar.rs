use crate::theme::{RAIL_WIDTH, Theme};
use gpui::{FontWeight, Hsla, SharedString, div, prelude::*, px, svg};
use harness_client::ConnectionState;
use harness_protocol::{
    ProjectSummary, ProviderId, SessionSummary, ThreadInboxStatus, ThreadLifecycle,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn sidebar(
    theme: Theme,
    projects: &[ProjectSummary],
    connection: ConnectionState,
    loaded: bool,
    fixture: bool,
) -> impl IntoElement {
    div()
        .w(px(RAIL_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .bg(theme.rail.hsla())
        .border_r_1()
        .border_color(theme.line.hsla())
        .child(sidebar_actions(theme))
        .child(if fixture {
            fixture_sidebar_body(theme).into_any_element()
        } else {
            sidebar_body(theme, projects, connection, loaded).into_any_element()
        })
        .child(
            div()
                .flex_none()
                .border_t_1()
                .border_color(theme.line.hsla())
                .p(px(8.0))
                .child(nav_item(
                    "settings",
                    "icons/settings.svg",
                    "Settings",
                    "Ctrl ,",
                    theme,
                )),
        )
}

fn sidebar_actions(theme: Theme) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .px(px(10.0))
        .pt(px(12.0))
        .pb(px(10.0))
        .child(nav_item(
            "new-chat",
            "icons/plus.svg",
            "New chat",
            "Ctrl N",
            theme,
        ))
        .child(nav_item(
            "new-project",
            "icons/folder-pen.svg",
            "New project",
            "Ctrl Shift O",
            theme,
        ))
        .child(
            div()
                .id("search-chats")
                .mt(px(8.0))
                .h(px(32.0))
                .w_full()
                .flex()
                .items_center()
                .px(px(8.0))
                .rounded(px(8.0))
                .bg(theme.surface.hsla())
                .text_color(theme.text_3.hsla())
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_2.hsla()))
                .child(icon("icons/search.svg", 13.0))
                .child(
                    div()
                        .ml(px(8.0))
                        .flex_1()
                        .text_size(px(11.5))
                        .child("Search chats"),
                )
                .child(
                    div()
                        .font_family("Geist Mono")
                        .text_size(px(10.5))
                        .child("Ctrl Shift F"),
                ),
        )
}

fn fixture_sidebar_body(theme: Theme) -> impl IntoElement {
    div()
        .id("sidebar-scroll")
        .flex_1()
        .overflow_y_scroll()
        .px(px(8.0))
        .pb(px(12.0))
        .child(section_label("Active".into(), theme))
        .child(inbox_row(
            "Finish Sidebar v2".into(),
            "personalharness · Codex · codex/sidebar-v2".into(),
            Status::Working,
            theme,
        ))
        .child(inbox_row(
            "Review lifecycle contract".into(),
            "personalharness · Codex · 9m ago".into(),
            Status::Approval,
            theme,
        ))
        .child(inbox_row(
            "Connect remote desktop".into(),
            "mobile-harness · Claude Code · 20m ago".into(),
            Status::Input,
            theme,
        ))
        .child(inbox_row(
            "Verify macOS terminal behavior".into(),
            "personalharness · Codex · 36m ago".into(),
            Status::Ready,
            theme,
        ))
        .child(inbox_row(
            "Run device smoke test".into(),
            "mobile-harness · Gemini · 1h ago".into(),
            Status::Failed,
            theme,
        ))
        .child(collapsed_group("Projects · 2".into(), None, theme))
        .child(collapsed_group("Snoozed".into(), Some("2".into()), theme))
        .child(collapsed_group("Settled".into(), Some("3".into()), theme))
        .child(settled_row(
            "Persist sidebar settings".into(),
            "personalharness · 40m ago".into(),
            theme,
        ))
        .child(settled_row(
            "Add lifecycle event routing".into(),
            "personalharness · 1h ago".into(),
            theme,
        ))
}

fn sidebar_body(
    theme: Theme,
    projects: &[ProjectSummary],
    connection: ConnectionState,
    loaded: bool,
) -> impl IntoElement {
    let mut active = Vec::new();
    let mut snoozed = Vec::new();
    let mut settled = Vec::new();
    for project in projects {
        for session in &project.sessions {
            match session.lifecycle.as_ref() {
                Some(ThreadLifecycle::Snoozed { .. }) => snoozed.push((project, session)),
                Some(ThreadLifecycle::Settled { .. }) => settled.push((project, session)),
                Some(ThreadLifecycle::Active { .. }) | None => active.push((project, session)),
            }
        }
    }
    active.sort_by(|(_, left), (_, right)| newest_first(left, right));
    snoozed.sort_by(|(_, left), (_, right)| newest_first(left, right));
    settled.sort_by(|(_, left), (_, right)| newest_first(left, right));

    let status = match connection {
        ConnectionState::Connecting => Some("Connecting to server…"),
        ConnectionState::Reconnecting => Some("Reconnecting…"),
        ConnectionState::Closed => Some("Server unavailable"),
        ConnectionState::Open if !loaded => Some("Loading threads…"),
        ConnectionState::Open if projects.is_empty() => Some("Nothing here yet."),
        ConnectionState::Open => None,
    };

    div()
        .id("sidebar-scroll")
        .flex_1()
        .overflow_y_scroll()
        .px(px(8.0))
        .pb(px(12.0))
        .when_some(status, |body, label| {
            body.child(empty_state(SharedString::from(label), theme))
        })
        .when(!active.is_empty(), |body| {
            body.child(section_label("Active".into(), theme))
                .children(active.into_iter().map(|(project, session)| {
                    inbox_row(
                        session.title.clone().into(),
                        session_meta(project, session).into(),
                        status_for(session),
                        theme,
                    )
                }))
        })
        .when(!snoozed.is_empty(), |body| {
            body.child(collapsed_group(
                "Snoozed".into(),
                Some(snoozed.len().to_string().into()),
                theme,
            ))
        })
        .when(!settled.is_empty(), |body| {
            body.child(collapsed_group(
                "Settled".into(),
                Some(settled.len().to_string().into()),
                theme,
            ))
            .children(settled.into_iter().take(10).map(|(project, session)| {
                settled_row(
                    session.title.clone().into(),
                    format!(
                        "{} · {}",
                        project.name,
                        relative_time(settled_at(session).unwrap_or(session.created_at))
                    )
                    .into(),
                    theme,
                )
            }))
        })
}

fn icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}

fn nav_item(
    id: &'static str,
    icon_path: &'static str,
    label: &'static str,
    shortcut: &'static str,
    theme: Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(32.0))
        .w_full()
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(px(8.0))
        .text_color(theme.text_2.hsla())
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(theme.surface_2.hsla())
                .text_color(theme.text.hsla())
        })
        .active(|style| style.opacity(0.72))
        .child(
            div()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_3.hsla())
                .child(icon(icon_path, 14.0)),
        )
        .child(div().ml(px(9.0)).flex_1().text_size(px(13.5)).child(label))
        .child(
            div()
                .font_family("Geist Mono")
                .text_size(px(10.5))
                .text_color(theme.text_3.hsla())
                .child(shortcut),
        )
}

fn section_label(label: SharedString, theme: Theme) -> impl IntoElement {
    div()
        .h(px(31.0))
        .flex()
        .items_end()
        .px(px(8.0))
        .pb(px(7.0))
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text_3.hsla())
        .child(label)
}

#[derive(Clone, Copy)]
enum Status {
    Starting,
    Working,
    Queued,
    Approval,
    Input,
    Ready,
    Failed,
    Idle,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::Working => "Working",
            Self::Queued => "Queued",
            Self::Approval => "Approval",
            Self::Input => "Input",
            Self::Ready => "Ready",
            Self::Failed => "Failed",
            Self::Idle => "Idle",
        }
    }

    fn color(self, theme: Theme) -> Hsla {
        match self {
            Self::Starting | Self::Working => theme.text.hsla(),
            Self::Queued | Self::Idle => theme.text_3.hsla(),
            Self::Approval | Self::Input => theme.attention.hsla(),
            Self::Ready => theme.success.hsla(),
            Self::Failed => theme.error.hsla(),
        }
    }
}

fn inbox_row(
    title: SharedString,
    meta: SharedString,
    status: Status,
    theme: Theme,
) -> impl IntoElement {
    let status_color = status.color(theme);
    div()
        .id(SharedString::from(format!("inbox:{title}")))
        .min_h(px(64.0))
        .w_full()
        .flex()
        .flex_col()
        .justify_center()
        .px(px(8.0))
        .rounded(px(8.0))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface.hsla()))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.response_text.hsla())
                        .child(title),
                )
                .child(
                    div()
                        .ml(px(8.0))
                        .px(px(5.0))
                        .h(px(18.0))
                        .flex()
                        .items_center()
                        .rounded(px(9.0))
                        .bg(status_color.opacity(0.12))
                        .text_color(status_color)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(status.label()),
                ),
        )
        .child(
            div()
                .mt(px(5.0))
                .truncate()
                .text_size(px(11.5))
                .text_color(theme.text_3.hsla())
                .child(meta),
        )
}

fn collapsed_group(
    label: SharedString,
    count: Option<SharedString>,
    theme: Theme,
) -> impl IntoElement {
    div()
        .mt(px(5.0))
        .h(px(39.0))
        .w_full()
        .flex()
        .items_center()
        .border_b_1()
        .border_color(theme.line.hsla())
        .text_color(theme.text_3.hsla())
        .text_size(px(11.5))
        .child(icon("icons/chevron-right.svg", 10.0))
        .child(div().ml(px(4.0)).flex_1().child(label))
        .when_some(count, |row, count| {
            row.child(div().pr(px(6.0)).child(count))
        })
}

fn settled_row(title: SharedString, meta: SharedString, theme: Theme) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("settled:{title}")))
        .min_h(px(44.0))
        .w_full()
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(px(8.0))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface.hsla()))
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(theme.text_2.hsla())
                        .child(title),
                )
                .child(
                    div()
                        .mt(px(2.0))
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(theme.text_3.hsla())
                        .child(meta),
                ),
        )
        .child(
            div()
                .ml(px(6.0))
                .text_color(theme.text_3.hsla())
                .child(icon("icons/check.svg", 11.0)),
        )
}

fn empty_state(label: SharedString, theme: Theme) -> impl IntoElement {
    div()
        .px(px(8.0))
        .py(px(18.0))
        .text_size(px(11.5))
        .text_color(theme.text_3.hsla())
        .child(label)
}

fn status_for(session: &SessionSummary) -> Status {
    match session.status.unwrap_or(if session.running {
        ThreadInboxStatus::Working
    } else {
        ThreadInboxStatus::Idle
    }) {
        ThreadInboxStatus::Starting => Status::Starting,
        ThreadInboxStatus::Working => Status::Working,
        ThreadInboxStatus::Queued => Status::Queued,
        ThreadInboxStatus::Approval => Status::Approval,
        ThreadInboxStatus::Input => Status::Input,
        ThreadInboxStatus::Failed => Status::Failed,
        ThreadInboxStatus::Ready => Status::Ready,
        ThreadInboxStatus::Idle => Status::Idle,
    }
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

fn session_meta(project: &ProjectSummary, session: &SessionSummary) -> String {
    let detail = session
        .worktree_branch
        .as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| relative_time(session.created_at));
    format!(
        "{} · {} · {detail}",
        project.name,
        provider_label(session.provider)
    )
}

fn newest_first(left: &SessionSummary, right: &SessionSummary) -> std::cmp::Ordering {
    right
        .created_at
        .partial_cmp(&left.created_at)
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn settled_at(session: &SessionSummary) -> Option<f64> {
    match session.lifecycle.as_ref() {
        Some(ThreadLifecycle::Settled { settled_at, .. }) => Some(*settled_at as f64),
        _ => None,
    }
}

fn relative_time(timestamp: f64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_millis() as f64);
    let elapsed = ((now - timestamp).max(0.0) / 1_000.0) as u64;
    match elapsed {
        0..=59 => "now".into(),
        60..=3_599 => format!("{}m ago", elapsed / 60),
        3_600..=86_399 => format!("{}h ago", elapsed / 3_600),
        _ => format!("{}d ago", elapsed / 86_400),
    }
}
