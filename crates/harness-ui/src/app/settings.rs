use super::HarnessApp;
use crate::preferences::{FontPreference, NativePreferences, ThemePreference};
use crate::theme::{Accent, Backdrop, Theme, ThemeMode};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, FontWeight, SharedString, div,
    ease_out_quint, prelude::*, px, svg,
};
use harness_protocol::{ProviderAuth, SidebarMode, SidebarSettings};
use std::rc::Rc;

const SETTINGS_CONTENT_WIDTH: f32 = 840.0;
const SETTINGS_SECTION_GAP: f32 = 30.0;

type SettingsAction = Rc<dyn Fn(&mut App)>;

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
                        this.settings_transition = this.settings_transition.wrapping_add(1);
                        cx.notify();
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
            Animation::new(std::time::Duration::from_millis(220)).with_easing(ease_out_quint()),
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
                Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                |panel, delta| panel.opacity(delta),
            )
            .into_any_element()
    }

    fn settings_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        match self.settings_section {
            SettingsSection::Providers => self.provider_settings(cx),
            SettingsSection::Models => self.model_settings(cx),
            SettingsSection::Mcp => self.unported_settings(
                "MCP",
                "Model Context Protocol",
                "The native MCP registry is the next settings slice. No server is shown as enabled until its Rust transport is connected.",
            ),
            SettingsSection::Skills => self.unported_settings(
                "Skills",
                "Agent skills",
                "Skill discovery is still served by the existing runtime. The GPUI registry will expose every discovered skill and its project scope here.",
            ),
            SettingsSection::Workflows => self.workflow_settings(cx),
            SettingsSection::Appearance => self.appearance_settings(cx),
            SettingsSection::Data => self.data_settings(cx),
            SettingsSection::About => self.about_settings(),
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
            let ready = provider.installed
                && provider.problem.is_none()
                && provider.auth != ProviderAuth::Unauthenticated;
            let status = if !provider.installed {
                "Not installed"
            } else if provider.problem.is_some() {
                "Needs attention"
            } else {
                match provider.auth {
                    ProviderAuth::Authenticated => "Ready",
                    ProviderAuth::Unauthenticated => "Sign-in required",
                    ProviderAuth::Unknown => "Installed",
                }
            };
            let note = provider.problem.clone().unwrap_or_else(|| {
                provider.version.as_ref().map_or_else(
                    || "Local provider adapter".into(),
                    |version| format!("Version {version}"),
                )
            });
            providers.push(settings_row(
                index,
                provider.display_name.clone(),
                note,
                status_pill(status, ready, theme),
                theme,
            ));
        }
        if providers.is_empty() {
            providers.push(settings_empty_row(
                "Provider discovery is waiting for the local Harness server.",
                theme,
            ));
        }
        blocks.push(settings_group("CLI providers", providers, theme));

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
            connections.push(settings_row(
                index,
                connection.display_name.clone(),
                connection
                    .problem
                    .clone()
                    .unwrap_or_else(|| connection.base_url.clone()),
                status_pill(
                    status,
                    connection.enabled
                        && connection.credential_configured
                        && connection.problem.is_none(),
                    theme,
                ),
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

        let mut agents = Vec::new();
        for (index, agent) in self.state.acp_agents.iter().enumerate() {
            let status = if agent.installed && agent.verified {
                "Ready"
            } else if agent.installed {
                "Unverified"
            } else {
                "Not installed"
            };
            agents.push(settings_row(
                index,
                agent.name.clone(),
                agent
                    .problem
                    .clone()
                    .unwrap_or_else(|| "ACP-compatible coding agent".into()),
                status_pill(status, agent.installed && agent.verified, theme),
                theme,
            ));
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

    fn about_settings(&self) -> gpui::Div {
        let theme = self.theme;
        settings_panel(
            "About",
            vec![settings_group(
                "Personal Harness",
                vec![
                    settings_row(
                        0,
                        "Native preview",
                        "Rust 2024 · GPUI 0.2 · protocol v2",
                        status_pill("Development", true, theme),
                        theme,
                    ),
                    settings_row(
                        1,
                        "Desktop runtime",
                        "The renderer, state reducer, protocol client, diff review, and terminal are native Rust.",
                        status_pill("GPUI", true, theme),
                        theme,
                    ),
                ],
                theme,
            )],
            theme,
        )
    }

    fn unported_settings(&self, title: &str, group: &str, note: &str) -> gpui::Div {
        settings_panel(
            title,
            vec![settings_group(
                group,
                vec![settings_empty_row(note.to_owned(), self.theme)],
                self.theme,
            )],
            self.theme,
        )
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
        let theme = self.theme;
        self.chat
            .update(cx, |chat, cx| chat.update_theme(theme, cx));
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

    fn persist_native_preferences(&mut self) {
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
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface_2.hsla()))
        .active(|style| style.opacity(0.72))
        .on_click(move |_event, _window, cx| action(cx))
        .child(settings_icon(icon, 13.0))
        .child(label)
        .into_any_element()
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

fn settings_icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}
