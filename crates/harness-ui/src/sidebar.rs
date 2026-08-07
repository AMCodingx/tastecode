use crate::shortcuts::{NEW_CHAT, NEW_PROJECT, SETTINGS, label as shortcut_label};
use crate::theme::{RADIUS_MD, RAIL_WIDTH, Theme};
use crate::zoom::px;
use chrono::{DateTime, Datelike, Local};
use gpui::{
    AnyElement, App, FontWeight, Hsla, Pixels, Point, SharedString, div, prelude::*, relative, svg,
};
use harness_client::ConnectionState;
use harness_protocol::{
    ProjectSummary, ProviderId, SessionSummary, SidebarMode, ThreadInboxStatus, ThreadLifecycle,
    UsageLimit,
};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) type SelectSession = Rc<dyn Fn(String, &mut App)>;
pub(crate) type SidebarAction = Rc<dyn Fn(&mut App)>;
pub(crate) type SelectScope = Rc<dyn Fn(Option<String>, &mut App)>;
pub(crate) type ProjectAction = Rc<dyn Fn(String, &mut App)>;
pub(crate) type OpenSidebarMenu = Rc<dyn Fn(SidebarMenuRequest, Point<Pixels>, &mut App)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SidebarMenuRequest {
    Project(String),
    Thread(String),
    Snooze(String),
}

#[derive(Clone)]
pub(crate) struct SidebarActions {
    pub(crate) select_session: SelectSession,
    pub(crate) new_chat: SidebarAction,
    pub(crate) new_project: SidebarAction,
    pub(crate) open_search: SidebarAction,
    pub(crate) open_settings: SidebarAction,
    pub(crate) toggle_scope: SidebarAction,
    pub(crate) select_scope: SelectScope,
    pub(crate) toggle_project: ProjectAction,
    pub(crate) toggle_project_sessions: ProjectAction,
    pub(crate) new_chat_in_project: ProjectAction,
    pub(crate) settle_thread: ProjectAction,
    pub(crate) open_menu: OpenSidebarMenu,
    pub(crate) toggle_snoozed: SidebarAction,
    pub(crate) toggle_settled: SidebarAction,
    pub(crate) toggle_account: SidebarAction,
    pub(crate) panic_stop: SidebarAction,
}

pub(crate) struct SidebarProps<'a> {
    pub(crate) theme: Theme,
    pub(crate) projects: &'a [ProjectSummary],
    pub(crate) connection: ConnectionState,
    pub(crate) loaded: bool,
    pub(crate) fixture: bool,
    pub(crate) mode: SidebarMode,
    pub(crate) selected_thread_id: Option<&'a str>,
    pub(crate) selected_scope: Option<&'a str>,
    pub(crate) scope_open: bool,
    pub(crate) new_thread_picker: bool,
    pub(crate) collapsed_projects: &'a std::collections::HashSet<String>,
    pub(crate) expanded_project_sessions: &'a std::collections::HashSet<String>,
    pub(crate) snoozed_expanded: bool,
    pub(crate) settled_expanded: bool,
    pub(crate) account_menu_open: bool,
    pub(crate) provider_name: &'a str,
    pub(crate) usage_limits: &'a [UsageLimit],
    pub(crate) panic_stopping: bool,
    pub(crate) glass: u8,
}

pub fn sidebar(props: SidebarProps<'_>, actions: SidebarActions) -> impl IntoElement {
    let SidebarProps {
        theme,
        projects,
        connection,
        loaded,
        fixture,
        mode,
        selected_thread_id,
        selected_scope,
        scope_open,
        new_thread_picker,
        collapsed_projects,
        expanded_project_sessions,
        snoozed_expanded,
        settled_expanded,
        account_menu_open,
        provider_name,
        usage_limits,
        panic_stopping,
        glass,
    } = props;
    div()
        .relative()
        .w(px(RAIL_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .bg(theme.rail.hsla().opacity(rail_opacity(glass)))
        .border_r_1()
        .border_color(if glass == 0 {
            theme.line.hsla()
        } else {
            gpui::white().opacity(0.04 + f32::from(glass.min(60)) / 1_000.0)
        })
        .child(if mode == SidebarMode::Inbox {
            sidebar_actions(
                theme,
                projects,
                selected_scope,
                scope_open,
                new_thread_picker,
                &actions,
            )
            .into_any_element()
        } else {
            classic_sidebar_actions(theme, &actions).into_any_element()
        })
        .child(if fixture {
            fixture_sidebar_body(theme).into_any_element()
        } else if mode == SidebarMode::Classic {
            classic_sidebar_body(
                theme,
                projects,
                connection,
                loaded,
                selected_thread_id,
                collapsed_projects,
                expanded_project_sessions,
                actions.clone(),
            )
            .into_any_element()
        } else {
            sidebar_body(
                theme,
                projects,
                connection,
                loaded,
                selected_thread_id,
                selected_scope,
                snoozed_expanded,
                settled_expanded,
                actions.clone(),
            )
            .into_any_element()
        })
        .child(sidebar_footer(
            theme,
            mode,
            provider_name,
            usage_limits,
            account_menu_open,
            panic_stopping,
            &actions,
        ))
}

fn rail_opacity(glass: u8) -> f32 {
    (1.0 - f32::from(glass.min(60)) * 0.013).max(0.0)
}

fn sidebar_footer(
    theme: Theme,
    mode: SidebarMode,
    provider_name: &str,
    usage_limits: &[UsageLimit],
    account_menu_open: bool,
    panic_stopping: bool,
    actions: &SidebarActions,
) -> AnyElement {
    if mode == SidebarMode::Inbox {
        return div()
            .flex_none()
            .border_t_1()
            .border_color(theme.line.hsla())
            .p(px(8.0))
            .child(nav_item(
                "settings",
                "icons/settings.svg",
                "Settings",
                shortcut_label(SETTINGS),
                theme,
                Some(actions.open_settings.clone()),
            ))
            .into_any_element();
    }

    let toggle = actions.toggle_account.clone();
    let initial = provider_name
        .chars()
        .find(|character| character.is_alphanumeric())
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "H".into());
    div()
        .relative()
        .flex_none()
        .border_t_1()
        .border_color(theme.line.hsla())
        .px(px(10.0))
        .pt(px(8.0))
        .pb(px(10.0))
        .when(account_menu_open, |footer| {
            footer.child(
                div()
                    .id("account-menu")
                    .occlude()
                    .absolute()
                    .left(px(10.0))
                    .right(px(10.0))
                    .bottom(px(53.0))
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(theme.line_strong.hsla())
                    .bg(theme.surface_2.hsla())
                    .shadow_lg()
                    .p(px(5.0))
                    .child(account_limits(provider_name, usage_limits, theme))
                    .child(footer_menu_action(
                        "account-stop-all",
                        "icons/octagon-x.svg",
                        if panic_stopping {
                            "Stopping sessions…"
                        } else {
                            "Stop all sessions"
                        },
                        true,
                        panic_stopping,
                        theme,
                        actions.panic_stop.clone(),
                    ))
                    .child(footer_menu_action(
                        "account-settings",
                        "icons/settings.svg",
                        "Settings",
                        false,
                        false,
                        theme,
                        actions.open_settings.clone(),
                    )),
            )
        })
        .child(
            div()
                .id("account-trigger")
                .h(px(38.0))
                .w_full()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(if account_menu_open {
                    theme.line_strong.hsla()
                } else {
                    theme.line.hsla()
                })
                .bg(theme.surface.hsla())
                .text_size(px(12.0))
                .text_color(theme.text_2.hsla())
                .cursor_pointer()
                .hover(move |style| {
                    style
                        .bg(theme.surface_2.hsla())
                        .border_color(theme.line_strong.hsla())
                        .text_color(theme.text.hsla())
                })
                .active(|style| style.top(px(1.0)))
                .on_click(move |_event, _window, cx| toggle(cx))
                .child(
                    div()
                        .size(px(22.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(11.0))
                        .bg(theme.surface_3.hsla())
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(initial),
                )
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .child(provider_name.to_owned()),
                )
                .child(icon("icons/chevron-down.svg", 11.0)),
        )
        .into_any_element()
}

fn account_limits(provider_name: &str, limits: &[UsageLimit], theme: Theme) -> AnyElement {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64() * 1_000.0);
    div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .px(px(9.0))
        .pt(px(7.0))
        .pb(px(9.0))
        .mb(px(4.0))
        .border_b_1()
        .border_color(theme.line.hsla())
        .text_size(px(11.0))
        .text_color(theme.text_2.hsla())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(12.0))
                .child(icon("icons/gauge.svg", 14.0))
                .child("Limits"),
        )
        .when(limits.is_empty(), |usage| {
            usage.child(
                div()
                    .text_color(theme.text_3.hsla())
                    .child(format!("{provider_name} reports no limits")),
            )
        })
        .children(limits.iter().map(|limit| usage_limit(limit, now_ms, theme)))
        .into_any_element()
}

fn usage_limit(limit: &UsageLimit, now_ms: f64, theme: Theme) -> AnyElement {
    let used = limit.used_percent.clamp(0.0, 100.0) as f32;
    let left = (100.0 - limit.used_percent).round().clamp(0.0, 100.0) as u8;
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .text_color(theme.text.hsla())
                        .child(limit.label.clone()),
                )
                .child(format!("{left}% left")),
        )
        .child(
            div()
                .h(px(4.0))
                .w_full()
                .overflow_hidden()
                .rounded(px(2.0))
                .bg(theme.surface_3.hsla())
                .child(
                    div()
                        .h_full()
                        .w(relative(used / 100.0))
                        .rounded(px(2.0))
                        .bg(theme.attention.hsla()),
                ),
        )
        .when_some(limit.resets_at, |window, resets_at| {
            window.child(
                div()
                    .text_size(px(10.0))
                    .text_color(theme.text_3.hsla())
                    .child(format!("Resets {}", reset_label(resets_at, now_ms))),
            )
        })
        .into_any_element()
}

fn reset_label(timestamp_ms: f64, now_ms: f64) -> String {
    let timestamp = timestamp_ms.round() as i64;
    DateTime::from_timestamp_millis(timestamp).map_or_else(
        || "later".into(),
        |date| {
            let local = date.with_timezone(&Local);
            if timestamp_ms - now_ms < 6.0 * 86_400_000.0 {
                local.format("%a %H:%M").to_string()
            } else {
                format!("{} {}", local.format("%b"), local.day())
            }
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn footer_menu_action(
    id: &'static str,
    icon_path: &'static str,
    label: &'static str,
    destructive: bool,
    disabled: bool,
    theme: Theme,
    action: SidebarAction,
) -> AnyElement {
    div()
        .id(id)
        .h(px(31.0))
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .rounded(px(7.0))
        .text_size(px(11.5))
        .text_color(if destructive {
            theme.error.hsla()
        } else {
            theme.text_2.hsla()
        })
        .opacity(if disabled { 0.5 } else { 1.0 })
        .when(!disabled, |row| {
            row.cursor_pointer()
                .hover(move |style| style.bg(theme.surface_3.hsla()))
                .on_click(move |_event, _window, cx| action(cx))
        })
        .child(icon(icon_path, 14.0))
        .child(label)
        .into_any_element()
}

fn classic_sidebar_actions(theme: Theme, actions: &SidebarActions) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .px(px(10.0))
        .pt(px(12.0))
        .pb(px(8.0))
        .child(nav_item(
            "new-chat",
            "icons/square-pen.svg",
            "New chat",
            shortcut_label(NEW_CHAT),
            theme,
            Some(actions.new_chat.clone()),
        ))
        .child(nav_item(
            "new-project",
            "icons/folder-pen.svg",
            "New project",
            shortcut_label(NEW_PROJECT),
            theme,
            Some(actions.new_project.clone()),
        ))
        .child(
            div()
                .mt(px(7.0))
                .h(px(26.0))
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .id("classic-search-chats")
                        .size(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.0))
                        .text_color(theme.text_3.hsla())
                        .cursor_pointer()
                        .hover(move |style| {
                            style
                                .bg(theme.surface_2.hsla())
                                .text_color(theme.text.hsla())
                        })
                        .on_click({
                            let open_search = actions.open_search.clone();
                            move |_event, _window, cx| open_search(cx)
                        })
                        .child(icon("icons/search.svg", 14.0)),
                ),
        )
}

fn sidebar_actions(
    theme: Theme,
    projects: &[ProjectSummary],
    selected_scope: Option<&str>,
    scope_open: bool,
    _new_thread_picker: bool,
    actions: &SidebarActions,
) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .px(px(10.0))
        .pt(px(10.0))
        .pb(px(9.0))
        .border_b_1()
        .border_color(theme.line.hsla())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .id("search-chats")
                        .h(px(32.0))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(7.0))
                        .px(px(9.0))
                        .rounded(px(RADIUS_MD))
                        .border_1()
                        .border_color(theme.line_strong.hsla())
                        .bg(theme.background.hsla())
                        .text_color(theme.text_3.hsla())
                        .cursor_pointer()
                        .hover(move |style| {
                            style
                                .border_color(theme.text_3.hsla().opacity(0.65))
                                .text_color(theme.text.hsla())
                        })
                        .on_click({
                            let open_search = actions.open_search.clone();
                            move |_event, _window, cx| open_search(cx)
                        })
                        .child(icon("icons/search.svg", 14.0))
                        .child(
                            div()
                                .min_w(px(0.0))
                                .flex_1()
                                .text_size(px(11.5))
                                .child("Search threads"),
                        ),
                )
                .child(
                    div()
                        .id("new-chat")
                        .h(px(34.0))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(8.0))
                        .rounded(px(RADIUS_MD))
                        .text_size(px(13.5))
                        .text_color(theme.text.hsla())
                        .cursor_pointer()
                        .hover(move |style| style.bg(theme.surface_2.hsla()))
                        .active(|style| style.opacity(0.82).top(px(1.0)))
                        .on_click({
                            let new_chat = actions.new_chat.clone();
                            move |_event, _window, cx| new_chat(cx)
                        })
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.text_2.hsla())
                                .child(icon("icons/square-pen.svg", 16.0)),
                        )
                        .child("New chat"),
                ),
        )
        .child(
            div()
                .mt(px(1.0))
                .h(px(29.0))
                .w_full()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(scope_control(
                    theme,
                    projects,
                    selected_scope,
                    scope_open,
                    actions.toggle_scope.clone(),
                    actions.select_scope.clone(),
                ))
                .child(
                    div()
                        .id("new-project")
                        .h(px(29.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .rounded(px(RADIUS_MD))
                        .border_1()
                        .border_color(theme.line_strong.hsla())
                        .text_size(px(10.5))
                        .text_color(theme.text_3.hsla())
                        .cursor_pointer()
                        .hover(move |style| {
                            style
                                .border_color(theme.text_3.hsla().opacity(0.72))
                                .text_color(theme.text.hsla())
                        })
                        .active(|style| style.opacity(0.76).top(px(1.0)))
                        .on_click({
                            let new_project = actions.new_project.clone();
                            move |_event, _window, cx| new_project(cx)
                        })
                        .child(icon("icons/folder-plus.svg", 13.0))
                        .child("Add Project"),
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
            "fixture-working".into(),
            "Finish Sidebar v2".into(),
            "personalharness · Codex · codex/sidebar-v2".into(),
            Status::Working,
            theme,
            false,
            None,
            None,
            None,
        ))
        .child(inbox_row(
            "fixture-approval".into(),
            "Review lifecycle contract".into(),
            "personalharness · Codex · 9m ago".into(),
            Status::Approval,
            theme,
            false,
            None,
            None,
            None,
        ))
        .child(inbox_row(
            "fixture-input".into(),
            "Connect remote desktop".into(),
            "mobile-harness · Claude Code · 20m ago".into(),
            Status::Input,
            theme,
            false,
            None,
            None,
            None,
        ))
        .child(inbox_row(
            "fixture-ready".into(),
            "Verify macOS terminal behavior".into(),
            "personalharness · Codex · 36m ago".into(),
            Status::Ready,
            theme,
            false,
            None,
            None,
            None,
        ))
        .child(inbox_row(
            "fixture-failed".into(),
            "Run device smoke test".into(),
            "mobile-harness · Gemini · 1h ago".into(),
            Status::Failed,
            theme,
            false,
            None,
            None,
            None,
        ))
        .child(collapsed_group(
            "Projects · 2".into(),
            None,
            false,
            theme,
            None,
        ))
        .child(collapsed_group(
            "Snoozed".into(),
            Some("2".into()),
            false,
            theme,
            None,
        ))
        .child(collapsed_group(
            "Settled".into(),
            Some("3".into()),
            true,
            theme,
            None,
        ))
        .child(settled_row(
            "fixture-settled-1".into(),
            "Persist sidebar settings".into(),
            "personalharness · 40m ago".into(),
            theme,
            false,
            None,
            None,
        ))
        .child(settled_row(
            "fixture-settled-2".into(),
            "Add lifecycle event routing".into(),
            "personalharness · 1h ago".into(),
            theme,
            false,
            None,
            None,
        ))
}

#[allow(clippy::too_many_arguments)]
fn sidebar_body(
    theme: Theme,
    projects: &[ProjectSummary],
    connection: ConnectionState,
    loaded: bool,
    selected_thread_id: Option<&str>,
    selected_scope: Option<&str>,
    snoozed_expanded: bool,
    settled_expanded: bool,
    actions: SidebarActions,
) -> impl IntoElement {
    let mut active = Vec::new();
    let mut snoozed = Vec::new();
    let mut settled = Vec::new();
    for project in projects {
        if selected_scope.is_some_and(|scope| scope != project.path) {
            continue;
        }
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
                        session.id.clone().into(),
                        session.title.clone().into(),
                        session_meta(project, session).into(),
                        status_for(session),
                        theme,
                        selected_thread_id == Some(session.id.as_str()),
                        Some(actions.select_session.clone()),
                        Some(actions.open_menu.clone()),
                        Some(actions.settle_thread.clone()),
                    )
                }))
        })
        .when(!snoozed.is_empty(), |body| {
            body.child(collapsed_group(
                "Snoozed".into(),
                Some(snoozed.len().to_string().into()),
                snoozed_expanded,
                theme,
                Some(actions.toggle_snoozed.clone()),
            ))
            .when(snoozed_expanded, |body| {
                body.children(snoozed.into_iter().take(10).map(|(project, session)| {
                    settled_row(
                        session.id.clone().into(),
                        session.title.clone().into(),
                        format!(
                            "{} · wakes {}",
                            project.name,
                            wake_label(session).unwrap_or_else(|| "later".into())
                        )
                        .into(),
                        theme,
                        selected_thread_id == Some(session.id.as_str()),
                        Some(actions.select_session.clone()),
                        Some(actions.open_menu.clone()),
                    )
                }))
            })
        })
        .when(!settled.is_empty(), |body| {
            body.child(collapsed_group(
                "Settled".into(),
                Some(settled.len().to_string().into()),
                settled_expanded,
                theme,
                Some(actions.toggle_settled.clone()),
            ))
            .when(settled_expanded, |body| {
                body.children(settled.into_iter().take(10).map(|(project, session)| {
                    settled_row(
                        session.id.clone().into(),
                        session.title.clone().into(),
                        format!(
                            "{} · {}",
                            project.name,
                            relative_time(settled_at(session).unwrap_or(session.created_at))
                        )
                        .into(),
                        theme,
                        selected_thread_id == Some(session.id.as_str()),
                        Some(actions.select_session.clone()),
                        Some(actions.open_menu.clone()),
                    )
                }))
            })
        })
}

fn icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}

fn nav_item(
    id: &'static str,
    icon_path: &'static str,
    label: &'static str,
    shortcut: impl Into<SharedString>,
    theme: Theme,
    action: Option<SidebarAction>,
) -> impl IntoElement {
    let shortcut = shortcut.into();
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
        .when_some(action, |item, action| {
            item.on_click(move |_event, _window, cx| action(cx))
        })
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

#[allow(clippy::too_many_arguments)]
fn classic_sidebar_body(
    theme: Theme,
    projects: &[ProjectSummary],
    connection: ConnectionState,
    loaded: bool,
    selected_thread_id: Option<&str>,
    collapsed_projects: &std::collections::HashSet<String>,
    expanded_project_sessions: &std::collections::HashSet<String>,
    actions: SidebarActions,
) -> impl IntoElement {
    let mut pinned_sessions = projects
        .iter()
        .flat_map(|project| {
            project
                .sessions
                .iter()
                .filter(|session| session.pinned)
                .map(move |session| (project, session))
        })
        .collect::<Vec<_>>();
    pinned_sessions.sort_by(|(_, left), (_, right)| newest_first(left, right));
    let mut ordered_projects = projects.iter().collect::<Vec<_>>();
    ordered_projects.sort_by_key(|project| !project.pinned);
    let status = match connection {
        ConnectionState::Connecting => Some("Connecting to server…"),
        ConnectionState::Reconnecting => Some("Reconnecting…"),
        ConnectionState::Closed => Some("Server unavailable"),
        ConnectionState::Open if !loaded => Some("Loading projects…"),
        ConnectionState::Open if projects.is_empty() => Some("Nothing here yet."),
        ConnectionState::Open => None,
    };

    div()
        .id("classic-sidebar-scroll")
        .flex_1()
        .overflow_y_scroll()
        .px(px(8.0))
        .pb(px(12.0))
        .when_some(status, |body, label| {
            body.child(empty_state(SharedString::from(label), theme))
        })
        .when(!pinned_sessions.is_empty(), |body| {
            body.child(section_label("Pinned".into(), theme)).children(
                pinned_sessions.into_iter().map(|(project, session)| {
                    classic_session_row(
                        project,
                        session,
                        selected_thread_id == Some(session.id.as_str()),
                        true,
                        theme,
                        &actions,
                    )
                }),
            )
        })
        .child(section_label("Projects".into(), theme))
        .children(ordered_projects.into_iter().map(|project| {
            classic_project(
                project,
                selected_thread_id,
                !collapsed_projects.contains(&project.path),
                expanded_project_sessions.contains(&project.path),
                theme,
                &actions,
            )
        }))
}

fn classic_project(
    project: &ProjectSummary,
    selected_thread_id: Option<&str>,
    expanded: bool,
    show_all: bool,
    theme: Theme,
    actions: &SidebarActions,
) -> AnyElement {
    const COLLAPSED_SESSION_COUNT: usize = 5;
    let path = project.path.clone();
    let toggle_path = path.clone();
    let menu_path = path.clone();
    let new_chat_path = path.clone();
    let show_path = path.clone();
    let sessions = project
        .sessions
        .iter()
        .filter(|session| !session.pinned)
        .collect::<Vec<_>>();
    let has_more = sessions.len() > COLLAPSED_SESSION_COUNT;
    let visible = if show_all {
        sessions.as_slice()
    } else {
        &sessions[..sessions.len().min(COLLAPSED_SESSION_COUNT)]
    };
    let header = div()
        .id(SharedString::from(format!(
            "classic-project:{}",
            project.path
        )))
        .group("classic-project")
        .h(px(34.0))
        .w_full()
        .flex()
        .items_center()
        .rounded(px(8.0))
        .hover(move |style| style.bg(theme.surface.hsla()))
        .on_click({
            let toggle = actions.toggle_project.clone();
            let open_menu = actions.open_menu.clone();
            move |event, _window, cx| {
                if event.is_right_click() {
                    cx.stop_propagation();
                    open_menu(
                        SidebarMenuRequest::Project(menu_path.clone()),
                        event.position(),
                        cx,
                    );
                } else if event.standard_click() {
                    toggle(toggle_path.clone(), cx);
                }
            }
        })
        .child(
            div()
                .ml(px(7.0))
                .text_color(theme.text_3.hsla())
                .child(icon(
                    if expanded {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    },
                    10.0,
                )),
        )
        .child(
            div()
                .ml(px(6.0))
                .size(px(14.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_3.hsla())
                .child(icon("icons/folder-pen.svg", 12.0)),
        )
        .child(
            div()
                .ml(px(7.0))
                .min_w(px(0.0))
                .flex_1()
                .truncate()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_2.hsla())
                .child(project.name.clone()),
        )
        .child(sidebar_menu_button(
            format!("project-menu:{}", project.path).into(),
            SidebarMenuRequest::Project(path),
            theme,
            actions.open_menu.clone(),
        ))
        .child(
            div()
                .id(SharedString::from(format!("new-chat:{}", project.path)))
                .mr(px(4.0))
                .size(px(25.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .text_color(theme.text_3.hsla())
                .opacity(0.0)
                .group_hover("classic-project", |button| button.opacity(1.0))
                .cursor_pointer()
                .hover(move |style| {
                    style
                        .bg(theme.surface_2.hsla())
                        .text_color(theme.text.hsla())
                })
                .on_click({
                    let new_chat = actions.new_chat_in_project.clone();
                    move |_event, _window, cx| {
                        cx.stop_propagation();
                        new_chat(new_chat_path.clone(), cx);
                    }
                })
                .child(icon("icons/plus.svg", 12.0)),
        );

    div()
        .w_full()
        .flex()
        .flex_col()
        .child(header)
        .when(expanded, |group| {
            group
                .children(visible.iter().map(|session| {
                    classic_session_row(
                        project,
                        session,
                        selected_thread_id == Some(session.id.as_str()),
                        false,
                        theme,
                        actions,
                    )
                }))
                .when(has_more, |group| {
                    group.child(
                        div()
                            .id(SharedString::from(format!("show-project:{}", project.path)))
                            .ml(px(28.0))
                            .h(px(27.0))
                            .flex()
                            .items_center()
                            .px(px(8.0))
                            .rounded(px(6.0))
                            .text_size(px(10.5))
                            .text_color(theme.text_3.hsla())
                            .cursor_pointer()
                            .hover(move |style| style.text_color(theme.text.hsla()))
                            .on_click({
                                let toggle = actions.toggle_project_sessions.clone();
                                move |_event, _window, cx| toggle(show_path.clone(), cx)
                            })
                            .child(if show_all { "Show less" } else { "Show more" }),
                    )
                })
        })
        .into_any_element()
}

fn classic_session_row(
    project: &ProjectSummary,
    session: &SessionSummary,
    active: bool,
    standalone: bool,
    theme: Theme,
    actions: &SidebarActions,
) -> AnyElement {
    let thread_id = session.id.clone();
    let select_id = thread_id.clone();
    let menu_id = thread_id.clone();
    let left = if standalone { 5.0 } else { 28.0 };
    div()
        .id(SharedString::from(format!(
            "classic-session:{}",
            session.id
        )))
        .group("classic-session")
        .ml(px(left))
        .h(px(34.0))
        .flex()
        .items_center()
        .px(px(7.0))
        .rounded(px(7.0))
        .when(active, |row| row.bg(theme.surface_2.hsla()))
        .hover(move |style| style.bg(theme.surface.hsla()))
        .cursor_pointer()
        .on_click({
            let select = actions.select_session.clone();
            let open_menu = actions.open_menu.clone();
            move |event, _window, cx| {
                if event.is_right_click() {
                    cx.stop_propagation();
                    open_menu(
                        SidebarMenuRequest::Thread(menu_id.clone()),
                        event.position(),
                        cx,
                    );
                } else if event.standard_click() {
                    select(select_id.clone(), cx);
                }
            }
        })
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .truncate()
                .text_size(px(11.5))
                .text_color(if active {
                    theme.text.hsla()
                } else {
                    theme.text_2.hsla()
                })
                .child(session.title.clone()),
        )
        .child(
            div()
                .ml(px(5.0))
                .text_size(px(9.5))
                .text_color(status_for(session).color(theme))
                .child(status_for(session).label()),
        )
        .child(sidebar_menu_button(
            format!("thread-menu:{}:{}", project.path, session.id).into(),
            SidebarMenuRequest::Thread(thread_id),
            theme,
            actions.open_menu.clone(),
        ))
        .into_any_element()
}

fn sidebar_menu_button(
    id: SharedString,
    request: SidebarMenuRequest,
    theme: Theme,
    open_menu: OpenSidebarMenu,
) -> AnyElement {
    div()
        .id(id)
        .ml(px(4.0))
        .size(px(25.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text_3.hsla())
        .opacity(0.62)
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(theme.surface_2.hsla())
                .text_color(theme.text.hsla())
                .opacity(1.0)
        })
        .on_click(move |event, _window, cx| {
            cx.stop_propagation();
            open_menu(request.clone(), event.position(), cx);
        })
        .child("•••")
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn scope_control(
    theme: Theme,
    projects: &[ProjectSummary],
    selected_scope: Option<&str>,
    open: bool,
    on_toggle: SidebarAction,
    on_select: SelectScope,
) -> impl IntoElement {
    let selected_name = selected_scope
        .and_then(|path| projects.iter().find(|project| project.path == path))
        .map_or_else(
            || SharedString::from("All projects"),
            |project| SharedString::from(project.name.clone()),
        );
    let all_action = on_select.clone();

    div()
        .relative()
        .min_w(px(0.0))
        .flex_1()
        .flex()
        .flex_col()
        .child(
            div()
                .id("sidebar-scope")
                .h(px(29.0))
                .w_full()
                .flex()
                .items_center()
                .min_w(px(0.0))
                .px(px(8.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(if open {
                    theme.text_3.hsla()
                } else {
                    theme.line_strong.hsla()
                })
                .bg(theme.background.hsla())
                .text_size(px(11.5))
                .text_color(theme.text_2.hsla())
                .cursor_pointer()
                .hover(move |style| {
                    style
                        .border_color(theme.text_3.hsla().opacity(0.72))
                        .text_color(theme.text.hsla())
                })
                .on_click(move |_event, _window, cx| on_toggle(cx))
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .child(selected_name),
                )
                .child(
                    div()
                        .ml(px(5.0))
                        .text_color(theme.text_3.hsla())
                        .child(icon("icons/chevron-down.svg", 11.0)),
                ),
        )
        .when(open, |control| {
            control.child(
                div()
                    .id("scope-options")
                    .occlude()
                    .absolute()
                    .top(px(33.0))
                    .left(px(0.0))
                    .right(px(0.0))
                    .max_h(px(220.0))
                    .overflow_y_scroll()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(theme.line_strong.hsla())
                    .bg(theme.surface_2.hsla())
                    .p(px(4.0))
                    .child(scope_option(
                        "scope:all".into(),
                        "All projects".into(),
                        selected_scope.is_none(),
                        theme,
                        Rc::new(move |cx| all_action(None, cx)),
                    ))
                    .children(projects.iter().map(|project| {
                        let path = project.path.clone();
                        let option_action = on_select.clone();
                        scope_option(
                            format!("scope:{}", project.path).into(),
                            project.name.clone().into(),
                            selected_scope == Some(project.path.as_str()),
                            theme,
                            Rc::new(move |cx| option_action(Some(path.clone()), cx)),
                        )
                    })),
            )
        })
}

fn scope_option(
    id: SharedString,
    label: SharedString,
    selected: bool,
    theme: Theme,
    action: SidebarAction,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(29.0))
        .w_full()
        .flex()
        .items_center()
        .px(px(7.0))
        .rounded(px(6.0))
        .when(selected, |item| item.bg(theme.surface_3.hsla()))
        .text_size(px(11.5))
        .text_color(if selected {
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
        .on_click(move |_event, _window, cx| action(cx))
        .child(div().min_w(px(0.0)).flex_1().truncate().child(label))
        .when(selected, |item| item.child(icon("icons/check.svg", 11.0)))
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

#[allow(clippy::too_many_arguments)]
fn inbox_row(
    thread_id: SharedString,
    title: SharedString,
    meta: SharedString,
    status: Status,
    theme: Theme,
    active: bool,
    on_select: Option<SelectSession>,
    open_menu: Option<OpenSidebarMenu>,
    settle: Option<ProjectAction>,
) -> impl IntoElement {
    let status_color = status.color(theme);
    let event_thread_id = thread_id.clone();
    let context_thread_id = thread_id.clone();
    let context_menu = open_menu.clone();
    let can_hide = matches!(status, Status::Ready | Status::Failed | Status::Idle);
    div()
        .id(SharedString::from(format!("inbox:{thread_id}")))
        .min_h(px(64.0))
        .w_full()
        .flex()
        .flex_col()
        .justify_center()
        .px(px(8.0))
        .rounded(px(8.0))
        .when(active, |row| row.bg(theme.surface_2.hsla()))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface.hsla()))
        .on_click(move |event, _window, cx| {
            if event.is_right_click() {
                if let Some(open_menu) = &context_menu {
                    cx.stop_propagation();
                    open_menu(
                        SidebarMenuRequest::Thread(context_thread_id.to_string()),
                        event.position(),
                        cx,
                    );
                }
            } else if event.standard_click()
                && let Some(handler) = &on_select
            {
                handler(event_thread_id.to_string(), cx);
            }
        })
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
                .when_some(
                    can_hide.then_some((open_menu.clone(), settle)),
                    |line, (open_menu, settle)| {
                        line.when_some(open_menu, |line, open_menu| {
                            line.child(sidebar_menu_button(
                                format!("snooze-menu:{thread_id}").into(),
                                SidebarMenuRequest::Snooze(thread_id.to_string()),
                                theme,
                                open_menu,
                            ))
                        })
                        .when_some(settle, |line, settle| {
                            let id = thread_id.to_string();
                            line.child(
                                div()
                                    .id(SharedString::from(format!("settle:{thread_id}")))
                                    .ml(px(2.0))
                                    .size(px(25.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(6.0))
                                    .text_color(theme.text_3.hsla())
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style
                                            .bg(theme.surface_2.hsla())
                                            .text_color(theme.text.hsla())
                                    })
                                    .on_click(move |_event, _window, cx| {
                                        cx.stop_propagation();
                                        settle(id.clone(), cx);
                                    })
                                    .child(icon("icons/check.svg", 11.0)),
                            )
                        })
                    },
                )
                .when_some(open_menu, |line, open_menu| {
                    line.child(sidebar_menu_button(
                        format!("inbox-menu:{thread_id}").into(),
                        SidebarMenuRequest::Thread(thread_id.to_string()),
                        theme,
                        open_menu,
                    ))
                })
                .child(
                    div()
                        .ml(px(6.0))
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
    expanded: bool,
    theme: Theme,
    action: Option<SidebarAction>,
) -> impl IntoElement {
    let id = SharedString::from(format!("sidebar-group:{label}"));
    div()
        .id(id)
        .mt(px(5.0))
        .h(px(39.0))
        .w_full()
        .flex()
        .items_center()
        .border_b_1()
        .border_color(theme.line.hsla())
        .text_color(theme.text_3.hsla())
        .text_size(px(11.5))
        .when(action.is_some(), |row| {
            row.cursor_pointer()
                .hover(move |style| style.text_color(theme.text.hsla()))
        })
        .when_some(action, |row, action| {
            row.on_click(move |_event, _window, cx| action(cx))
        })
        .child(icon(
            if expanded {
                "icons/chevron-down.svg"
            } else {
                "icons/chevron-right.svg"
            },
            10.0,
        ))
        .child(div().ml(px(4.0)).flex_1().child(label))
        .when_some(count, |row, count| {
            row.child(div().pr(px(6.0)).child(count))
        })
}

fn settled_row(
    thread_id: SharedString,
    title: SharedString,
    meta: SharedString,
    theme: Theme,
    active: bool,
    on_select: Option<SelectSession>,
    open_menu: Option<OpenSidebarMenu>,
) -> impl IntoElement {
    let event_thread_id = thread_id.clone();
    let context_thread_id = thread_id.clone();
    let context_menu = open_menu.clone();
    div()
        .id(SharedString::from(format!("settled:{thread_id}")))
        .min_h(px(44.0))
        .w_full()
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(px(8.0))
        .when(active, |row| row.bg(theme.surface_2.hsla()))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface.hsla()))
        .on_click(move |event, _window, cx| {
            if event.is_right_click() {
                if let Some(open_menu) = &context_menu {
                    cx.stop_propagation();
                    open_menu(
                        SidebarMenuRequest::Thread(context_thread_id.to_string()),
                        event.position(),
                        cx,
                    );
                }
            } else if event.standard_click()
                && let Some(handler) = &on_select
            {
                handler(event_thread_id.to_string(), cx);
            }
        })
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
        .when_some(open_menu, |row, open_menu| {
            row.child(sidebar_menu_button(
                format!("shelf-menu:{thread_id}").into(),
                SidebarMenuRequest::Thread(thread_id.to_string()),
                theme,
                open_menu,
            ))
        })
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
        ProviderId::Grok => "Grok",
        ProviderId::Cursor => "Cursor",
        ProviderId::OpenCode => "OpenCode",
        ProviderId::Antigravity => "Antigravity",
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

fn wake_label(session: &SessionSummary) -> Option<String> {
    let Some(ThreadLifecycle::Snoozed { wake_at, .. }) = session.lifecycle.as_ref() else {
        return None;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64);
    let minutes = wake_at.saturating_sub(now).div_ceil(60_000);
    Some(match minutes {
        0..=59 => format!("in {}m", minutes.max(1)),
        60..=1_439 => format!("in {}h", minutes.div_ceil(60)),
        _ => format!("in {}d", minutes.div_ceil(1_440)),
    })
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
