use super::HarnessApp;
use super::provider_terminal::{ProviderTerminalKey, ProviderTerminalPhase};
use crate::client_state::{AuthTarget, ProviderTerminalKind};
use crate::preferences::{FontPreference, NativePreferences, ThemePreference};
use crate::theme::{Accent, Backdrop, Theme, ThemeMode};
use crate::zoom::px;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Entity, FontWeight, PathPromptOptions,
    PromptButton, PromptLevel, SharedString, Window, div, prelude::*, svg,
};
use gpui_component::input::{Input, InputState};
use harness_protocol::{
    McpAuth, McpAuthMethod, McpConfigValue, McpServer, McpServerConfig, McpStartupStatus,
    McpTransport, ModelConnectionInput, ModelConnectionPreset, ModelTransport, ProviderAuth,
    ProviderId, ProviderLogin, SidebarMode, SidebarSettings, Skill, SkillScope, UpdateCheckResult,
};
use std::rc::Rc;

const SETTINGS_CONTENT_WIDTH: f32 = 840.0;
const SETTINGS_SECTION_GAP: f32 = 30.0;

type SettingsAction = Rc<dyn Fn(&mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum McpEditorMode {
    Add,
    Edit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct McpEditorState {
    mode: McpEditorMode,
    provider: ProviderId,
    project_path: String,
    server_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SettingsSection {
    #[default]
    Providers,
    Models,
    Mcp,
    Skills,
    Workflows,
    Appearance,
    Data,
    About,
}

impl SettingsSection {
    const ALL: [Self; 8] = [
        Self::Providers,
        Self::Models,
        Self::Mcp,
        Self::Skills,
        Self::Workflows,
        Self::Appearance,
        Self::Data,
        Self::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Providers => "Providers",
            Self::Models => "Models",
            Self::Mcp => "MCP",
            Self::Skills => "Skills",
            Self::Workflows => "Workflows",
            Self::Appearance => "Appearance",
            Self::Data => "Data",
            Self::About => "About",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Providers => "icons/user-round.svg",
            Self::Models => "icons/boxes.svg",
            Self::Mcp => "icons/network.svg",
            Self::Skills => "icons/blocks.svg",
            Self::Workflows => "icons/panel-left.svg",
            Self::Appearance => "icons/palette.svg",
            Self::Data => "icons/database.svg",
            Self::About => "icons/info.svg",
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|section| *section == self)
            .unwrap_or_default()
    }
}

impl HarnessApp {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        if self.settings_open {
            return;
        }
        if self.sidebar_controls.is_open() {
            self.close_sidebar_controls(cx);
        }
        self.close_rollback(cx);
        self.account_menu_open = false;
        self.settings_return_to_chat = self.chat_visible;
        self.settings_open = true;
        self.settings_section = SettingsSection::Providers;
        self.settings_focus_pending = true;
        self.scope_open = false;
        self.settings_transition = self.settings_transition.wrapping_add(1);
        self.settings_open_transition = self.settings_open_transition.wrapping_add(1);
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.chat_visible = self.settings_return_to_chat;
        self.settings_focus_pending = false;
        self.mcp_editor = None;
        self.mcp_editor_submission_id = None;
        cx.notify();
    }

    pub(super) fn settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let back_view = cx.weak_entity();
        let back: SettingsAction = Rc::new(move |cx| {
            let _ = back_view.update(cx, |this, cx| this.close_settings(cx));
        });

        let mut nav_items = Vec::with_capacity(SettingsSection::ALL.len());
        for (index, section) in SettingsSection::ALL.into_iter().enumerate() {
            let selected = self.settings_section == section;
            let view = cx.weak_entity();
            let action: SettingsAction = Rc::new(move |cx| {
                let _ = view.update(cx, |this, cx| {
                    if this.settings_section != section {
                        this.settings_section = section;
                        this.mcp_editor = None;
                        this.mcp_editor_submission_id = None;
                        this.settings_transition = this.settings_transition.wrapping_add(1);
                        this.refresh_settings_inventory(cx);
                    }
                });
            });
            nav_items.push(settings_nav_item(
                index,
                section.label(),
                section.icon(),
                selected,
                theme,
                action,
            ));
        }

        let sidebar = div()
            .w(px(crate::RAIL_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .px(px(8.0))
            .pt(px(14.0))
            .pb(px(16.0))
            .bg(theme
                .rail
                .hsla()
                .opacity(1.0 - f32::from(self.preferences.sidebar_glass) / 200.0))
            .border_r_1()
            .border_color(theme.line.hsla())
            .child(
                div()
                    .id("settings-back")
                    .h(px(30.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .text_size(px(11.5))
                    .text_color(theme.text_2.hsla())
                    .cursor_pointer()
                    .hover(move |style| {
                        style
                            .bg(theme.surface_2.hsla())
                            .text_color(theme.text.hsla())
                    })
                    .active(|style| style.opacity(0.72))
                    .on_click(move |_event, _window, cx| back(cx))
                    .child(settings_icon("icons/arrow-left.svg", 14.0))
                    .child("Back to app"),
            )
            .child(
                div()
                    .mt(px(18.0))
                    .mb(px(4.0))
                    .px(px(8.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_3.hsla())
                    .child("Settings"),
            )
            .child(div().flex().flex_col().gap(px(1.0)).children(nav_items));

        let transition = self.settings_transition ^ self.settings_section.index() as u64;
        let content = self.settings_content(cx).with_animation(
            ("settings-section", transition),
            Animation::new(std::time::Duration::from_millis(220))
                .with_easing(crate::theme::web_ease_out),
            |panel, delta| panel.opacity(delta).mt(px(4.0 * (1.0 - delta))),
        );

        div()
            .size_full()
            .flex()
            .bg(theme.background.hsla())
            .child(sidebar)
            .child(
                div()
                    .id("settings-scroll")
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(SETTINGS_CONTENT_WIDTH))
                            .mx_auto()
                            .pt(px(48.0))
                            .px(px(48.0))
                            .pb(px(64.0))
                            .child(content),
                    ),
            )
            .with_animation(
                ("settings-overlay", self.settings_open_transition),
                Animation::new(theme.motion.fast).with_easing(crate::theme::web_ease_out),
                |panel, delta| panel.opacity(delta),
            )
            .into_any_element()
    }

    fn settings_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        match self.settings_section {
            SettingsSection::Providers => self.provider_settings(cx),
            SettingsSection::Models => self.model_settings(cx),
            SettingsSection::Mcp => self.mcp_settings(cx),
            SettingsSection::Skills => self.skills_settings(cx),
            SettingsSection::Workflows => self.workflow_settings(cx),
            SettingsSection::Appearance => self.appearance_settings(cx),
            SettingsSection::Data => self.data_settings(cx),
            SettingsSection::About => self.about_settings(cx),
        }
    }

    fn provider_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let mut blocks = Vec::new();
        let refresh_view = cx.weak_entity();
        let refresh: SettingsAction = Rc::new(move |cx| {
            let _ = refresh_view.update(cx, |this, cx| {
                this.state.refresh_model_catalog();
                cx.notify();
            });
        });

        let mut providers = Vec::new();
        for (index, provider) in self.state.provider_statuses.iter().enumerate() {
            let target = AuthTarget::provider(provider.id);
            let install_key =
                ProviderTerminalKey::new(target.clone(), ProviderTerminalKind::Install);
            let sign_in_key =
                ProviderTerminalKey::new(target.clone(), ProviderTerminalKind::SignIn);
            let account = self.state.accounts.get(&target);
            let signed_in = account
                .map(|account| account.signed_in)
                .unwrap_or(provider.auth == ProviderAuth::Authenticated);
            let ready = provider.installed && provider.problem.is_none() && signed_in;
            let status = if !provider.installed {
                "Not installed"
            } else if provider.problem.is_some() {
                "Needs attention"
            } else {
                match (signed_in, provider.setup.as_ref().map(|setup| setup.login)) {
                    (true, _) => "Ready",
                    (false, Some(ProviderLogin::Provider)) => "CLI sign-in required",
                    (false, _) => "Sign-in required",
                }
            };
            let idle_note = provider.problem.clone().unwrap_or_else(|| {
                if !provider.installed {
                    return provider
                        .setup
                        .as_ref()
                        .and_then(|setup| setup.install_command.clone())
                        .unwrap_or_else(|| "Provider CLI is not installed.".into());
                }
                if let Some(account) = account
                    && account.signed_in
                {
                    let identity = [account.email.as_deref(), account.plan.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    if identity.is_empty() {
                        "Signed in".into()
                    } else {
                        identity
                    }
                } else {
                    provider.version.as_ref().map_or_else(
                        || "Local provider adapter".into(),
                        |version| format!("Version {version}"),
                    )
                }
            });
            let terminal_key = if !provider.installed {
                Some(&install_key)
            } else if !signed_in
                && provider
                    .setup
                    .as_ref()
                    .is_some_and(|setup| setup.login == ProviderLogin::Provider)
            {
                Some(&sign_in_key)
            } else {
                None
            };
            let terminal =
                terminal_key.and_then(|key| self.provider_terminal_snapshot(key, &idle_note, cx));
            let note = terminal
                .as_ref()
                .map_or_else(|| idle_note.clone(), |terminal| terminal.note.clone());
            let busy = self.state.auth_busy.as_ref() == Some(&target)
                || self.state.provider_terminal_busy.is_some();
            let trailing = if busy {
                status_pill("Working…", false, theme)
            } else if !provider.installed {
                if let Some(setup) = &provider.setup {
                    if setup.install_command.is_some() {
                        match terminal.as_ref().map(|terminal| terminal.phase) {
                            Some(ProviderTerminalPhase::Starting) => {
                                status_pill("Starting…", false, theme)
                            }
                            Some(ProviderTerminalPhase::Running) => {
                                let key = install_key.clone();
                                let view = cx.weak_entity();
                                let action: SettingsAction = Rc::new(move |cx| {
                                    let key = key.clone();
                                    let _ = view.update(cx, |this, cx| {
                                        this.toggle_provider_terminal(&key, cx);
                                    });
                                });
                                provider_action_button(
                                    index,
                                    if terminal.as_ref().is_some_and(|terminal| terminal.visible) {
                                        "Hide terminal"
                                    } else {
                                        "Installing…"
                                    },
                                    false,
                                    theme,
                                    action,
                                )
                            }
                            Some(ProviderTerminalPhase::Succeeded) => {
                                status_pill("Installed", true, theme)
                            }
                            Some(ProviderTerminalPhase::Failed | ProviderTerminalPhase::Error)
                            | None => {
                                let target = target.clone();
                                let title = provider.display_name.clone();
                                let view = cx.weak_entity();
                                let action: SettingsAction = Rc::new(move |cx| {
                                    let target = target.clone();
                                    let title = title.clone();
                                    let _ = view.update(cx, |this, cx| {
                                        this.start_provider_terminal(
                                            target,
                                            ProviderTerminalKind::Install,
                                            title,
                                            cx,
                                        );
                                    });
                                });
                                provider_action_button(
                                    index,
                                    if terminal.is_some() {
                                        "Retry install"
                                    } else {
                                        "Install"
                                    },
                                    false,
                                    theme,
                                    action,
                                )
                            }
                        }
                    } else {
                        let url = setup.install_url.clone();
                        let action: SettingsAction = Rc::new(move |cx| cx.open_url(&url));
                        provider_action_button(index, "Install first", false, theme, action)
                    }
                } else {
                    status_pill(status, false, theme)
                }
            } else if provider
                .setup
                .as_ref()
                .is_some_and(|setup| setup.login == ProviderLogin::App)
            {
                let target = target.clone();
                let view = cx.weak_entity();
                let action: SettingsAction = Rc::new(move |cx| {
                    let target = target.clone();
                    let _ = view.update(cx, |this, cx| {
                        let update = if signed_in {
                            this.state.sign_out(target)
                        } else {
                            this.state.start_auth(target)
                        };
                        this.apply_client_update(update, cx);
                    });
                });
                provider_action_button(
                    index,
                    if signed_in { "Sign out" } else { "Sign in" },
                    signed_in,
                    theme,
                    action,
                )
            } else if provider
                .setup
                .as_ref()
                .is_some_and(|setup| setup.login == ProviderLogin::Provider)
                && !signed_in
            {
                match terminal.as_ref().map(|terminal| terminal.phase) {
                    Some(ProviderTerminalPhase::Starting) => status_pill("Starting…", false, theme),
                    Some(ProviderTerminalPhase::Running) => {
                        let key = sign_in_key.clone();
                        let view = cx.weak_entity();
                        let action: SettingsAction = Rc::new(move |cx| {
                            let key = key.clone();
                            let _ = view.update(cx, |this, cx| {
                                this.toggle_provider_terminal(&key, cx);
                            });
                        });
                        provider_action_button(
                            index,
                            if terminal.as_ref().is_some_and(|terminal| terminal.visible) {
                                "Hide terminal"
                            } else {
                                "Show terminal"
                            },
                            false,
                            theme,
                            action,
                        )
                    }
                    Some(ProviderTerminalPhase::Succeeded) => status_pill("Checking…", true, theme),
                    Some(ProviderTerminalPhase::Failed | ProviderTerminalPhase::Error) | None => {
                        let target = target.clone();
                        let title = provider.display_name.clone();
                        let view = cx.weak_entity();
                        let action: SettingsAction = Rc::new(move |cx| {
                            let target = target.clone();
                            let title = title.clone();
                            let _ = view.update(cx, |this, cx| {
                                this.start_provider_terminal(
                                    target,
                                    ProviderTerminalKind::SignIn,
                                    title,
                                    cx,
                                );
                            });
                        });
                        provider_action_button(
                            index,
                            if terminal.is_some() {
                                "Retry sign-in"
                            } else {
                                "Sign in"
                            },
                            false,
                            theme,
                            action,
                        )
                    }
                }
            } else {
                status_pill(status, ready, theme)
            };
            providers.push(settings_row(
                index,
                provider.display_name.clone(),
                note,
                trailing,
                theme,
            ));
            if let Some(key) = terminal_key
                && let Some(terminal) = self.provider_terminal_element(key, cx)
            {
                providers.push(terminal);
            }
        }
        if providers.is_empty() {
            providers.push(settings_empty_row(
                "Provider discovery is waiting for the local Harness server.",
                theme,
            ));
        }
        blocks.push(settings_group("CLI providers", providers, theme));
        if let Some(error) = &self.state.auth_error {
            blocks.push(settings_error_group(
                "Provider sign-in error",
                error.clone(),
                theme,
            ));
        }

        let mut connections = Vec::new();
        for (index, connection) in self.state.model_connections.iter().enumerate() {
            let status = if !connection.enabled {
                "Disabled"
            } else if connection.problem.is_some() {
                "Needs attention"
            } else if connection.credential_configured {
                "Ready"
            } else {
                "API key required"
            };
            let ready = connection.enabled
                && connection.credential_configured
                && connection.problem.is_none();
            let trailing = if self.state.connection_busy.as_deref() == Some(connection.id.as_str())
            {
                status_pill("Removing…", false, theme)
            } else {
                let connection_id = connection.id.clone();
                let view = cx.weak_entity();
                let remove: SettingsAction = Rc::new(move |cx| {
                    let connection_id = connection_id.clone();
                    let _ = view.update(cx, |this, cx| {
                        let update = this.state.remove_connection(connection_id);
                        this.apply_client_update(update, cx);
                    });
                });
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(status_pill(status, ready, theme))
                    .child(connection_remove_button(index, theme, remove))
                    .into_any_element()
            };
            connections.push(settings_row(
                index,
                connection.display_name.clone(),
                connection
                    .problem
                    .clone()
                    .unwrap_or_else(|| connection.base_url.clone()),
                trailing,
                theme,
            ));
        }
        if connections.is_empty() {
            connections.push(settings_empty_row(
                "No direct API connections are configured.",
                theme,
            ));
        }
        blocks.push(settings_group("Direct API connections", connections, theme));
        if let Some(error) = &self.state.connection_error {
            blocks.push(settings_error_group(
                "Connection error",
                error.clone(),
                theme,
            ));
        }
        if self.connection_editor_open {
            blocks.push(self.connection_form(cx));
        } else {
            let add_view = cx.weak_entity();
            let add: SettingsAction = Rc::new(move |cx| {
                let _ = add_view.update(cx, |this, cx| {
                    this.state.connection_error = None;
                    this.connection_editor_open = true;
                    cx.notify();
                });
            });
            blocks.push(
                div()
                    .flex()
                    .justify_end()
                    .child(settings_button(
                        "add-connection",
                        "Connect another plan or API",
                        "icons/plus.svg",
                        theme,
                        add,
                        false,
                    ))
                    .into_any_element(),
            );
        }

        let mut agents = Vec::new();
        for (index, agent) in self.state.acp_agents.iter().enumerate() {
            let target = AuthTarget::agent(ProviderId::Acp, agent.id.clone());
            let install_key =
                ProviderTerminalKey::new(target.clone(), ProviderTerminalKind::Install);
            let sign_in_key =
                ProviderTerminalKey::new(target.clone(), ProviderTerminalKind::SignIn);
            let account = self.state.accounts.get(&target);
            let signed_in = account.is_some_and(|account| account.signed_in);
            let status = if agent.installed && agent.verified && signed_in {
                "Ready"
            } else if agent.installed {
                "CLI sign-in required"
            } else {
                "Not installed"
            };
            let idle_note = agent.problem.clone().unwrap_or_else(|| {
                if !agent.installed {
                    return agent
                        .setup
                        .install_command
                        .clone()
                        .unwrap_or_else(|| "Provider CLI is not installed.".into());
                }
                if let Some(account) = account
                    && account.signed_in
                {
                    let identity = [account.email.as_deref(), account.plan.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    if identity.is_empty() {
                        "Signed in".into()
                    } else {
                        identity
                    }
                } else if agent.verified {
                    "Captured ACP protocol adapter".into()
                } else {
                    "ACP-compatible coding agent".into()
                }
            });
            let terminal_key = if !agent.installed {
                Some(&install_key)
            } else if !signed_in {
                Some(&sign_in_key)
            } else {
                None
            };
            let terminal =
                terminal_key.and_then(|key| self.provider_terminal_snapshot(key, &idle_note, cx));
            let note = terminal
                .as_ref()
                .map_or_else(|| idle_note.clone(), |terminal| terminal.note.clone());
            let busy = self.state.provider_terminal_busy.is_some();
            let action_index = 1_000 + index;
            let trailing = if busy {
                status_pill("Working…", false, theme)
            } else if !agent.installed {
                if agent.setup.install_command.is_some() {
                    match terminal.as_ref().map(|terminal| terminal.phase) {
                        Some(ProviderTerminalPhase::Starting) => {
                            status_pill("Starting…", false, theme)
                        }
                        Some(ProviderTerminalPhase::Running) => {
                            let key = install_key.clone();
                            let view = cx.weak_entity();
                            let action: SettingsAction = Rc::new(move |cx| {
                                let key = key.clone();
                                let _ = view.update(cx, |this, cx| {
                                    this.toggle_provider_terminal(&key, cx);
                                });
                            });
                            provider_action_button(
                                action_index,
                                if terminal.as_ref().is_some_and(|terminal| terminal.visible) {
                                    "Hide terminal"
                                } else {
                                    "Installing…"
                                },
                                false,
                                theme,
                                action,
                            )
                        }
                        Some(ProviderTerminalPhase::Succeeded) => {
                            status_pill("Installed", true, theme)
                        }
                        Some(ProviderTerminalPhase::Failed | ProviderTerminalPhase::Error)
                        | None => {
                            let target = target.clone();
                            let title = agent.name.clone();
                            let view = cx.weak_entity();
                            let action: SettingsAction = Rc::new(move |cx| {
                                let target = target.clone();
                                let title = title.clone();
                                let _ = view.update(cx, |this, cx| {
                                    this.start_provider_terminal(
                                        target,
                                        ProviderTerminalKind::Install,
                                        title,
                                        cx,
                                    );
                                });
                            });
                            provider_action_button(
                                action_index,
                                if terminal.is_some() {
                                    "Retry install"
                                } else {
                                    "Install"
                                },
                                false,
                                theme,
                                action,
                            )
                        }
                    }
                } else {
                    let url = agent.setup.install_url.clone();
                    let action: SettingsAction = Rc::new(move |cx| cx.open_url(&url));
                    provider_action_button(action_index, "Install first", false, theme, action)
                }
            } else if !signed_in {
                match terminal.as_ref().map(|terminal| terminal.phase) {
                    Some(ProviderTerminalPhase::Starting) => status_pill("Starting…", false, theme),
                    Some(ProviderTerminalPhase::Running) => {
                        let key = sign_in_key.clone();
                        let view = cx.weak_entity();
                        let action: SettingsAction = Rc::new(move |cx| {
                            let key = key.clone();
                            let _ = view.update(cx, |this, cx| {
                                this.toggle_provider_terminal(&key, cx);
                            });
                        });
                        provider_action_button(
                            action_index,
                            if terminal.as_ref().is_some_and(|terminal| terminal.visible) {
                                "Hide terminal"
                            } else {
                                "Show terminal"
                            },
                            false,
                            theme,
                            action,
                        )
                    }
                    Some(ProviderTerminalPhase::Succeeded) => status_pill("Checking…", true, theme),
                    Some(ProviderTerminalPhase::Failed | ProviderTerminalPhase::Error) | None => {
                        let target = target.clone();
                        let title = agent.name.clone();
                        let view = cx.weak_entity();
                        let action: SettingsAction = Rc::new(move |cx| {
                            let target = target.clone();
                            let title = title.clone();
                            let _ = view.update(cx, |this, cx| {
                                this.start_provider_terminal(
                                    target,
                                    ProviderTerminalKind::SignIn,
                                    title,
                                    cx,
                                );
                            });
                        });
                        provider_action_button(
                            action_index,
                            if terminal.is_some() {
                                "Retry sign-in"
                            } else {
                                "Sign in"
                            },
                            false,
                            theme,
                            action,
                        )
                    }
                }
            } else {
                status_pill(status, agent.verified && signed_in, theme)
            };
            agents.push(settings_row(
                index,
                agent.name.clone(),
                note,
                trailing,
                theme,
            ));
            if let Some(key) = terminal_key
                && let Some(terminal) = self.provider_terminal_element(key, cx)
            {
                agents.push(terminal);
            }
        }
        if !agents.is_empty() {
            blocks.push(settings_group("ACP agents", agents, theme));
        }

        blocks.push(
            div()
                .flex()
                .justify_end()
                .child(settings_button(
                    "refresh-providers",
                    "Refresh providers",
                    "icons/rotate-ccw.svg",
                    theme,
                    refresh,
                    false,
                ))
                .into_any_element(),
        );
        settings_panel("Providers", blocks, theme)
    }

    fn connection_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let presets = [
            (ModelConnectionPreset::Openai, "OpenAI"),
            (ModelConnectionPreset::Anthropic, "Anthropic"),
            (ModelConnectionPreset::Openrouter, "OpenRouter"),
            (ModelConnectionPreset::Kimi, "Kimi"),
            (ModelConnectionPreset::Zai, "Z.ai"),
            (ModelConnectionPreset::Custom, "Custom"),
        ];
        let mut preset_buttons = Vec::new();
        for (index, (preset, label)) in presets.into_iter().enumerate() {
            let selected = self.connection_preset == preset;
            preset_buttons.push(
                div()
                    .id(("connection-preset", index))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .px(px(10.0))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(if selected {
                        theme.attention.hsla()
                    } else {
                        theme.line.hsla()
                    })
                    .bg(if selected {
                        theme.surface_2.hsla()
                    } else {
                        theme.surface.hsla()
                    })
                    .text_size(px(10.5))
                    .text_color(if selected {
                        theme.text.hsla()
                    } else {
                        theme.text_3.hsla()
                    })
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_2.hsla()))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.select_connection_preset(preset, window, cx);
                    }))
                    .child(label)
                    .into_any_element(),
            );
        }
        let busy = self.state.connection_busy.is_some();
        let can_submit = !busy
            && !self.connection_name.read(cx).value().trim().is_empty()
            && !self.connection_base_url.read(cx).value().trim().is_empty()
            && !self
                .connection_api_key
                .read(cx)
                .unmask_value()
                .trim()
                .is_empty();

        let cancel = div()
            .id("cancel-connection")
            .h(px(30.0))
            .flex()
            .items_center()
            .gap(px(7.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(theme.surface.hsla())
            .text_size(px(11.0))
            .text_color(theme.text_2.hsla())
            .when(!busy, |button| {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_2.hsla()))
                    .active(|style| style.opacity(0.72))
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.cancel_connection_editor(window, cx)
                    }))
            })
            .child(settings_icon("icons/x.svg", 13.0))
            .child("Cancel")
            .into_any_element();
        let submit = if can_submit {
            div()
                .id("submit-connection")
                .h(px(30.0))
                .flex()
                .items_center()
                .px(px(12.0))
                .rounded(px(8.0))
                .bg(theme.attention.hsla())
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(gpui::white())
                .cursor_pointer()
                .hover(|style| style.opacity(0.9))
                .active(|style| style.opacity(0.72))
                .on_click(
                    cx.listener(|this, _event, window, cx| this.submit_connection(window, cx)),
                )
                .child("Connect")
                .into_any_element()
        } else {
            div()
                .h(px(30.0))
                .flex()
                .items_center()
                .px(px(12.0))
                .rounded(px(8.0))
                .bg(theme.surface_3.hsla())
                .text_size(px(11.0))
                .text_color(theme.text_3.hsla())
                .opacity(0.62)
                .child(if busy { "Connecting…" } else { "Connect" })
                .into_any_element()
        };

        div()
            .w_full()
            .child(
                div()
                    .mb(px(12.0))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_2.hsla())
                    .child("New API connection"),
            )
            .child(
                div()
                    .w_full()
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(theme.line_strong.hsla())
                    .bg(theme.rail.hsla())
                    .p(px(16.0))
                    .child(
                        div()
                            .mb(px(14.0))
                            .flex()
                            .flex_wrap()
                            .gap(px(7.0))
                            .children(preset_buttons),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_wrap()
                            .gap(px(12.0))
                            .child(connection_field(
                                "Name",
                                &self.connection_name,
                                false,
                                theme,
                            ))
                            .child(connection_field(
                                "Default model",
                                &self.connection_default_model,
                                false,
                                theme,
                            ))
                            .child(connection_field(
                                "Base URL",
                                &self.connection_base_url,
                                true,
                                theme,
                            ))
                            .child(connection_field(
                                "API key",
                                &self.connection_api_key,
                                true,
                                theme,
                            )),
                    )
                    .child(
                        div()
                            .mt(px(16.0))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(cancel)
                            .child(submit),
                    ),
            )
            .into_any_element()
    }

    fn select_connection_preset(
        &mut self,
        preset: ModelConnectionPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.connection_preset = preset;
        let config = connection_preset(preset);
        self.connection_name
            .update(cx, |input, cx| input.set_value(config.label, window, cx));
        self.connection_base_url
            .update(cx, |input, cx| input.set_value(config.base_url, window, cx));
        self.connection_default_model
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.state.connection_error = None;
        cx.notify();
    }

    fn submit_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.connection_busy.is_some() {
            return;
        }
        let display_name = self.connection_name.read(cx).value().trim().to_owned();
        let base_url = self.connection_base_url.read(cx).value().trim().to_owned();
        let default_model = self
            .connection_default_model
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let api_key = self
            .connection_api_key
            .read(cx)
            .unmask_value()
            .trim()
            .to_owned();
        if display_name.is_empty() || base_url.is_empty() || api_key.is_empty() {
            self.state.connection_error = Some("Name, base URL, and API key are required.".into());
            cx.notify();
            return;
        }
        if let Err(message) = validate_model_endpoint(&base_url) {
            self.state.connection_error = Some(message);
            cx.notify();
            return;
        }
        let config = connection_preset(self.connection_preset);
        let connection_id = format!(
            "{}-{}",
            connection_preset_id(self.connection_preset),
            uuid::Uuid::new_v4()
        );
        let update = self.state.upsert_connection(
            ModelConnectionInput {
                id: connection_id.clone(),
                display_name,
                preset: self.connection_preset,
                transport: config.transport,
                base_url,
                default_model: (!default_model.is_empty()).then_some(default_model),
                enabled: true,
            },
            api_key,
        );
        self.connection_submission_id = Some(connection_id);
        self.connection_api_key
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.apply_client_update(update, cx);
    }

    fn cancel_connection_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.connection_busy.is_some() {
            return;
        }
        self.connection_api_key
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.connection_editor_open = false;
        self.connection_submission_id = None;
        self.state.connection_error = None;
        cx.notify();
    }

    fn model_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let mut sources: Vec<(String, Vec<crate::client_state::ModelChoice>)> = Vec::new();
        for choice in &self.state.model_catalog {
            if let Some((_, choices)) = sources
                .iter_mut()
                .find(|(source, _)| *source == choice.source_name)
            {
                choices.push(choice.clone());
            } else {
                sources.push((choice.source_name.clone(), vec![choice.clone()]));
            }
        }

        let mut blocks = Vec::new();
        for (source_index, (source, choices)) in sources.into_iter().enumerate() {
            let any_visible = choices
                .iter()
                .any(|choice| !self.preferences.hidden_models.contains(&choice.key));
            let visible_count = choices
                .iter()
                .filter(|choice| !self.preferences.hidden_models.contains(&choice.key))
                .count();
            let keys = choices
                .iter()
                .map(|choice| choice.key.clone())
                .collect::<Vec<_>>();
            let master_view = cx.weak_entity();
            let master: SettingsAction = Rc::new(move |cx| {
                let keys = keys.clone();
                let _ = master_view.update(cx, |this, cx| {
                    this.set_models_visible(&keys, !any_visible, cx);
                });
            });
            let header = div()
                .min_h(px(58.0))
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .px(px(16.0))
                .py(px(10.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text.hsla())
                                .child(source.clone()),
                        )
                        .child(
                            div()
                                .mt(px(2.0))
                                .text_size(px(11.0))
                                .text_color(theme.text_3.hsla())
                                .child(format!(
                                    "{visible_count} of {} {} visible",
                                    choices.len(),
                                    if choices.len() == 1 {
                                        "model"
                                    } else {
                                        "models"
                                    }
                                )),
                        ),
                )
                .child(settings_switch(
                    source_index * 10_000,
                    any_visible,
                    theme,
                    master,
                ));

            let mut rows = vec![header.into_any_element()];
            for (model_index, choice) in choices.into_iter().enumerate() {
                let visible = !self.preferences.hidden_models.contains(&choice.key);
                let key = choice.key.clone();
                let view = cx.weak_entity();
                let action: SettingsAction = Rc::new(move |cx| {
                    let key = key.clone();
                    let _ = view.update(cx, |this, cx| {
                        this.set_models_visible(&[key], !visible, cx);
                    });
                });
                rows.push(settings_row(
                    model_index + 1,
                    choice.model.display_name,
                    choice
                        .model
                        .description
                        .unwrap_or_else(|| "Available from this provider".into()),
                    settings_switch(
                        source_index * 10_000 + model_index + 1,
                        visible,
                        theme,
                        action,
                    ),
                    theme,
                ));
            }
            blocks.push(settings_group(source.as_str(), rows, theme));
        }

        if blocks.is_empty() {
            blocks.push(settings_group(
                "Model visibility",
                vec![settings_empty_row(
                    if self.state.model_catalog_loaded {
                        "No models are available from connected providers yet."
                    } else {
                        "Model discovery is still in progress."
                    },
                    theme,
                )],
                theme,
            ));
        }
        settings_panel("Models", blocks, theme)
    }

    fn mcp_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let Some((provider, project_path)) = self.settings_scope() else {
            return settings_panel(
                "MCP servers",
                vec![settings_group(
                    "Model Context Protocol",
                    vec![settings_empty_row(
                        "Select a project in the sidebar before managing MCP servers.",
                        theme,
                    )],
                    theme,
                )],
                theme,
            );
        };
        let project_name = self.settings_project_name(&project_path);
        let provider_name = self.settings_provider_name(provider);
        let inventory = self.state.mcp_inventory.as_ref().filter(|inventory| {
            inventory.provider == provider && inventory.project_path == project_path
        });
        let refresh_view = cx.weak_entity();
        let refresh_path = project_path.clone();
        let refresh: SettingsAction = Rc::new(move |cx| {
            let path = refresh_path.clone();
            let _ = refresh_view.update(cx, |this, cx| {
                let update = this.state.request_mcp_inventory(provider, path);
                this.apply_client_update(update, cx);
            });
        });
        let add_action = inventory
            .filter(|inventory| inventory.result.capabilities.add)
            .map(|_| {
                let path = project_path.clone();
                mcp_action_button(
                    "mcp-add-server".into(),
                    "Add server".into(),
                    false,
                    self.state.mcp_busy.is_none(),
                    theme,
                    cx.listener(move |this, _event, window, cx| {
                        this.open_mcp_editor(
                            McpEditorMode::Add,
                            provider,
                            path.clone(),
                            None,
                            window,
                            cx,
                        );
                    }),
                )
            });
        let intro = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .child(
                div()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_2.hsla())
                            .child(format!("Available in {project_name}")),
                    )
                    .child(
                        div()
                            .mt(px(3.0))
                            .text_size(px(10.5))
                            .text_color(theme.text_3.hsla())
                            .child(format!("Managed through {provider_name}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when_some(add_action, |actions, add| actions.child(add))
                    .child(settings_button(
                        "refresh-mcp",
                        if self.state.mcp_loading {
                            "Refreshing…"
                        } else {
                            "Refresh"
                        },
                        "icons/rotate-ccw.svg",
                        theme,
                        refresh,
                        false,
                    )),
            )
            .into_any_element();
        let mut blocks = vec![settings_plain_group("Project inventory", intro, theme)];

        if let Some(error) = &self.state.mcp_error {
            blocks.push(settings_error_group("MCP error", error.clone(), theme));
        }
        if let Some(notice) = &self.state.mcp_notice {
            blocks.push(settings_notice_group("MCP", notice.clone(), theme));
        }
        if self.mcp_editor.as_ref().is_some_and(|editor| {
            editor.provider == provider && editor.project_path == project_path
        }) {
            blocks.push(self.mcp_editor_form(cx));
        }
        let Some(inventory) = inventory else {
            blocks.push(settings_group(
                "Servers",
                vec![settings_empty_row(
                    if self.state.mcp_loading {
                        "Loading MCP servers…"
                    } else {
                        "MCP inventory has not been loaded yet."
                    },
                    theme,
                )],
                theme,
            ));
            return settings_panel("MCP servers", blocks, theme);
        };
        if !inventory.result.capabilities.inventory {
            blocks.push(settings_group(
                "Servers",
                vec![settings_empty_row(
                    format!("{provider_name} does not expose MCP inventory here."),
                    theme,
                )],
                theme,
            ));
            return settings_panel("MCP servers", blocks, theme);
        }

        let mut rows = Vec::new();
        for (index, server) in inventory.result.servers.iter().enumerate() {
            let busy = self.state.mcp_busy.as_deref() == Some(server.id.as_str());
            let needs_oauth = inventory.result.capabilities.start_o_auth
                && matches!(
                    server.auth,
                    McpAuth::SignInRequired {
                        method: McpAuthMethod::Oauth
                    }
                );
            let can_toggle = inventory.result.capabilities.remove
                && ((!server.enabled && server.scope == harness_protocol::McpServerScope::Project)
                    || (inventory.result.capabilities.add
                        && server.scope == harness_protocol::McpServerScope::Global));
            let can_edit = inventory.result.capabilities.update
                && server.scope == harness_protocol::McpServerScope::Project
                && server.transport.is_some();
            let can_remove = inventory.result.capabilities.remove
                && server.scope == harness_protocol::McpServerScope::Project
                && server.enabled;
            let oauth_active = self.state.mcp_oauth.as_ref().is_some_and(|oauth| {
                oauth.provider == provider
                    && oauth.project_path == project_path
                    && oauth.server_id == server.id
            });
            let mut actions = Vec::new();
            if busy {
                actions.push(status_pill("Saving…", false, theme));
            } else if oauth_active {
                let can_cancel = inventory.result.capabilities.cancel_o_auth;
                actions.push(mcp_action_button(
                    format!("mcp-cancel-oauth-{}", server.id).into(),
                    if can_cancel {
                        "Cancel sign-in"
                    } else {
                        "Signing in…"
                    }
                    .into(),
                    false,
                    can_cancel,
                    theme,
                    cx.listener(|this, _event, _window, cx| {
                        let update = this.state.cancel_mcp_oauth();
                        this.apply_client_update(update, cx);
                    }),
                ));
            } else if needs_oauth {
                let path = project_path.clone();
                let server_id = server.id.clone();
                actions.push(mcp_action_button(
                    format!("mcp-oauth-{server_id}").into(),
                    "Sign in".into(),
                    false,
                    true,
                    theme,
                    cx.listener(move |this, _event, _window, cx| {
                        let update =
                            this.state
                                .start_mcp_oauth(provider, path.clone(), server_id.clone());
                        this.apply_client_update(update, cx);
                    }),
                ));
            }
            if can_toggle && !busy {
                let enabled = server.enabled;
                let server = server.clone();
                let path = project_path.clone();
                let view = cx.weak_entity();
                let action: SettingsAction = Rc::new(move |cx| {
                    let server = server.clone();
                    let path = path.clone();
                    let _ = view.update(cx, |this, cx| {
                        let update = this.state.toggle_mcp_server(provider, path, &server);
                        this.apply_client_update(update, cx);
                    });
                });
                actions.push(settings_switch(920_000 + index, enabled, theme, action));
            }
            if can_edit && !busy {
                let path = project_path.clone();
                let server = server.clone();
                actions.push(mcp_action_button(
                    format!("mcp-edit-{}", server.id).into(),
                    "Edit".into(),
                    false,
                    true,
                    theme,
                    cx.listener(move |this, _event, window, cx| {
                        this.open_mcp_editor(
                            McpEditorMode::Edit,
                            provider,
                            path.clone(),
                            Some(server.clone()),
                            window,
                            cx,
                        );
                    }),
                ));
            }
            if can_remove && !busy {
                let path = project_path.clone();
                let server_id = server.id.clone();
                let server_name = server
                    .display_name
                    .clone()
                    .unwrap_or_else(|| server.id.clone());
                actions.push(mcp_action_button(
                    format!("mcp-remove-{server_id}").into(),
                    "Remove".into(),
                    true,
                    true,
                    theme,
                    cx.listener(move |this, _event, window, cx| {
                        this.confirm_remove_mcp_server(
                            provider,
                            path.clone(),
                            server_id.clone(),
                            server_name.clone(),
                            window,
                            cx,
                        );
                    }),
                ));
            }

            let name = server
                .display_name
                .clone()
                .unwrap_or_else(|| server.id.clone());
            let scope = match server.scope {
                harness_protocol::McpServerScope::Project => "project",
                harness_protocol::McpServerScope::Global => "global",
            };
            let (status, ready) = mcp_status(server);
            let detail_key = (provider, project_path.clone(), server.id.clone());
            let expanded = self.mcp_expanded_servers.contains(&detail_key);
            let expand_view = cx.weak_entity();
            let summary = format!(
                "{} tools · {} resources · {} templates",
                server.tools.len(),
                server.resources.len(),
                server.resource_templates.len()
            );
            let details = div()
                .id(SharedString::from(format!("mcp-details-{}", server.id)))
                .mt(px(9.0))
                .text_size(px(10.5))
                .text_color(theme.text_3.hsla())
                .cursor_pointer()
                .on_click(move |_event, _window, cx| {
                    let detail_key = detail_key.clone();
                    let _ = expand_view.update(cx, |this, cx| {
                        if !this.mcp_expanded_servers.remove(&detail_key) {
                            this.mcp_expanded_servers.insert(detail_key);
                        }
                        cx.notify();
                    });
                })
                .child(if expanded {
                    format!("Hide · {summary}")
                } else {
                    summary
                });
            let tool_list = expanded.then(|| {
                if server.tools.is_empty() {
                    div()
                        .mt(px(8.0))
                        .text_size(px(10.5))
                        .text_color(theme.text_3.hsla())
                        .child("No tools reported.")
                        .into_any_element()
                } else {
                    div()
                        .mt(px(8.0))
                        .pl(px(10.0))
                        .border_l_1()
                        .border_color(theme.line.hsla())
                        .flex()
                        .flex_col()
                        .gap(px(5.0))
                        .children(server.tools.iter().map(|tool| {
                            let title = tool.title.as_deref().unwrap_or(&tool.name);
                            let line = tool.description.as_ref().map_or_else(
                                || title.to_owned(),
                                |description| format!("{title} — {description}"),
                            );
                            div()
                                .text_size(px(10.5))
                                .line_height(px(15.0))
                                .text_color(theme.text_2.hsla())
                                .child(line)
                        }))
                        .into_any_element()
                }
            });
            let failure = match &server.startup {
                McpStartupStatus::Failed { message } => Some(
                    div()
                        .mt(px(8.0))
                        .text_size(px(10.5))
                        .text_color(theme.error.hsla())
                        .child(message.clone()),
                ),
                _ => None,
            };
            rows.push(
                div()
                    .min_h(px(76.0))
                    .w_full()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(20.0))
                    .px(px(16.0))
                    .py(px(14.0))
                    .when(index > 0, |row| {
                        row.border_t_1().border_color(theme.line.hsla())
                    })
                    .opacity(if server.enabled { 1.0 } else { 0.58 })
                    .hover(move |style| style.bg(theme.surface.hsla()))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap(px(7.0))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text.hsla())
                                            .child(name),
                                    )
                                    .child(mcp_badge(scope, theme))
                                    .child(mcp_badge(status, theme))
                                    .when(ready, |heading| {
                                        heading.child(
                                            div()
                                                .size(px(5.0))
                                                .rounded_full()
                                                .bg(theme.success.hsla()),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .mt(px(6.0))
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_3.hsla())
                                    .child(mcp_transport_label(server.transport.as_ref())),
                            )
                            .when_some(failure, |copy, failure| copy.child(failure))
                            .child(details)
                            .when_some(tool_list, |copy, tools| copy.child(tools)),
                    )
                    .child(
                        div()
                            .max_w(px(250.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_end()
                            .flex_wrap()
                            .gap(px(7.0))
                            .children(actions),
                    )
                    .into_any_element(),
            );
        }
        if rows.is_empty() {
            rows.push(settings_empty_row(
                "No MCP servers are configured for this project.",
                theme,
            ));
        }
        blocks.push(settings_group("Servers", rows, theme));
        settings_panel("MCP servers", blocks, theme)
    }

    fn mcp_editor_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let editor = self.mcp_editor.as_ref().expect("editor checked by caller");
        let editing = editor.mode == McpEditorMode::Edit;
        let saving = self.mcp_editor_submission_id.is_some();
        let can_save = !saving
            && !self.mcp_editor_id.read(cx).value().trim().is_empty()
            && !self.mcp_editor_transport.read(cx).value().trim().is_empty();
        let cancel = mcp_action_button(
            "mcp-editor-cancel".into(),
            "Cancel".into(),
            false,
            !saving,
            theme,
            cx.listener(|this, _event, _window, cx| {
                this.mcp_editor = None;
                this.mcp_editor_submission_id = None;
                this.state.mcp_error = None;
                cx.notify();
            }),
        );
        let save = mcp_action_button(
            "mcp-editor-save".into(),
            if saving { "Saving…" } else { "Save server" }.into(),
            false,
            can_save,
            theme,
            cx.listener(|this, _event, window, cx| {
                this.submit_mcp_editor(window, cx);
            }),
        );

        div()
            .w_full()
            .child(
                div()
                    .mb(px(12.0))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_2.hsla())
                    .child(if editing {
                        "Edit MCP server"
                    } else {
                        "Add MCP server"
                    }),
            )
            .child(
                div()
                    .w_full()
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(theme.line_strong.hsla())
                    .bg(theme.rail.hsla())
                    .p(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .gap(px(14.0))
                            .child(mcp_editor_field(
                                "Server ID",
                                &self.mcp_editor_id,
                                editing,
                                false,
                                theme,
                            ))
                            .child(mcp_editor_field(
                                "Display name",
                                &self.mcp_editor_name,
                                false,
                                false,
                                theme,
                            )),
                    )
                    .child(mcp_editor_field(
                        "Transport JSON",
                        &self.mcp_editor_transport,
                        false,
                        true,
                        theme,
                    ))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .line_height(px(15.0))
                            .text_color(theme.text_3.hsla())
                            .child(
                                "Use stdio or HTTP transport fields. Reference secrets with a credentialRef instead of entering them here.",
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(cancel)
                            .child(save),
                    ),
            )
            .into_any_element()
    }

    fn confirm_remove_mcp_server(
        &mut self,
        provider: ProviderId,
        project_path: String,
        server_id: String,
        server_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.mcp_busy.is_some() {
            return;
        }
        let message = format!("Remove {server_name} from this project?");
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(
                "This removes the project configuration. It does not delete the MCP server itself.",
            ),
            &[PromptButton::cancel("Cancel"), PromptButton::new("Remove")],
            cx,
        );
        cx.spawn(async move |view, cx| {
            let Ok(1) = answer.await else {
                return;
            };
            let _ = view.update(cx, |this, cx| {
                let update = this
                    .state
                    .remove_mcp_server(provider, project_path, server_id);
                this.apply_client_update(update, cx);
            });
        })
        .detach();
    }

    fn open_mcp_editor(
        &mut self,
        mode: McpEditorMode,
        provider: ProviderId,
        project_path: String,
        server: Option<McpServer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.mcp_busy.is_some() {
            return;
        }
        let server_id = server.as_ref().map(|server| server.id.clone());
        let id = server_id.clone().unwrap_or_default();
        let display_name = server
            .as_ref()
            .and_then(|server| server.display_name.clone())
            .unwrap_or_default();
        let transport = server
            .as_ref()
            .and_then(|server| server.transport.as_ref())
            .and_then(|transport| serde_json::to_string_pretty(transport).ok())
            .unwrap_or_else(|| {
                "{\n  \"type\": \"http\",\n  \"url\": \"https://example.com/mcp\"\n}".into()
            });
        self.mcp_editor = Some(McpEditorState {
            mode,
            provider,
            project_path,
            server_id,
        });
        self.mcp_editor_submission_id = None;
        self.state.mcp_error = None;
        self.state.mcp_notice = None;
        self.mcp_editor_id
            .update(cx, |input, cx| input.set_value(id, window, cx));
        self.mcp_editor_name
            .update(cx, |input, cx| input.set_value(display_name, window, cx));
        self.mcp_editor_transport
            .update(cx, |input, cx| input.set_value(transport, window, cx));
        if mode == McpEditorMode::Add {
            self.mcp_editor_id
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            self.mcp_editor_transport
                .update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    fn submit_mcp_editor(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.mcp_editor_submission_id.is_some() || self.state.mcp_busy.is_some() {
            return;
        }
        let Some(editor) = self.mcp_editor.clone() else {
            return;
        };
        let id = self.mcp_editor_id.read(cx).value().trim().to_owned();
        let display_name = self.mcp_editor_name.read(cx).value().trim().to_owned();
        let transport_json = self.mcp_editor_transport.read(cx).value().trim().to_owned();
        if id.is_empty() || transport_json.is_empty() {
            self.state.mcp_error = Some("Server ID and transport JSON are required.".into());
            cx.notify();
            return;
        }
        if editor.mode == McpEditorMode::Edit && editor.server_id.as_deref() != Some(id.as_str()) {
            self.state.mcp_error = Some("An existing MCP server ID cannot be changed.".into());
            cx.notify();
            return;
        }
        let transport = match serde_json::from_str::<McpTransport>(&transport_json) {
            Ok(transport) => transport,
            Err(_) => {
                self.state.mcp_error = Some("Transport must be valid MCP JSON.".into());
                cx.notify();
                return;
            }
        };
        if let Err(message) = validate_mcp_transport(&transport) {
            self.state.mcp_error = Some(message);
            cx.notify();
            return;
        }
        let update = self.state.save_mcp_server(
            editor.provider,
            editor.project_path,
            McpServerConfig {
                id: id.clone(),
                enabled: true,
                display_name: (!display_name.is_empty()).then_some(display_name),
                transport: Some(transport),
            },
            editor.mode == McpEditorMode::Edit,
        );
        self.mcp_editor_submission_id = Some(id);
        self.apply_client_update(update, cx);
    }

    fn skills_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let Some((provider, project_path)) = self.settings_scope() else {
            return settings_panel(
                "Agent Skills",
                vec![settings_group(
                    "Skills",
                    vec![settings_empty_row(
                        "Select a project in the sidebar before managing Agent Skills.",
                        theme,
                    )],
                    theme,
                )],
                theme,
            );
        };
        let project_name = self.settings_project_name(&project_path);
        let provider_name = self.settings_provider_name(provider);
        let refresh_view = cx.weak_entity();
        let refresh_path = project_path.clone();
        let refresh: SettingsAction = Rc::new(move |cx| {
            let path = refresh_path.clone();
            let _ = refresh_view.update(cx, |this, cx| {
                let update = this.state.request_skills_inventory(provider, path);
                this.apply_client_update(update, cx);
            });
        });
        let inventory = self.state.skills_inventory.as_ref().filter(|inventory| {
            inventory.provider == provider && inventory.project_path == project_path
        });
        let install_supported = inventory.is_some_and(|inventory| {
            inventory.result.capabilities.install && self.state.skills_busy.is_none()
        });
        let intro_action = if install_supported {
            let install_view = cx.weak_entity();
            let install_path = project_path.clone();
            let install: SettingsAction = Rc::new(move |cx| {
                let path = install_path.clone();
                let _ =
                    install_view.update(cx, |this, cx| this.pick_skill_folder(provider, path, cx));
            });
            settings_button(
                "install-skill",
                "Install from folder",
                "icons/folder-pen.svg",
                theme,
                install,
                false,
            )
        } else {
            settings_button(
                "refresh-skills",
                if self.state.skills_loading {
                    "Refreshing…"
                } else {
                    "Refresh"
                },
                "icons/rotate-ccw.svg",
                theme,
                refresh,
                false,
            )
        };
        let intro = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .child(
                div()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_2.hsla())
                            .child(format!("Available in {project_name}")),
                    )
                    .child(
                        div()
                            .mt(px(3.0))
                            .text_size(px(10.5))
                            .text_color(theme.text_3.hsla())
                            .child(format!("Discovered by {provider_name}")),
                    ),
            )
            .child(intro_action)
            .into_any_element();
        let mut blocks = vec![settings_plain_group("Project inventory", intro, theme)];

        if let Some(error) = &self.state.skills_error {
            blocks.push(settings_error_group("Skills error", error.clone(), theme));
        }
        let Some(inventory) = inventory else {
            blocks.push(settings_group(
                "Skills",
                vec![settings_empty_row(
                    if self.state.skills_loading {
                        "Discovering Agent Skills…"
                    } else {
                        "Skill inventory has not been loaded yet."
                    },
                    theme,
                )],
                theme,
            ));
            return settings_panel("Agent Skills", blocks, theme);
        };
        if !inventory.result.errors.is_empty() {
            let details = inventory
                .result
                .errors
                .iter()
                .map(|error| format!("{} · {}", error.message, error.path))
                .collect::<Vec<_>>()
                .join("\n");
            blocks.push(settings_error_group(
                "Some skills could not be loaded",
                details,
                theme,
            ));
        }
        if !inventory.result.capabilities.inventory {
            blocks.push(settings_group(
                "Skills",
                vec![settings_empty_row(
                    format!("{provider_name} does not expose Agent Skills here."),
                    theme,
                )],
                theme,
            ));
            return settings_panel("Agent Skills", blocks, theme);
        }

        let mut rows = Vec::new();
        for (index, skill) in inventory.result.skills.iter().enumerate() {
            let busy = self.state.skills_busy.as_deref() == Some(skill.id.as_str());
            let trailing = if busy {
                status_pill("Saving…", false, theme)
            } else if inventory.result.capabilities.configure {
                let id = skill.id.clone();
                let path = project_path.clone();
                let enabled = skill.enabled;
                let view = cx.weak_entity();
                let action: SettingsAction = Rc::new(move |cx| {
                    let id = id.clone();
                    let path = path.clone();
                    let _ = view.update(cx, |this, cx| {
                        let update = this.state.set_skill_enabled(provider, path, id, !enabled);
                        this.apply_client_update(update, cx);
                    });
                });
                settings_switch(930_000 + index, skill.enabled, theme, action)
            } else {
                status_pill(
                    if skill.enabled { "Enabled" } else { "Disabled" },
                    skill.enabled,
                    theme,
                )
            };
            rows.push(settings_row(
                index,
                skill
                    .display_name
                    .clone()
                    .unwrap_or_else(|| skill.name.clone()),
                skill_note(skill),
                trailing,
                theme,
            ));
        }
        if rows.is_empty() {
            rows.push(settings_empty_row(
                "No Agent Skills were discovered for this project.",
                theme,
            ));
        }
        blocks.push(settings_group("Skills", rows, theme));
        settings_panel("Agent Skills", blocks, theme)
    }

    fn workflow_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let inbox = self.state.sidebar_settings.mode == SidebarMode::Inbox;
        let classic_view = cx.weak_entity();
        let classic: SettingsAction = Rc::new(move |cx| {
            let _ = classic_view.update(cx, |this, cx| {
                this.set_sidebar_mode(SidebarMode::Classic, cx)
            });
        });
        let inbox_view = cx.weak_entity();
        let inbox_action: SettingsAction = Rc::new(move |cx| {
            let _ = inbox_view.update(cx, |this, cx| this.set_sidebar_mode(SidebarMode::Inbox, cx));
        });
        let version_picker = div()
            .flex()
            .p(px(2.0))
            .rounded(px(8.0))
            .bg(theme.surface_2.hsla())
            .child(segmented_button(
                "workflow-v1",
                "V1 Classic",
                !inbox,
                theme,
                classic,
            ))
            .child(segmented_button(
                "workflow-v2",
                "V2 Inbox",
                inbox,
                theme,
                inbox_action,
            ))
            .into_any_element();

        let auto_settle = self.state.sidebar_settings.auto_settle_days.is_some();
        let days = self.state.sidebar_settings.auto_settle_days.unwrap_or(3);
        let minus_view = cx.weak_entity();
        let minus: SettingsAction = Rc::new(move |cx| {
            let _ = minus_view.update(cx, |this, cx| {
                this.set_auto_settle_days(Some(days.saturating_sub(1).max(1)), cx)
            });
        });
        let plus_view = cx.weak_entity();
        let plus: SettingsAction = Rc::new(move |cx| {
            let _ = plus_view.update(cx, |this, cx| {
                this.set_auto_settle_days(Some(days.saturating_add(1).min(90)), cx)
            });
        });
        let toggle_view = cx.weak_entity();
        let toggle: SettingsAction = Rc::new(move |cx| {
            let _ = toggle_view.update(cx, |this, cx| {
                this.set_auto_settle_days(if auto_settle { None } else { Some(3) }, cx)
            });
        });
        let settle_control = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(stepper_button(
                "settle-minus",
                "−",
                theme,
                minus,
                !auto_settle,
            ))
            .child(
                div()
                    .min_w(px(52.0))
                    .text_center()
                    .text_size(px(11.5))
                    .text_color(if auto_settle {
                        theme.text_2.hsla()
                    } else {
                        theme.text_3.hsla()
                    })
                    .child(format!("{days}d")),
            )
            .child(stepper_button(
                "settle-plus",
                "+",
                theme,
                plus,
                !auto_settle,
            ))
            .child(settings_switch(900_000, auto_settle, theme, toggle))
            .into_any_element();

        settings_panel(
            "Workflows",
            vec![settings_group(
                "Sidebar",
                vec![
                    settings_row(
                        0,
                        "Sidebar version",
                        "V2 is a stable cross-project inbox with snoozed and settled shelves.",
                        version_picker,
                        theme,
                    ),
                    settings_row(
                        1,
                        "Settle inactive threads",
                        "Move eligible inactive work out of the inbox after this many days.",
                        settle_control,
                        theme,
                    ),
                ],
                theme,
            )],
            theme,
        )
    }

    fn appearance_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let mut blocks = Vec::new();
        let theme_options = [
            (ThemePreference::System, "System"),
            (ThemePreference::Light, "Light"),
            (ThemePreference::Dark, "Dark"),
        ];
        let mut theme_cards = Vec::new();
        for (index, (preference, label)) in theme_options.into_iter().enumerate() {
            let selected = self.preferences.theme == preference;
            let preview_mode = match preference {
                ThemePreference::System => self.system_theme_mode,
                ThemePreference::Light => ThemeMode::Light,
                ThemePreference::Dark => ThemeMode::Dark,
            };
            let preview = Theme::new(
                preview_mode,
                self.preferences.backdrop,
                self.preferences.accent,
            );
            let view = cx.weak_entity();
            let action: SettingsAction = Rc::new(move |cx| {
                let _ = view.update(cx, |this, cx| this.set_theme_preference(preference, cx));
            });
            theme_cards.push(theme_card(index, label, preview, selected, theme, action));
        }
        blocks.push(settings_plain_group(
            "Theme",
            div()
                .w_full()
                .flex()
                .gap(px(16.0))
                .children(theme_cards)
                .into_any_element(),
            theme,
        ));

        let font_options = [
            (FontPreference::Geist, "Geist"),
            (FontPreference::System, "System"),
            (FontPreference::Humanist, "Humanist"),
            (FontPreference::Rounded, "Rounded"),
            (FontPreference::Serif, "Editorial"),
            (FontPreference::Mono, "Mono"),
        ];
        let mut font_choices = Vec::new();
        for (index, (preference, label)) in font_options.into_iter().enumerate() {
            let view = cx.weak_entity();
            let action: SettingsAction = Rc::new(move |cx| {
                let _ = view.update(cx, |this, cx| this.set_font_preference(preference, cx));
            });
            font_choices.push(appearance_choice(
                index,
                label,
                font_family(preference),
                None,
                self.preferences.font == preference,
                theme,
                action,
            ));
        }
        blocks.push(settings_plain_group(
            "Interface font",
            div()
                .flex()
                .flex_wrap()
                .gap(px(10.0))
                .children(font_choices)
                .into_any_element(),
            theme,
        ));

        let backdrop_options = [
            (Backdrop::Default, "Graphite", 0x303035),
            (Backdrop::Slate, "Slate", 0x354252),
            (Backdrop::Mocha, "Mocha", 0x58453e),
            (Backdrop::Forest, "Forest", 0x36503e),
            (Backdrop::Midnight, "Midnight", 0x29375c),
            (Backdrop::Plum, "Plum", 0x513b53),
        ];
        let mut backdrop_choices = Vec::new();
        for (index, (backdrop, label, swatch)) in backdrop_options.into_iter().enumerate() {
            let view = cx.weak_entity();
            let action: SettingsAction = Rc::new(move |cx| {
                let _ = view.update(cx, |this, cx| this.set_backdrop(backdrop, cx));
            });
            backdrop_choices.push(appearance_choice(
                100 + index,
                label,
                "Geist",
                Some(swatch),
                self.preferences.backdrop == backdrop,
                theme,
                action,
            ));
        }
        blocks.push(settings_plain_group(
            "Background",
            div()
                .flex()
                .flex_wrap()
                .gap(px(10.0))
                .children(backdrop_choices)
                .into_any_element(),
            theme,
        ));

        let glass = self.preferences.sidebar_glass;
        let glass_minus_view = cx.weak_entity();
        let glass_minus: SettingsAction = Rc::new(move |cx| {
            let _ = glass_minus_view.update(cx, |this, cx| {
                this.set_sidebar_glass(glass.saturating_sub(5), cx)
            });
        });
        let glass_plus_view = cx.weak_entity();
        let glass_plus: SettingsAction = Rc::new(move |cx| {
            let _ = glass_plus_view.update(cx, |this, cx| {
                this.set_sidebar_glass(glass.saturating_add(5).min(60), cx)
            });
        });
        let glass_toggle_view = cx.weak_entity();
        let glass_toggle: SettingsAction = Rc::new(move |cx| {
            let _ = glass_toggle_view.update(cx, |this, cx| {
                this.set_sidebar_glass(if glass > 0 { 0 } else { 35 }, cx)
            });
        });
        let glass_control = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(stepper_button(
                "glass-minus",
                "−",
                theme,
                glass_minus,
                glass == 0,
            ))
            .child(
                div()
                    .min_w(px(38.0))
                    .text_center()
                    .text_size(px(11.0))
                    .text_color(theme.text_2.hsla())
                    .child(format!("{glass}%")),
            )
            .child(stepper_button(
                "glass-plus",
                "+",
                theme,
                glass_plus,
                glass >= 60,
            ))
            .child(settings_switch(910_000, glass > 0, theme, glass_toggle))
            .into_any_element();
        blocks.push(settings_group(
            "Sidebar",
            vec![settings_row(
                0,
                "Translucent sidebar",
                "Let the selected background palette soften the rail.",
                glass_control,
                theme,
            )],
            theme,
        ));

        let accent_options = [
            (Accent::Neutral, "Neutral", 0x71717a),
            (Accent::Ocean, "Ocean", 0x2d7fbd),
            (Accent::Forest, "Forest", 0x397a56),
            (Accent::Sunset, "Sunset", 0x8b63bd),
            (Accent::Amber, "Amber", 0x9a6823),
            (Accent::Rose, "Rose", 0x9a4b6a),
            (Accent::Lavender, "Lavender", 0x6658a6),
        ];
        let mut accent_choices = Vec::new();
        for (index, (accent, label, swatch)) in accent_options.into_iter().enumerate() {
            let view = cx.weak_entity();
            let action: SettingsAction = Rc::new(move |cx| {
                let _ = view.update(cx, |this, cx| this.set_accent(accent, cx));
            });
            accent_choices.push(appearance_choice(
                200 + index,
                label,
                "Geist",
                Some(swatch),
                self.preferences.accent == accent,
                theme,
                action,
            ));
        }
        blocks.push(settings_plain_group(
            "Accent palette",
            div()
                .flex()
                .flex_wrap()
                .gap(px(10.0))
                .children(accent_choices)
                .into_any_element(),
            theme,
        ));

        #[cfg(target_os = "macos")]
        blocks.push(settings_group(
            "Text rendering",
            vec![settings_row(
                0,
                "Font smoothing",
                "GPUI uses native macOS antialiasing and subpixel positioning.",
                status_pill("On", true, theme),
                theme,
            )],
            theme,
        ));

        settings_panel("Appearance", blocks, theme)
    }

    fn data_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let project_count = self.state.projects.len();
        let reset_view = cx.weak_entity();
        let reset: SettingsAction = Rc::new(move |cx| {
            let _ = reset_view.update(cx, |this, cx| this.reset_native_preferences(cx));
        });
        settings_panel(
            "Data",
            vec![settings_group(
                "Local data",
                vec![settings_row(
                    0,
                    format!(
                        "{project_count} {} on this machine",
                        if project_count == 1 {
                            "project"
                        } else {
                            "projects"
                        }
                    ),
                    "Reset clears GPUI appearance and model-visibility choices. Projects, sessions, and credentials stay untouched.",
                    settings_button(
                        "reset-native-settings",
                        "Reset app",
                        "icons/rotate-ccw.svg",
                        theme,
                        reset,
                        true,
                    ),
                    theme,
                )],
                theme,
            )],
            theme,
        )
    }

    fn about_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let product_note = self
            .state
            .update_check
            .as_ref()
            .and_then(|result| result.local_commit.as_deref())
            .map_or_else(
                || "Desktop · pre-release".to_owned(),
                |commit| format!("Desktop · pre-release · {}", short_commit(commit)),
            );
        let update_note = update_check_note(self.state.update_check.as_ref());
        let checking = self.state.update_checking;
        let check_view = cx.weak_entity();
        let check: SettingsAction = Rc::new(move |cx| {
            let _ = check_view.update(cx, |this, cx| {
                let update = this.state.request_update_check();
                this.apply_client_update(update, cx);
            });
        });
        let source: SettingsAction = Rc::new(|cx| {
            cx.open_url("https://github.com/Leonxlnx/personalharness");
        });
        settings_panel(
            "About",
            vec![settings_group(
                "Personal Harness",
                vec![
                    settings_row(
                        0,
                        "Personal Harness",
                        product_note,
                        div().into_any_element(),
                        theme,
                    ),
                    settings_row(
                        1,
                        "Updates",
                        update_note,
                        settings_button_enabled(
                            "check-for-updates",
                            if checking {
                                "Checking…"
                            } else {
                                "Check for updates"
                            },
                            "icons/rotate-ccw.svg",
                            theme,
                            check,
                            false,
                            !checking,
                        ),
                        theme,
                    ),
                    settings_row(
                        2,
                        "Source",
                        "Open source, and built to be forked.",
                        provider_action_button(0, "GitHub", false, theme, source),
                        theme,
                    ),
                ],
                theme,
            )],
            theme,
        )
    }

    fn refresh_settings_inventory(&mut self, cx: &mut Context<Self>) {
        let Some((provider, project_path)) = self.settings_scope() else {
            cx.notify();
            return;
        };
        let update = match self.settings_section {
            SettingsSection::Mcp => self.state.request_mcp_inventory(provider, project_path),
            SettingsSection::Skills => self.state.request_skills_inventory(provider, project_path),
            _ => {
                cx.notify();
                return;
            }
        };
        self.apply_client_update(update, cx);
    }

    fn settings_scope(&self) -> Option<(ProviderId, String)> {
        let provider = self
            .selected_model_choice()
            .map_or(ProviderId::Codex, |choice| choice.provider);
        let project_path = self
            .active_project_path
            .clone()
            .or_else(|| self.sidebar_scope.clone())
            .or_else(|| {
                self.state
                    .projects
                    .first()
                    .map(|project| project.path.clone())
            })?;
        Some((provider, project_path))
    }

    fn settings_project_name(&self, project_path: &str) -> String {
        self.state
            .projects
            .iter()
            .find(|project| project.path == project_path)
            .map_or_else(|| project_path.to_owned(), |project| project.name.clone())
    }

    fn settings_provider_name(&self, provider: ProviderId) -> String {
        self.state
            .provider_statuses
            .iter()
            .find(|status| status.id == provider)
            .map(|status| status.display_name.clone())
            .or_else(|| {
                self.state
                    .model_catalog
                    .iter()
                    .find(|choice| choice.provider == provider)
                    .map(|choice| choice.source_name.clone())
            })
            .unwrap_or_else(|| provider_label(provider).into())
    }

    fn pick_skill_folder(
        &mut self,
        provider: ProviderId,
        project_path: String,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Install Agent Skill".into()),
        });
        cx.spawn(async move |view, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
                return;
            };
            let folder_path = folder.to_string_lossy().into_owned();
            let _ = view.update(cx, |this, cx| {
                let update =
                    this.state
                        .install_skill_from_folder(provider, project_path, folder_path);
                this.apply_client_update(update, cx);
            });
        })
        .detach();
    }

    fn set_models_visible(&mut self, keys: &[String], visible: bool, cx: &mut Context<Self>) {
        for key in keys {
            if visible {
                self.preferences.hidden_models.remove(key);
            } else {
                self.preferences.hidden_models.insert(key.clone());
            }
        }
        self.sync_model_selection();
        self.sync_composer_settings(cx);
        self.persist_native_preferences();
        cx.notify();
    }

    fn set_sidebar_mode(&mut self, mode: SidebarMode, cx: &mut Context<Self>) {
        if self.state.sidebar_settings.mode == mode {
            return;
        }
        let settings = SidebarSettings {
            mode,
            auto_settle_days: self.state.sidebar_settings.auto_settle_days,
        };
        let update = self.state.update_sidebar_settings(settings);
        self.apply_client_update(update, cx);
    }

    fn set_auto_settle_days(&mut self, days: Option<u8>, cx: &mut Context<Self>) {
        let days = days.map(|days| days.clamp(1, 90));
        if self.state.sidebar_settings.auto_settle_days == days {
            return;
        }
        let settings = SidebarSettings {
            mode: self.state.sidebar_settings.mode,
            auto_settle_days: days,
        };
        let update = self.state.update_sidebar_settings(settings);
        self.apply_client_update(update, cx);
    }

    fn set_theme_preference(&mut self, preference: ThemePreference, cx: &mut Context<Self>) {
        self.preferences.theme = preference;
        self.apply_native_theme(cx);
    }

    fn set_font_preference(&mut self, preference: FontPreference, cx: &mut Context<Self>) {
        self.preferences.font = preference;
        self.persist_native_preferences();
        cx.notify();
    }

    fn set_backdrop(&mut self, backdrop: Backdrop, cx: &mut Context<Self>) {
        self.preferences.backdrop = backdrop;
        self.apply_native_theme(cx);
    }

    fn set_accent(&mut self, accent: Accent, cx: &mut Context<Self>) {
        self.preferences.accent = accent;
        self.apply_native_theme(cx);
    }

    fn set_sidebar_glass(&mut self, glass: u8, cx: &mut Context<Self>) {
        self.preferences.sidebar_glass = glass.min(60);
        self.persist_native_preferences();
        cx.notify();
    }

    pub(super) fn apply_native_theme(&mut self, cx: &mut Context<Self>) {
        let mode = match self.preferences.theme {
            ThemePreference::System => self.system_theme_mode,
            ThemePreference::Light => ThemeMode::Light,
            ThemePreference::Dark => ThemeMode::Dark,
        };
        self.theme = Theme::new(mode, self.preferences.backdrop, self.preferences.accent);
        super::sync_component_theme(self.theme, cx);
        let theme = self.theme;
        self.chat
            .update(cx, |chat, cx| chat.update_theme(theme, cx));
        self.update_provider_terminal_themes(cx);
        self.persist_native_preferences();
        cx.notify();
    }

    fn reset_native_preferences(&mut self, cx: &mut Context<Self>) {
        self.preferences = NativePreferences::default();
        match NativePreferences::reset_file() {
            Ok(()) => self.state.notice = Some("Native preferences were reset.".into()),
            Err(error) => self.state.notice = Some(format!("Could not reset preferences: {error}")),
        }
        self.apply_native_theme(cx);
        self.sync_model_selection();
        self.sync_composer_settings(cx);
    }

    pub(super) fn persist_native_preferences(&mut self) {
        if let Err(error) = self.preferences.save() {
            self.state.notice = Some(format!("Could not save native preferences: {error}"));
        }
    }

    pub(super) fn interface_font(&self) -> &'static str {
        font_family(self.preferences.font)
    }
}

fn settings_panel(title: &str, blocks: Vec<AnyElement>, theme: Theme) -> gpui::Div {
    div()
        .w_full()
        .child(
            div()
                .mb(px(40.0))
                .text_size(px(24.0))
                .line_height(px(29.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text.hsla())
                .child(title.to_owned()),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(SETTINGS_SECTION_GAP))
                .children(blocks),
        )
}

fn settings_group(title: &str, rows: Vec<AnyElement>, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .child(
            div()
                .mb(px(12.0))
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_2.hsla())
                .child(title.to_owned()),
        )
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .rounded(px(10.0))
                .border_1()
                .border_color(theme.line_strong.hsla())
                .bg(theme.rail.hsla())
                .children(rows),
        )
        .into_any_element()
}

fn settings_plain_group(title: &str, child: AnyElement, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .child(
            div()
                .mb(px(12.0))
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_2.hsla())
                .child(title.to_owned()),
        )
        .child(child)
        .into_any_element()
}

fn settings_error_group(title: &str, message: String, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .child(
            div()
                .mb(px(12.0))
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.error.hsla())
                .child(title.to_owned()),
        )
        .child(
            div()
                .w_full()
                .rounded(px(10.0))
                .border_1()
                .border_color(theme.error.hsla().opacity(0.35))
                .bg(theme.error.hsla().opacity(0.08))
                .px(px(16.0))
                .py(px(12.0))
                .text_size(px(11.0))
                .line_height(px(16.0))
                .text_color(theme.error.hsla())
                .child(message),
        )
        .into_any_element()
}

fn settings_notice_group(title: &str, message: String, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .child(
            div()
                .mb(px(12.0))
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.success.hsla())
                .child(title.to_owned()),
        )
        .child(
            div()
                .w_full()
                .rounded(px(10.0))
                .border_1()
                .border_color(theme.success.hsla().opacity(0.3))
                .bg(theme.success.hsla().opacity(0.07))
                .px(px(16.0))
                .py(px(12.0))
                .text_size(px(11.0))
                .line_height(px(16.0))
                .text_color(theme.text_2.hsla())
                .child(message),
        )
        .into_any_element()
}

fn settings_row(
    index: usize,
    title: impl Into<SharedString>,
    note: impl Into<SharedString>,
    trailing: AnyElement,
    theme: Theme,
) -> AnyElement {
    div()
        .min_h(px(66.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(20.0))
        .px(px(16.0))
        .py(px(12.0))
        .when(index > 0, |row| {
            row.border_t_1().border_color(theme.line.hsla())
        })
        .hover(move |style| style.bg(theme.surface.hsla()))
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text.hsla())
                        .child(title.into()),
                )
                .child(
                    div()
                        .mt(px(2.0))
                        .text_size(px(11.0))
                        .line_height(px(16.0))
                        .text_color(theme.text_3.hsla())
                        .child(note.into()),
                ),
        )
        .child(div().flex_none().child(trailing))
        .into_any_element()
}

fn settings_empty_row(note: impl Into<SharedString>, theme: Theme) -> AnyElement {
    div()
        .min_h(px(66.0))
        .w_full()
        .flex()
        .items_center()
        .px(px(16.0))
        .py(px(12.0))
        .text_size(px(11.5))
        .line_height(px(17.0))
        .text_color(theme.text_3.hsla())
        .child(note.into())
        .into_any_element()
}

fn settings_nav_item(
    index: usize,
    label: &'static str,
    icon: &'static str,
    selected: bool,
    theme: Theme,
    action: SettingsAction,
) -> AnyElement {
    div()
        .id(("settings-nav", index))
        .min_h(px(32.0))
        .w_full()
        .flex()
        .items_center()
        .gap(px(9.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(8.0))
        .bg(if selected {
            theme.surface_2.hsla()
        } else {
            theme.rail.hsla().opacity(0.0)
        })
        .text_size(px(12.0))
        .text_color(if selected {
            theme.text.hsla()
        } else {
            theme.text_2.hsla()
        })
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(theme.surface_2.hsla())
                .text_color(theme.text.hsla())
        })
        .active(|style| style.opacity(0.72))
        .on_click(move |_event, _window, cx| action(cx))
        .child(
            div()
                .size(px(15.0))
                .text_color(if selected {
                    theme.text_2.hsla()
                } else {
                    theme.text_3.hsla()
                })
                .child(settings_icon(icon, 15.0)),
        )
        .child(label)
        .into_any_element()
}

fn settings_switch(id: usize, on: bool, theme: Theme, action: SettingsAction) -> AnyElement {
    div()
        .id(("settings-switch", id))
        .relative()
        .w(px(32.0))
        .h(px(18.0))
        .flex_none()
        .rounded(px(9.0))
        .bg(if on {
            theme.attention.hsla()
        } else {
            theme.surface_3.hsla()
        })
        .cursor_pointer()
        .active(|style| style.opacity(0.72))
        .on_click(move |_event, _window, cx| action(cx))
        .child(
            div()
                .absolute()
                .top(px(2.0))
                .left(px(if on { 16.0 } else { 2.0 }))
                .size(px(14.0))
                .rounded(px(7.0))
                .bg(gpui::white()),
        )
        .into_any_element()
}

fn segmented_button(
    id: &'static str,
    label: &'static str,
    selected: bool,
    theme: Theme,
    action: SettingsAction,
) -> AnyElement {
    div()
        .id(id)
        .h(px(28.0))
        .flex()
        .items_center()
        .px(px(10.0))
        .rounded(px(6.0))
        .bg(if selected {
            theme.rail.hsla()
        } else {
            theme.surface_2.hsla().opacity(0.0)
        })
        .text_size(px(10.5))
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(if selected {
            theme.text.hsla()
        } else {
            theme.text_3.hsla()
        })
        .cursor_pointer()
        .hover(move |style| style.text_color(theme.text.hsla()))
        .on_click(move |_event, _window, cx| action(cx))
        .child(label)
        .into_any_element()
}

fn stepper_button(
    id: &'static str,
    label: &'static str,
    theme: Theme,
    action: SettingsAction,
    disabled: bool,
) -> AnyElement {
    div()
        .id(id)
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .border_1()
        .border_color(theme.line_strong.hsla())
        .bg(theme.surface.hsla())
        .text_size(px(13.0))
        .text_color(if disabled {
            theme.text_3.hsla().opacity(0.5)
        } else {
            theme.text_2.hsla()
        })
        .when(!disabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_2.hsla()))
                .on_click(move |_event, _window, cx| action(cx))
        })
        .child(label)
        .into_any_element()
}

fn settings_button(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    theme: Theme,
    action: SettingsAction,
    destructive: bool,
) -> AnyElement {
    settings_button_enabled(id, label, icon, theme, action, destructive, true)
}

fn settings_button_enabled(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    theme: Theme,
    action: SettingsAction,
    destructive: bool,
    enabled: bool,
) -> AnyElement {
    let text = if destructive {
        theme.error.hsla()
    } else {
        theme.text_2.hsla()
    };
    div()
        .id(id)
        .h(px(30.0))
        .flex()
        .items_center()
        .gap(px(7.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.line_strong.hsla())
        .bg(theme.surface.hsla())
        .text_size(px(11.0))
        .text_color(text)
        .opacity(if enabled { 1.0 } else { 0.48 })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_2.hsla()))
                .active(|style| style.opacity(0.72))
                .on_click(move |_event, _window, cx| action(cx))
        })
        .child(settings_icon(icon, 13.0))
        .child(label)
        .into_any_element()
}

fn connection_remove_button(index: usize, theme: Theme, action: SettingsAction) -> AnyElement {
    div()
        .id(("remove-connection", index))
        .h(px(26.0))
        .flex()
        .items_center()
        .px(px(9.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(theme.error.hsla().opacity(0.32))
        .text_size(px(10.5))
        .text_color(theme.error.hsla())
        .cursor_pointer()
        .hover(move |style| style.bg(theme.error.hsla().opacity(0.09)))
        .active(|style| style.opacity(0.72))
        .on_click(move |_event, _window, cx| action(cx))
        .child("Remove")
        .into_any_element()
}

fn provider_action_button(
    index: usize,
    label: &'static str,
    destructive: bool,
    theme: Theme,
    action: SettingsAction,
) -> AnyElement {
    div()
        .id(("provider-action", index))
        .h(px(28.0))
        .flex()
        .items_center()
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(if destructive {
            theme.error.hsla().opacity(0.32)
        } else {
            theme.line_strong.hsla()
        })
        .bg(theme.surface.hsla())
        .text_size(px(10.5))
        .text_color(if destructive {
            theme.error.hsla()
        } else {
            theme.text_2.hsla()
        })
        .cursor_pointer()
        .hover(move |style| {
            style.bg(if destructive {
                theme.error.hsla().opacity(0.09)
            } else {
                theme.surface_2.hsla()
            })
        })
        .active(|style| style.opacity(0.72))
        .on_click(move |_event, _window, cx| action(cx))
        .child(label)
        .into_any_element()
}

fn mcp_action_button(
    id: SharedString,
    label: SharedString,
    destructive: bool,
    enabled: bool,
    theme: Theme,
    action: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .h(px(28.0))
        .flex()
        .items_center()
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(if destructive {
            theme.error.hsla().opacity(0.32)
        } else {
            theme.line_strong.hsla()
        })
        .bg(theme.surface.hsla())
        .text_size(px(10.5))
        .text_color(if destructive {
            theme.error.hsla()
        } else {
            theme.text_2.hsla()
        })
        .opacity(if enabled { 1.0 } else { 0.48 })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| {
                    style.bg(if destructive {
                        theme.error.hsla().opacity(0.09)
                    } else {
                        theme.surface_2.hsla()
                    })
                })
                .active(|style| style.opacity(0.72))
                .on_click(action)
        })
        .child(label)
        .into_any_element()
}

fn mcp_badge(label: &'static str, theme: Theme) -> AnyElement {
    div()
        .px(px(6.0))
        .py(px(1.0))
        .rounded(px(8.0))
        .bg(theme.surface_2.hsla())
        .text_size(px(9.5))
        .text_color(theme.text_3.hsla())
        .child(label)
        .into_any_element()
}

fn mcp_transport_label(transport: Option<&McpTransport>) -> String {
    match transport {
        Some(McpTransport::Http { url, .. }) => url.clone(),
        Some(McpTransport::Stdio { command, args, .. }) => std::iter::once(command.as_str())
            .chain(args.iter().flatten().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        None => "Configuration managed by provider".into(),
    }
}

fn mcp_editor_field(
    label: &'static str,
    state: &Entity<InputState>,
    disabled: bool,
    multiline: bool,
    theme: Theme,
) -> AnyElement {
    div()
        .min_w(px(0.0))
        .flex_1()
        .when(multiline, |field| field.w_full())
        .child(
            div()
                .mb(px(6.0))
                .text_size(px(10.5))
                .text_color(theme.text_2.hsla())
                .child(label),
        )
        .child(
            Input::new(state)
                .appearance(false)
                .bordered(false)
                .focus_bordered(false)
                .disabled(disabled)
                .h(px(if multiline { 142.0 } else { 34.0 }))
                .w_full()
                .px(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.line_strong.hsla())
                .bg(theme.surface.hsla())
                .text_size(px(11.5))
                .text_color(theme.text.hsla()),
        )
        .into_any_element()
}

fn connection_field(
    label: &'static str,
    state: &Entity<InputState>,
    wide: bool,
    theme: Theme,
) -> AnyElement {
    div()
        .min_w(px(if wide { 0.0 } else { 220.0 }))
        .when(wide, |field| field.w_full().flex_none())
        .when(!wide, |field| field.flex_1())
        .child(
            div()
                .mb(px(6.0))
                .text_size(px(10.5))
                .text_color(theme.text_3.hsla())
                .child(label),
        )
        .child(
            Input::new(state)
                .appearance(false)
                .bordered(false)
                .focus_bordered(false)
                .h(px(34.0))
                .w_full()
                .px(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.line_strong.hsla())
                .bg(theme.surface.hsla())
                .text_size(px(11.5))
                .text_color(theme.text.hsla()),
        )
        .into_any_element()
}

#[derive(Clone, Copy)]
struct ConnectionPresetConfig {
    label: &'static str,
    base_url: &'static str,
    transport: ModelTransport,
}

fn connection_preset(preset: ModelConnectionPreset) -> ConnectionPresetConfig {
    match preset {
        ModelConnectionPreset::Openai => ConnectionPresetConfig {
            label: "OpenAI API",
            base_url: "https://api.openai.com/v1",
            transport: ModelTransport::OpenaiResponses,
        },
        ModelConnectionPreset::Anthropic => ConnectionPresetConfig {
            label: "Anthropic API",
            base_url: "https://api.anthropic.com/v1",
            transport: ModelTransport::AnthropicMessages,
        },
        ModelConnectionPreset::Openrouter => ConnectionPresetConfig {
            label: "OpenRouter",
            base_url: "https://openrouter.ai/api/v1",
            transport: ModelTransport::OpenaiCompatible,
        },
        ModelConnectionPreset::Kimi => ConnectionPresetConfig {
            label: "Kimi API",
            base_url: "https://api.moonshot.ai/v1",
            transport: ModelTransport::OpenaiCompatible,
        },
        ModelConnectionPreset::Zai => ConnectionPresetConfig {
            label: "Z.ai API",
            base_url: "https://api.z.ai/api/paas/v4",
            transport: ModelTransport::OpenaiCompatible,
        },
        ModelConnectionPreset::Custom => ConnectionPresetConfig {
            label: "Custom endpoint",
            base_url: "http://127.0.0.1:11434/v1",
            transport: ModelTransport::OpenaiCompatible,
        },
    }
}

fn connection_preset_id(preset: ModelConnectionPreset) -> &'static str {
    match preset {
        ModelConnectionPreset::Openai => "openai",
        ModelConnectionPreset::Anthropic => "anthropic",
        ModelConnectionPreset::Openrouter => "openrouter",
        ModelConnectionPreset::Kimi => "kimi",
        ModelConnectionPreset::Zai => "zai",
        ModelConnectionPreset::Custom => "custom",
    }
}

fn validate_model_endpoint(value: &str) -> Result<(), String> {
    let url = url::Url::parse(value).map_err(|_| "Base URL must be a valid URL.".to_owned())?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if url.host_str() == Some("127.0.0.1") => Ok(()),
        _ => Err("Use HTTPS, or loopback HTTP on 127.0.0.1 for a local server.".into()),
    }
}

fn validate_mcp_transport(transport: &McpTransport) -> Result<(), String> {
    match transport {
        McpTransport::Stdio {
            command,
            cwd,
            environment,
            ..
        } => {
            if command.trim().is_empty() {
                return Err("A stdio MCP transport needs a command.".into());
            }
            if cwd.as_ref().is_some_and(|cwd| cwd.trim().is_empty()) {
                return Err("The MCP working directory cannot be empty.".into());
            }
            validate_mcp_values(environment.as_ref(), "environment variable")
        }
        McpTransport::Http { url, headers } => {
            let parsed = url::Url::parse(url)
                .map_err(|_| "The MCP HTTP transport needs a valid URL.".to_owned())?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
                return Err("The MCP transport URL must use HTTP or HTTPS.".into());
            }
            validate_mcp_values(headers.as_ref(), "header")
        }
    }
}

fn validate_mcp_values(
    values: Option<&std::collections::BTreeMap<String, McpConfigValue>>,
    label: &str,
) -> Result<(), String> {
    for (name, value) in values.into_iter().flatten() {
        if name.trim().is_empty() {
            return Err(format!("An MCP {label} name cannot be empty."));
        }
        if let McpConfigValue::Credential { credential_ref } = value
            && credential_ref.trim().is_empty()
        {
            return Err(format!(
                "The credentialRef for MCP {label} {name} is empty."
            ));
        }
    }
    Ok(())
}

fn status_pill(label: &'static str, ready: bool, theme: Theme) -> AnyElement {
    div()
        .h(px(24.0))
        .flex()
        .items_center()
        .px(px(9.0))
        .rounded(px(12.0))
        .bg(if ready {
            theme.success.hsla().opacity(0.12)
        } else {
            theme.surface_2.hsla()
        })
        .text_size(px(10.5))
        .text_color(if ready {
            theme.success.hsla()
        } else {
            theme.text_3.hsla()
        })
        .child(label)
        .into_any_element()
}

fn theme_card(
    index: usize,
    label: &'static str,
    preview: Theme,
    selected: bool,
    theme: Theme,
    action: SettingsAction,
) -> AnyElement {
    div()
        .id(("theme-card", index))
        .min_w(px(0.0))
        .flex_1()
        .cursor_pointer()
        .on_click(move |_event, _window, cx| action(cx))
        .child(
            div()
                .relative()
                .h(px(104.0))
                .w_full()
                .overflow_hidden()
                .rounded(px(10.0))
                .border_2()
                .border_color(if selected {
                    theme.attention.hsla()
                } else {
                    theme.line_strong.hsla()
                })
                .bg(preview.background.hsla())
                .child(
                    div()
                        .h(px(20.0))
                        .w_full()
                        .bg(preview.titlebar.hsla())
                        .border_b_1()
                        .border_color(preview.line.hsla()),
                )
                .child(
                    div()
                        .m(px(12.0))
                        .h(px(52.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(preview.line.hsla())
                        .bg(preview.rail.hsla())
                        .child(
                            div()
                                .m(px(9.0))
                                .h(px(5.0))
                                .w(px(48.0))
                                .rounded(px(3.0))
                                .bg(preview.text_2.hsla().opacity(0.5)),
                        )
                        .child(
                            div()
                                .mx(px(9.0))
                                .h(px(4.0))
                                .w(px(72.0))
                                .rounded(px(2.0))
                                .bg(preview.text_3.hsla().opacity(0.45)),
                        ),
                ),
        )
        .child(
            div()
                .mt(px(8.0))
                .flex()
                .items_center()
                .gap(px(7.0))
                .text_size(px(11.0))
                .text_color(if selected {
                    theme.text.hsla()
                } else {
                    theme.text_3.hsla()
                })
                .child(
                    div()
                        .size(px(12.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(if selected {
                            theme.attention.hsla()
                        } else {
                            theme.line_strong.hsla()
                        })
                        .bg(if selected {
                            theme.attention.hsla()
                        } else {
                            theme.background.hsla()
                        })
                        .when(selected, |mark| {
                            mark.child(settings_icon("icons/check.svg", 8.0))
                        }),
                )
                .child(label),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn appearance_choice(
    id: usize,
    label: &'static str,
    preview_font: &'static str,
    swatch: Option<u32>,
    selected: bool,
    theme: Theme,
    action: SettingsAction,
) -> AnyElement {
    let preview = match swatch {
        Some(color) => div()
            .size(px(34.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.line_strong.hsla())
            .bg(gpui::rgb(color))
            .into_any_element(),
        None => div()
            .size(px(34.0))
            .flex()
            .items_center()
            .justify_center()
            .font_family(preview_font)
            .text_size(px(16.0))
            .text_color(theme.text.hsla())
            .child("Ag")
            .into_any_element(),
    };
    div()
        .id(("appearance-choice", id))
        .w(px(104.0))
        .min_h(px(68.0))
        .flex()
        .items_center()
        .gap(px(9.0))
        .px(px(10.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(if selected {
            theme.attention.hsla()
        } else {
            theme.line.hsla()
        })
        .bg(if selected {
            theme.surface_2.hsla()
        } else {
            theme.rail.hsla()
        })
        .text_size(px(10.5))
        .text_color(if selected {
            theme.text.hsla()
        } else {
            theme.text_3.hsla()
        })
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface_2.hsla()))
        .active(|style| style.opacity(0.72))
        .on_click(move |_event, _window, cx| action(cx))
        .child(preview)
        .child(label)
        .into_any_element()
}

fn font_family(preference: FontPreference) -> &'static str {
    match preference {
        FontPreference::Geist => "Geist",
        FontPreference::System => ".SystemUIFont",
        FontPreference::Humanist => {
            if cfg!(target_os = "windows") {
                "Segoe UI"
            } else {
                "Avenir Next"
            }
        }
        FontPreference::Rounded => {
            if cfg!(target_os = "windows") {
                "Arial Rounded MT Bold"
            } else {
                "SF Pro Rounded"
            }
        }
        FontPreference::Serif => {
            if cfg!(target_os = "windows") {
                "Georgia"
            } else {
                "Charter"
            }
        }
        FontPreference::Mono => "Geist Mono",
    }
}

fn mcp_status(server: &McpServer) -> (&'static str, bool) {
    if !server.enabled {
        return ("Disabled", false);
    }
    if matches!(server.auth, McpAuth::SignInRequired { .. }) {
        return ("Sign-in required", false);
    }
    match server.startup {
        McpStartupStatus::Ready => ("Ready", true),
        McpStartupStatus::Starting => ("Starting", false),
        McpStartupStatus::Stopped => ("Stopped", false),
        McpStartupStatus::Failed { .. } => ("Failed", false),
    }
}

fn skill_note(skill: &Skill) -> String {
    let scope = match skill.scope {
        SkillScope::Project => "Project",
        SkillScope::User => "User",
        SkillScope::System => "System",
        SkillScope::Admin => "Admin",
    };
    if skill.dependency_errors.is_empty() {
        if skill.description.is_empty() {
            format!("{scope} skill")
        } else {
            format!("{scope} · {}", skill.description)
        }
    } else {
        let errors = skill
            .dependency_errors
            .iter()
            .map(|error| format!("{}: {}", error.dependency, error.message))
            .collect::<Vec<_>>()
            .join("; ");
        format!("{scope} · {errors}")
    }
}

fn provider_label(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "Codex",
        ProviderId::ClaudeCode => "Claude Code",
        ProviderId::Cursor => "Cursor",
        ProviderId::OpenCode => "OpenCode",
        ProviderId::Acp => "ACP",
        ProviderId::Api => "Direct API",
    }
}

fn short_commit(commit: &str) -> String {
    commit.chars().take(7).collect()
}

fn update_check_note(result: Option<&UpdateCheckResult>) -> String {
    let Some(result) = result else {
        return "Compare this build with the latest commit on GitHub.".into();
    };
    if let Some(error) = &result.error {
        return error.clone();
    }
    if result.up_to_date == Some(true) {
        let remote = result
            .remote
            .as_ref()
            .map_or_else(String::new, |remote| short_commit(&remote.sha));
        return format!("Up to date · {remote} is the newest commit.");
    }
    if let Some(remote) = &result.remote {
        let message = if remote.message.is_empty() {
            String::new()
        } else {
            format!(": \"{}\"", remote.message)
        };
        return format!(
            "Newer commit on GitHub{message} ({}). Pull and restart to update.",
            short_commit(&remote.sha)
        );
    }
    "Could not determine a verdict.".into()
}

fn settings_icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_endpoints_require_https_or_literal_loopback_http() {
        assert!(validate_model_endpoint("https://api.example.com/v1").is_ok());
        assert!(validate_model_endpoint("http://127.0.0.1:11434/v1").is_ok());
        assert!(validate_model_endpoint("http://localhost:11434/v1").is_err());
        assert!(validate_model_endpoint("http://api.example.com/v1").is_err());
    }

    #[test]
    fn connection_presets_keep_the_web_transport_contract() {
        assert_eq!(
            connection_preset(ModelConnectionPreset::Openai).transport,
            ModelTransport::OpenaiResponses
        );
        assert_eq!(
            connection_preset(ModelConnectionPreset::Anthropic).transport,
            ModelTransport::AnthropicMessages
        );
        assert_eq!(
            connection_preset(ModelConnectionPreset::Custom).base_url,
            "http://127.0.0.1:11434/v1"
        );
    }

    #[test]
    fn mcp_transport_validation_matches_the_wire_contract() {
        assert!(
            validate_mcp_transport(&McpTransport::Http {
                url: "https://example.com/mcp".into(),
                headers: None,
            })
            .is_ok()
        );
        assert!(
            validate_mcp_transport(&McpTransport::Http {
                url: "file:///tmp/mcp".into(),
                headers: None,
            })
            .is_err()
        );
        assert!(
            validate_mcp_transport(&McpTransport::Stdio {
                command: " ".into(),
                args: None,
                cwd: None,
                environment: None,
            })
            .is_err()
        );
    }

    #[test]
    fn mcp_credential_references_cannot_be_empty() {
        let headers = std::collections::BTreeMap::from([(
            "Authorization".into(),
            McpConfigValue::Credential {
                credential_ref: "".into(),
            },
        )]);
        assert!(
            validate_mcp_transport(&McpTransport::Http {
                url: "https://example.com/mcp".into(),
                headers: Some(headers),
            })
            .is_err()
        );
    }

    #[test]
    fn about_update_copy_matches_the_web_surface() {
        assert_eq!(
            update_check_note(None),
            "Compare this build with the latest commit on GitHub."
        );
        assert_eq!(
            update_check_note(Some(&UpdateCheckResult {
                local_commit: Some("111111111".into()),
                remote: Some(harness_protocol::UpdateRemote {
                    sha: "222222222".into(),
                    message: "Latest change".into(),
                    date: "2026-08-06T12:00:00Z".into(),
                }),
                up_to_date: Some(false),
                error: None,
            })),
            "Newer commit on GitHub: \"Latest change\" (2222222). Pull and restart to update."
        );
    }
}
