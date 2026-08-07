use super::HarnessApp;
use crate::client_state::AuthTarget;
use crate::provider_icon::provider_icon;
use crate::theme::Theme;
use crate::zoom::px;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, FontWeight, Transformation, Window, div,
    percentage, prelude::*, svg,
};
use gpui_component::input::Input;
use gpui_component::scroll::ScrollableElement;
use harness_protocol::{Account, ProviderId};
use std::rc::Rc;
use std::time::Duration;

const ONBOARDING_WIDTH: f32 = 680.0;
const ONBOARDING_CARD_WIDTH: f32 = 306.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum OnboardingStep {
    #[default]
    Welcome,
    Provider,
    Agent,
    SignIn,
    Models,
    Done,
}

#[derive(Clone, Debug)]
pub(super) struct OnboardingState {
    step: OnboardingStep,
    provider: ProviderId,
    agent: Option<String>,
    agent_name: Option<String>,
    account: Option<Account>,
    api_key_expanded: bool,
    show_api_key: bool,
    transition: u64,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            step: OnboardingStep::Welcome,
            provider: ProviderId::Codex,
            agent: None,
            agent_name: None,
            account: None,
            api_key_expanded: false,
            show_api_key: false,
            transition: 1,
        }
    }
}

#[derive(Clone, Copy)]
struct ProviderCard {
    provider: ProviderId,
    name: &'static str,
    blurb: &'static str,
    plans: &'static [&'static str],
}

const PROVIDERS: [ProviderCard; 5] = [
    ProviderCard {
        provider: ProviderId::Codex,
        name: "Codex",
        blurb: "OpenAI’s agent, signed in with your ChatGPT account",
        plans: &["Plus", "Pro", "Business", "API key"],
    },
    ProviderCard {
        provider: ProviderId::ClaudeCode,
        name: "Claude Code",
        blurb: "Anthropic’s agent, signed in with your Claude account",
        plans: &["Pro", "Max", "API key"],
    },
    ProviderCard {
        provider: ProviderId::Acp,
        name: "Any ACP agent",
        blurb: "Gemini, Kimi, Qwen — anything speaking the open protocol",
        plans: &["Your own account", "API key"],
    },
    ProviderCard {
        provider: ProviderId::Cursor,
        name: "Cursor",
        blurb: "The Cursor agent, outside the editor",
        plans: &["Pro", "Pro+", "Ultra"],
    },
    ProviderCard {
        provider: ProviderId::OpenCode,
        name: "OpenCode",
        blurb: "Open source, any model you like",
        plans: &["Any provider", "Local models"],
    },
];

type OnboardingAction = Rc<dyn Fn(&mut Window, &mut App)>;

impl HarnessApp {
    pub(super) fn onboarding_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.onboarding.as_ref() else {
            return div().into_any_element();
        };
        let panel = match state.step {
            OnboardingStep::Welcome => self.onboarding_welcome(cx),
            OnboardingStep::Provider => self.onboarding_providers(cx),
            OnboardingStep::Agent => self.onboarding_agents(cx),
            OnboardingStep::SignIn => self.onboarding_sign_in(cx),
            OnboardingStep::Models => self.onboarding_models(cx),
            OnboardingStep::Done => self.onboarding_done(cx),
        };
        let dots = onboarding_steps(state)
            .into_iter()
            .enumerate()
            .map(|(index, step)| progress_dot(index, step == state.step, self.theme));
        let transition = state.transition;

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(18.0))
            .px(px(32.0))
            .pt(px(48.0))
            .pb(px(36.0))
            .overflow_y_scrollbar()
            .bg(self.theme.background.hsla())
            .child(
                div()
                    .w_full()
                    .max_w(px(ONBOARDING_WIDTH))
                    .child(panel)
                    .with_animation(
                        ("onboarding-step", transition),
                        Animation::new(self.theme.motion_duration(Duration::from_millis(300)))
                            .with_easing(crate::theme::web_ease_out),
                        |panel, delta| panel.opacity(delta).mt(px(10.0 * (1.0 - delta))),
                    ),
            )
            .child(div().flex().gap(px(5.0)).children(dots))
            .into_any_element()
    }

    fn onboarding_welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let weak = cx.weak_entity();
        let next: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.set_onboarding_step(OnboardingStep::Provider, cx);
            });
        });
        let facts = [
            (
                "icons/panels-top-left.svg",
                "Use your current accounts",
                "Your subscriptions stay with their providers.",
            ),
            (
                "icons/lock-keyhole.svg",
                "Your work stays local",
                "No Harness account, telemetry, or cloud relay.",
            ),
            (
                "icons/git-branch.svg",
                "Keep control",
                "Open source and built around your local projects.",
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (icon, title, note))| fact_card(index, icon, title, note, theme));

        onboarding_pane(
            div()
                .px(px(30.0))
                .pt(px(34.0))
                .pb(px(30.0))
                .child(onboarding_mark("icons/panels-top-left.svg", false, theme))
                .child(onboarding_title("Set up Personal Harness", theme))
                .child(onboarding_lede(
                    "Connect one coding agent now. You can add the rest later from Settings.",
                    theme,
                ))
                .child(div().flex().gap(px(8.0)).children(facts)),
            onboarding_footer(
                vec![onboarding_button(
                    "onboarding-continue",
                    "Continue",
                    Some("icons/arrow-right.svg"),
                    true,
                    true,
                    theme,
                    next,
                )],
                theme,
            ),
            theme,
        )
    }

    fn onboarding_providers(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.onboarding.as_ref().expect("onboarding is open");
        let theme = self.theme;
        let selected = state.provider;
        let cards = PROVIDERS.into_iter().enumerate().map(|(index, card)| {
            let weak = cx.weak_entity();
            let is_selected = card.provider == selected;
            div()
                .id(("onboarding-provider", index))
                .w(px(ONBOARDING_CARD_WIDTH))
                .min_h(px(118.0))
                .p(px(14.0))
                .rounded(px(11.0))
                .border_1()
                .border_color(if is_selected {
                    theme.line_strong.hsla()
                } else {
                    theme.line.hsla()
                })
                .bg(if is_selected {
                    theme.surface_2.hsla()
                } else {
                    theme.surface.hsla()
                })
                .shadow_lg()
                .cursor_pointer()
                .hover(move |style| style.border_color(theme.line_strong.hsla()).mt(px(-1.0)))
                .active(|style| style.mt(px(1.0)))
                .on_click(move |_event, _window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if let Some(state) = this.onboarding.as_mut() {
                            state.provider = card.provider;
                            if card.provider != ProviderId::Acp {
                                state.agent = None;
                                state.agent_name = None;
                            }
                            state.account = None;
                            state.transition = state.transition.wrapping_add(1);
                        }
                        this.state.auth_error = None;
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(9.0))
                                .child(provider_icon(card.provider, theme, 18.0))
                                .child(
                                    div()
                                        .text_size(px(14.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(card.name),
                                ),
                        )
                        .when(is_selected, |row| row.child(selection_check(index, theme))),
                )
                .child(
                    div()
                        .mt(px(5.0))
                        .text_size(px(11.5))
                        .line_height(px(17.0))
                        .text_color(theme.text_2.hsla())
                        .child(card.blurb),
                )
                .child(
                    div()
                        .mt(px(9.0))
                        .flex()
                        .flex_wrap()
                        .gap(px(5.0))
                        .children(card.plans.iter().map(|plan| chiplet(plan, theme))),
                )
                .into_any_element()
        });
        let back_weak = cx.weak_entity();
        let back: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = back_weak.update(cx, |this, cx| {
                this.set_onboarding_step(OnboardingStep::Welcome, cx);
            });
        });
        let next_weak = cx.weak_entity();
        let next: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = next_weak.update(cx, |this, cx| this.continue_onboarding_provider(cx));
        });

        onboarding_pane(
            onboarding_content(
                "Choose your first agent",
                "Choose one to finish setup. You can connect others later.",
                div().flex().flex_wrap().gap(px(7.0)).children(cards),
                theme,
            ),
            onboarding_footer(
                vec![
                    onboarding_button(
                        "onboarding-provider-back",
                        "Back",
                        None,
                        false,
                        true,
                        theme,
                        back,
                    ),
                    onboarding_button(
                        "onboarding-provider-next",
                        "Continue",
                        Some("icons/arrow-right.svg"),
                        true,
                        true,
                        theme,
                        next,
                    ),
                ],
                theme,
            ),
            theme,
        )
    }

    fn onboarding_agents(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.onboarding.as_ref().expect("onboarding is open");
        let theme = self.theme;
        let selected = state.agent.as_deref();
        let cards = self
            .state
            .acp_agents
            .iter()
            .enumerate()
            .map(|(index, agent)| {
                let weak = cx.weak_entity();
                let agent_id = agent.id.clone();
                let agent_name = agent.name.clone();
                let installed = agent.installed;
                let is_selected = selected == Some(agent.id.as_str());
                let note = if installed {
                    if agent.verified {
                        "Tested against this build".into()
                    } else {
                        "Supported, but not tested by us yet".into()
                    }
                } else {
                    agent
                        .setup
                        .install_command
                        .clone()
                        .unwrap_or_else(|| "Install it, then come back".into())
                };
                div()
                    .id(("onboarding-agent", index))
                    .w(px(ONBOARDING_CARD_WIDTH))
                    .min_h(px(88.0))
                    .p(px(14.0))
                    .rounded(px(11.0))
                    .border_1()
                    .border_color(if is_selected {
                        theme.line_strong.hsla()
                    } else {
                        theme.line.hsla()
                    })
                    .bg(if is_selected {
                        theme.surface_2.hsla()
                    } else {
                        theme.surface.hsla()
                    })
                    .opacity(if installed { 1.0 } else { 0.4 })
                    .when(installed, |card| {
                        card.cursor_pointer()
                            .hover(move |style| {
                                style.border_color(theme.line_strong.hsla()).mt(px(-1.0))
                            })
                            .active(|style| style.mt(px(1.0)))
                            .on_click(move |_event, _window, cx| {
                                let agent_id = agent_id.clone();
                                let agent_name = agent_name.clone();
                                let _ = weak.update(cx, |this, cx| {
                                    if let Some(state) = this.onboarding.as_mut() {
                                        state.agent = Some(agent_id);
                                        state.agent_name = Some(agent_name);
                                        state.transition = state.transition.wrapping_add(1);
                                    }
                                    cx.notify();
                                });
                            })
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(agent.name.clone()),
                            )
                            .when(!installed, |row| {
                                row.child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_2.hsla())
                                        .child("Not installed"),
                                )
                            })
                            .when(is_selected, |row| row.child(selection_check(index, theme))),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .text_size(px(11.5))
                            .line_height(px(17.0))
                            .text_color(theme.text_2.hsla())
                            .child(note),
                    )
                    .into_any_element()
            });
        let installed = self.state.acp_agents.iter().any(|agent| agent.installed);
        let loaded = self.state.model_catalog_loaded || !self.state.acp_agents.is_empty();
        let back_weak = cx.weak_entity();
        let back: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = back_weak.update(cx, |this, cx| {
                this.set_onboarding_step(OnboardingStep::Provider, cx);
            });
        });
        let next_weak = cx.weak_entity();
        let next: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = next_weak.update(cx, |this, cx| {
                this.set_onboarding_step(OnboardingStep::Models, cx);
            });
        });
        let can_continue = state.agent.is_some();
        let list = div()
            .flex()
            .flex_wrap()
            .gap(px(7.0))
            .children(cards)
            .when(!loaded, |list| {
                list.child(onboarding_status(
                    "Looking for agents on this machine…",
                    theme,
                ))
            })
            .when(loaded && !installed, |list| {
                list.child(onboarding_error(
                    "None of these are installed yet. Install one and reopen this step.",
                    theme,
                ))
            });

        onboarding_pane(
            onboarding_content(
                "Choose an ACP agent",
                "Personal Harness found these compatible agents on this computer.",
                list,
                theme,
            ),
            onboarding_footer(
                vec![
                    onboarding_button(
                        "onboarding-agent-back",
                        "Back",
                        None,
                        false,
                        true,
                        theme,
                        back,
                    ),
                    onboarding_button(
                        "onboarding-agent-next",
                        "Continue",
                        Some("icons/arrow-right.svg"),
                        true,
                        can_continue,
                        theme,
                        next,
                    ),
                ],
                theme,
            ),
            theme,
        )
    }

    fn onboarding_sign_in(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.onboarding.as_ref().expect("onboarding is open");
        let theme = self.theme;
        let card = provider_card(state.provider);
        let target = onboarding_target(state);
        let waiting = self.state.auth_logins.contains_key(&target);
        let busy = self.state.auth_busy.as_ref() == Some(&target);
        let start_weak = cx.weak_entity();
        let start: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = start_weak.update(cx, |this, cx| this.start_onboarding_auth(cx));
        });
        let cancel_weak = cx.weak_entity();
        let cancel: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = cancel_weak.update(cx, |this, cx| this.cancel_onboarding_auth(cx));
        });
        let back_weak = cx.weak_entity();
        let back: OnboardingAction = Rc::new(move |window, cx| {
            let _ = back_weak.update(cx, |this, cx| {
                this.clear_onboarding_api_key(window, cx);
                this.set_onboarding_step(OnboardingStep::Provider, cx);
            });
        });
        let key_weak = cx.weak_entity();
        let use_key: OnboardingAction = Rc::new(move |window, cx| {
            let _ = key_weak.update(cx, |this, cx| {
                this.submit_onboarding_api_key(window, cx);
            });
        });
        let reveal_weak = cx.weak_entity();
        let reveal: OnboardingAction = Rc::new(move |window, cx| {
            let _ = reveal_weak.update(cx, |this, cx| {
                this.toggle_onboarding_api_key_visibility(window, cx);
            });
        });
        let expand_weak = cx.weak_entity();
        let expanded = state.api_key_expanded;
        let expand: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = expand_weak.update(cx, |this, cx| {
                if let Some(state) = this.onboarding.as_mut() {
                    state.api_key_expanded = !state.api_key_expanded;
                    state.transition = state.transition.wrapping_add(1);
                }
                this.state.auth_error = None;
                cx.notify();
            });
        });
        let show_key = state.show_api_key;
        let error = self.state.auth_error.clone();
        let key_empty = self
            .onboarding_api_key
            .read(cx)
            .unmask_value()
            .trim()
            .is_empty();

        let browser = if waiting {
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(11.0))
                .border_1()
                .border_color(theme.line.hsla())
                .bg(theme.surface_2.hsla())
                .child(onboarding_spinner(theme))
                .child(
                    div()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Waiting for your browser…"),
                        )
                        .child(
                            div()
                                .mt(px(2.0))
                                .text_size(px(11.5))
                                .text_color(theme.text_3.hsla())
                                .child(
                                    "Finish signing in there and this window will continue on its own.",
                                ),
                        ),
                )
                .into_any_element()
        } else {
            onboarding_button(
                "onboarding-browser-sign-in",
                if busy {
                    "Starting…"
                } else {
                    "Sign in with Codex"
                },
                None,
                true,
                !busy,
                theme,
                start,
            )
        };

        let key_section = div()
            .mt(px(18.0))
            .pt(px(14.0))
            .border_t_1()
            .border_color(theme.line.hsla())
            .child(onboarding_text_action(
                "onboarding-expand-api-key",
                if expanded {
                    "Hide API key option"
                } else {
                    "Use an API key instead"
                },
                theme,
                expand,
            ))
            .when(expanded, |section| {
                section
                    .child(
                        div()
                            .mt(px(10.0))
                            .mb(px(10.0))
                            .text_size(px(11.5))
                            .line_height(px(18.0))
                            .text_color(theme.text_3.hsla())
                            .child(
                                "Billed per token by the vendor. Stored in your operating system’s credential store, never in a file we write.",
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                Input::new(&self.onboarding_api_key)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .disabled(busy)
                                    .h(px(34.0))
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .px(px(10.0))
                                    .rounded(px(8.0))
                                    .border_1()
                                    .border_color(theme.line_strong.hsla())
                                    .bg(theme.surface.hsla())
                                    .font_family("Geist Mono")
                                    .text_size(px(11.5))
                                    .text_color(theme.text.hsla()),
                            )
                            .child(onboarding_button(
                                "onboarding-reveal-key",
                                if show_key { "Hide" } else { "Show" },
                                None,
                                false,
                                !busy,
                                theme,
                                reveal,
                            ))
                            .child(onboarding_button(
                                "onboarding-use-key",
                                if busy { "Saving…" } else { "Use key" },
                                None,
                                true,
                                !busy && !key_empty,
                                theme,
                                use_key,
                            )),
                    )
            });

        let content = div()
            .px(px(30.0))
            .pt(px(30.0))
            .pb(px(30.0))
            .child(onboarding_title(
                &format!("Sign in to {}", card.name),
                theme,
            ))
            .child(onboarding_lede(
                &format!(
                    "This opens {}’s own sign-in page in your browser. Your password and token never pass through Personal Harness.",
                    card.name
                ),
                theme,
            ))
            .child(browser)
            .when_some(error, |content, error| {
                content.child(onboarding_error(&error, theme))
            })
            .child(key_section);
        let mut footer = vec![onboarding_button(
            "onboarding-sign-in-back",
            "Back",
            None,
            false,
            true,
            theme,
            back,
        )];
        if waiting {
            footer.push(onboarding_button(
                "onboarding-sign-in-cancel",
                "Cancel",
                None,
                false,
                true,
                theme,
                cancel,
            ));
        }
        onboarding_pane(content, onboarding_footer(footer, theme), theme)
    }

    fn onboarding_models(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.onboarding.as_ref().expect("onboarding is open");
        let theme = self.theme;
        let provider = state.provider;
        let agent = state.agent.as_deref();
        let choices = self
            .state
            .model_catalog
            .iter()
            .filter(|choice| {
                choice.provider == provider
                    && (provider != ProviderId::Acp || choice.agent_id.as_deref() == agent)
            })
            .enumerate()
            .map(|(index, choice)| {
                let weak = cx.weak_entity();
                let key = choice.key.clone();
                let visible = !self.preferences.hidden_models.contains(&choice.key);
                div()
                    .id(("onboarding-model", index))
                    .w(px(ONBOARDING_CARD_WIDTH))
                    .min_h(px(68.0))
                    .p(px(14.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .rounded(px(11.0))
                    .border_1()
                    .border_color(if visible {
                        theme.line_strong.hsla()
                    } else {
                        theme.line.hsla()
                    })
                    .bg(if visible {
                        theme.surface_2.hsla()
                    } else {
                        theme.surface.hsla()
                    })
                    .cursor_pointer()
                    .hover(move |style| style.border_color(theme.line_strong.hsla()).mt(px(-1.0)))
                    .active(|style| style.mt(px(1.0)))
                    .on_click(move |_event, _window, cx| {
                        let key = key.clone();
                        let _ = weak.update(cx, |this, cx| {
                            this.toggle_onboarding_model(&key, cx);
                        });
                    })
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .child(provider_icon(choice.provider, theme, 18.0))
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(13.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(choice.model.display_name.clone()),
                                    )
                                    .child(
                                        div()
                                            .mt(px(1.0))
                                            .truncate()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_3.hsla())
                                            .child(choice.source_name.clone()),
                                    ),
                            ),
                    )
                    .when(visible, |card| card.child(selection_check(index, theme)))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let empty = choices.is_empty();
        let back_step = match provider {
            ProviderId::Codex => OnboardingStep::SignIn,
            ProviderId::Acp => OnboardingStep::Agent,
            _ => OnboardingStep::Provider,
        };
        let back_weak = cx.weak_entity();
        let back: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = back_weak.update(cx, |this, cx| {
                this.set_onboarding_step(back_step, cx);
            });
        });
        let next_weak = cx.weak_entity();
        let next: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = next_weak.update(cx, |this, cx| {
                this.set_onboarding_step(OnboardingStep::Done, cx);
            });
        });
        let list = div()
            .max_h(px(390.0))
            .overflow_y_scrollbar()
            .pr(px(3.0))
            .flex()
            .flex_wrap()
            .gap(px(7.0))
            .children(choices)
            .when(empty && !self.state.model_catalog_loaded, |list| {
                list.child(onboarding_status("Loading models…", theme))
            })
            .when(empty && self.state.model_catalog_loaded, |list| {
                list.child(onboarding_status(
                    "No models are available for this agent yet. You can refresh them later in Settings.",
                    theme,
                ))
            });

        onboarding_pane(
            onboarding_content(
                "Choose your model list",
                "Keep the composer focused. You can change this any time in Settings.",
                list,
                theme,
            ),
            onboarding_footer(
                vec![
                    onboarding_button(
                        "onboarding-models-back",
                        "Back",
                        None,
                        false,
                        true,
                        theme,
                        back,
                    ),
                    onboarding_button(
                        "onboarding-models-next",
                        "Continue",
                        Some("icons/arrow-right.svg"),
                        true,
                        true,
                        theme,
                        next,
                    ),
                ],
                theme,
            ),
            theme,
        )
    }

    fn onboarding_done(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.onboarding.as_ref().expect("onboarding is open");
        let theme = self.theme;
        let card = provider_card(state.provider);
        let account = state
            .account
            .as_ref()
            .or_else(|| self.state.accounts.get(&onboarding_target(state)));
        let weak = cx.weak_entity();
        let finish: OnboardingAction = Rc::new(move |_window, cx| {
            let _ = weak.update(cx, |this, cx| this.finish_onboarding(cx));
        });
        let identity = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.0))
            .p(px(13.0))
            .mb(px(20.0))
            .rounded(px(11.0))
            .border_1()
            .border_color(theme.line.hsla())
            .bg(theme.surface_2.hsla())
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(state.agent_name.as_deref().unwrap_or(card.name).to_owned()),
            )
            .when_some(
                account.and_then(|account| account.plan.clone()),
                |row, plan| row.child(chiplet(&plan, theme)),
            )
            .when_some(
                account.and_then(|account| account.email.clone()),
                |row, email| {
                    row.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.text_3.hsla())
                            .child(email),
                    )
                },
            );

        onboarding_pane(
            div()
                .px(px(30.0))
                .pt(px(30.0))
                .pb(px(30.0))
                .child(onboarding_mark("icons/check.svg", true, theme))
                .child(onboarding_title("Ready to start", theme))
                .child(identity)
                .child(onboarding_lede(
                    "Open a project folder and start a session. Agents, models, and permissions stay available in Settings.",
                    theme,
                )),
            onboarding_footer(
                vec![onboarding_button(
                    "onboarding-finish",
                    "Open Personal Harness",
                    None,
                    true,
                    true,
                    theme,
                    finish,
                )],
                theme,
            ),
            theme,
        )
    }

    fn set_onboarding_step(&mut self, step: OnboardingStep, cx: &mut Context<Self>) {
        if let Some(state) = self.onboarding.as_mut() {
            state.step = step;
            state.transition = state.transition.wrapping_add(1);
        }
        self.state.auth_error = None;
        cx.notify();
    }

    fn continue_onboarding_provider(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding.as_mut() else {
            return;
        };
        let next = match state.provider {
            ProviderId::Acp => {
                if state.agent.is_none()
                    && let Some(agent) = self.state.acp_agents.iter().find(|agent| agent.installed)
                {
                    state.agent = Some(agent.id.clone());
                    state.agent_name = Some(agent.name.clone());
                }
                OnboardingStep::Agent
            }
            ProviderId::Codex => {
                let target = AuthTarget::provider(ProviderId::Codex);
                if let Some(account) = self
                    .state
                    .accounts
                    .get(&target)
                    .filter(|account| account.signed_in)
                    .cloned()
                {
                    state.account = Some(account);
                    OnboardingStep::Models
                } else {
                    OnboardingStep::SignIn
                }
            }
            _ => OnboardingStep::Models,
        };
        state.step = next;
        state.transition = state.transition.wrapping_add(1);
        self.state.auth_error = None;
        cx.notify();
    }

    fn start_onboarding_auth(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding.as_ref() else {
            return;
        };
        let update = self.state.start_auth(onboarding_target(state));
        self.apply_client_update(update, cx);
    }

    fn cancel_onboarding_auth(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding.as_ref() else {
            return;
        };
        let update = self.state.cancel_auth(onboarding_target(state));
        self.apply_client_update(update, cx);
    }

    fn submit_onboarding_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding.as_ref() else {
            return;
        };
        let target = onboarding_target(state);
        let api_key = self.onboarding_api_key.read(cx).unmask_value().to_string();
        self.clear_onboarding_api_key(window, cx);
        let update = self.state.use_api_key(target, api_key);
        self.apply_client_update(update, cx);
    }

    fn clear_onboarding_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.onboarding_api_key.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_masked(true, window, cx);
        });
        if let Some(state) = self.onboarding.as_mut() {
            state.show_api_key = false;
        }
    }

    fn toggle_onboarding_api_key_visibility(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.onboarding.as_mut() else {
            return;
        };
        state.show_api_key = !state.show_api_key;
        let masked = !state.show_api_key;
        self.onboarding_api_key
            .update(cx, |input, cx| input.set_masked(masked, window, cx));
        cx.notify();
    }

    fn toggle_onboarding_model(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.preferences.hidden_models.contains(key) {
            self.preferences.hidden_models.remove(key);
        } else {
            self.preferences.hidden_models.insert(key.to_owned());
        }
        self.persist_native_preferences();
        self.sync_model_selection();
        self.sync_composer_settings(cx);
        cx.notify();
    }

    fn finish_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding.as_ref() else {
            return;
        };
        let provider = state.provider;
        let agent = state.agent.clone();
        let agent_name = state.agent_name.clone();
        self.preferences.setup_provider = Some(provider);
        self.preferences.setup_agent = agent.clone();
        self.preferences.setup_agent_name = agent_name;
        self.persist_native_preferences();

        let preferred = self
            .state
            .model_catalog
            .iter()
            .filter(|choice| {
                choice.provider == provider
                    && (provider != ProviderId::Acp || choice.agent_id == agent)
                    && !self.preferences.hidden_models.contains(&choice.key)
            })
            .find(|choice| choice.model.is_default)
            .or_else(|| {
                self.state.model_catalog.iter().find(|choice| {
                    choice.provider == provider
                        && (provider != ProviderId::Acp || choice.agent_id == agent)
                        && !self.preferences.hidden_models.contains(&choice.key)
                })
            })
            .map(|choice| choice.key.clone());
        self.onboarding = None;
        if let Some(key) = preferred {
            self.select_model(&key, cx);
        } else {
            self.sync_model_selection();
            self.sync_composer_settings(cx);
        }
        cx.notify();
    }

    pub(super) fn sync_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.onboarding.as_ref() else {
            return;
        };
        if state.step != OnboardingStep::SignIn {
            return;
        }
        let target = onboarding_target(state);
        let Some(account) = self
            .state
            .accounts
            .get(&target)
            .filter(|account| account.signed_in)
            .cloned()
        else {
            return;
        };
        if let Some(state) = self.onboarding.as_mut() {
            state.account = Some(account);
            state.step = OnboardingStep::Models;
            state.transition = state.transition.wrapping_add(1);
        }
        cx.notify();
    }
}

fn onboarding_target(state: &OnboardingState) -> AuthTarget {
    match &state.agent {
        Some(agent) if state.provider == ProviderId::Acp => {
            AuthTarget::agent(state.provider, agent.clone())
        }
        _ => AuthTarget::provider(state.provider),
    }
}

fn onboarding_steps(state: &OnboardingState) -> Vec<OnboardingStep> {
    match state.provider {
        ProviderId::Codex => vec![
            OnboardingStep::Welcome,
            OnboardingStep::Provider,
            OnboardingStep::SignIn,
            OnboardingStep::Models,
            OnboardingStep::Done,
        ],
        ProviderId::Acp => vec![
            OnboardingStep::Welcome,
            OnboardingStep::Provider,
            OnboardingStep::Agent,
            OnboardingStep::Models,
            OnboardingStep::Done,
        ],
        _ => vec![
            OnboardingStep::Welcome,
            OnboardingStep::Provider,
            OnboardingStep::Models,
            OnboardingStep::Done,
        ],
    }
}

fn provider_card(provider: ProviderId) -> ProviderCard {
    PROVIDERS
        .into_iter()
        .find(|card| card.provider == provider)
        .unwrap_or(PROVIDERS[0])
}

fn onboarding_pane(content: gpui::Div, footer: AnyElement, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .overflow_hidden()
        .rounded(px(17.0))
        .border_1()
        .border_color(theme.line_strong.hsla())
        .bg(theme.surface.hsla())
        .shadow_lg()
        .child(content)
        .child(footer)
        .into_any_element()
}

fn onboarding_content(title: &str, lede: &str, body: impl IntoElement, theme: Theme) -> gpui::Div {
    div()
        .px(px(30.0))
        .pt(px(30.0))
        .pb(px(30.0))
        .child(onboarding_title(title, theme))
        .child(onboarding_lede(lede, theme))
        .child(body)
}

fn onboarding_title(title: &str, theme: Theme) -> gpui::Div {
    div()
        .mb(px(12.0))
        .text_size(px(30.0))
        .line_height(px(32.4))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text.hsla())
        .child(title.to_owned())
}

fn onboarding_lede(text: &str, theme: Theme) -> gpui::Div {
    div()
        .max_w(px(540.0))
        .mb(px(26.0))
        .text_size(px(13.5))
        .line_height(px(21.0))
        .text_color(theme.text_2.hsla())
        .child(text.to_owned())
}

fn onboarding_footer(children: Vec<AnyElement>, theme: Theme) -> AnyElement {
    div()
        .min_h(px(63.0))
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.0))
        .px(px(18.0))
        .py(px(14.0))
        .border_t_1()
        .border_color(theme.line.hsla())
        .bg(theme.surface_2.hsla().opacity(0.55))
        .children(children)
        .into_any_element()
}

fn onboarding_button(
    id: &'static str,
    label: &'static str,
    icon: Option<&'static str>,
    primary: bool,
    enabled: bool,
    theme: Theme,
    action: OnboardingAction,
) -> AnyElement {
    div()
        .id(id)
        .min_h(px(34.0))
        .px(px(if primary { 14.0 } else { 10.0 }))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(if primary {
            theme.text.hsla().opacity(0.7)
        } else {
            theme.line_strong.hsla()
        })
        .bg(if primary {
            theme.text.hsla()
        } else {
            theme.surface.hsla()
        })
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if primary {
            theme.background.hsla()
        } else {
            theme.text_2.hsla()
        })
        .opacity(if enabled { 1.0 } else { 0.42 })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.mt(px(-1.0)))
                .active(|style| style.mt(px(1.0)).opacity(0.86))
                .on_click(move |_event, window, cx| action(window, cx))
        })
        .child(label)
        .when_some(icon, |button, icon| {
            button.child(svg().path(icon).size(px(14.0)))
        })
        .into_any_element()
}

fn onboarding_text_action(
    id: &'static str,
    label: &'static str,
    theme: Theme,
    action: OnboardingAction,
) -> AnyElement {
    div()
        .id(id)
        .w_auto()
        .flex()
        .text_size(px(11.5))
        .text_color(theme.text_2.hsla())
        .cursor_pointer()
        .hover(move |style| style.text_color(theme.text.hsla()))
        .on_click(move |_event, window, cx| action(window, cx))
        .child(label)
        .into_any_element()
}

fn onboarding_mark(icon: &'static str, success: bool, theme: Theme) -> AnyElement {
    div()
        .size(px(42.0))
        .mb(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(11.0))
        .border_1()
        .border_color(theme.line_strong.hsla())
        .bg(theme.surface_2.hsla())
        .text_color(if success {
            theme.success.hsla()
        } else {
            theme.text.hsla()
        })
        .child(svg().path(icon).size(px(21.0)))
        .into_any_element()
}

fn fact_card(
    index: usize,
    icon: &'static str,
    title: &'static str,
    note: &'static str,
    theme: Theme,
) -> AnyElement {
    div()
        .id(("onboarding-fact", index))
        .w(px(201.0))
        .min_h(px(92.0))
        .p(px(13.0))
        .flex()
        .items_start()
        .gap(px(10.0))
        .rounded(px(11.0))
        .border_1()
        .border_color(theme.line.hsla())
        .bg(theme.surface_2.hsla())
        .child(
            div()
                .size(px(26.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.0))
                .bg(theme.surface.hsla())
                .text_color(theme.text_2.hsla())
                .child(svg().path(icon).size(px(15.0))),
        )
        .child(
            div()
                .min_w(px(0.0))
                .text_size(px(11.0))
                .line_height(px(16.0))
                .text_color(theme.text_2.hsla())
                .child(
                    div()
                        .mb(px(3.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text.hsla())
                        .child(title),
                )
                .child(note),
        )
        .into_any_element()
}

fn chiplet(label: &str, theme: Theme) -> AnyElement {
    div()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .bg(theme.surface_3.hsla())
        .text_size(px(9.5))
        .text_color(theme.text_2.hsla())
        .child(label.to_owned())
        .into_any_element()
}

fn selection_check(index: usize, theme: Theme) -> AnyElement {
    div()
        .id(("onboarding-check", index))
        .size(px(22.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme.text.hsla())
        .text_color(theme.background.hsla())
        .child(svg().path("icons/check.svg").size(px(12.0)))
        .with_animation(
            ("onboarding-check-in", index),
            Animation::new(theme.motion_duration(Duration::from_millis(240)))
                .with_easing(crate::theme::web_ease_out),
            |check, delta| check.opacity(delta),
        )
        .into_any_element()
}

fn progress_dot(index: usize, selected: bool, theme: Theme) -> AnyElement {
    div()
        .id(("onboarding-progress", index))
        .h(px(3.0))
        .w(px(if selected { 24.0 } else { 17.0 }))
        .rounded_full()
        .bg(if selected {
            theme.text.hsla()
        } else {
            theme.surface_3.hsla()
        })
        .into_any_element()
}

fn onboarding_spinner(theme: Theme) -> AnyElement {
    svg()
        .path("icons/loader-circle.svg")
        .size(px(16.0))
        .mt(px(3.0))
        .text_color(theme.text_2.hsla())
        .with_animation(
            "onboarding-spinner",
            theme.repeating_animation(Duration::from_millis(900)),
            |spinner, delta| spinner.with_transformation(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
}

fn onboarding_status(message: &str, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .py(px(14.0))
        .text_size(px(11.5))
        .line_height(px(18.0))
        .text_color(theme.text_3.hsla())
        .child(message.to_owned())
        .into_any_element()
}

fn onboarding_error(message: &str, theme: Theme) -> AnyElement {
    div()
        .w_full()
        .mt(px(12.0))
        .text_size(px(11.5))
        .line_height(px(18.0))
        .text_color(theme.error.hsla())
        .child(message.to_owned())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_progress_matches_each_provider_path() {
        let mut state = OnboardingState::default();
        assert_eq!(onboarding_steps(&state).len(), 5);
        state.provider = ProviderId::Acp;
        assert!(onboarding_steps(&state).contains(&OnboardingStep::Agent));
        state.provider = ProviderId::Cursor;
        assert_eq!(onboarding_steps(&state).len(), 4);
        assert!(!onboarding_steps(&state).contains(&OnboardingStep::SignIn));
    }

    #[test]
    fn auth_target_preserves_the_selected_acp_agent() {
        let state = OnboardingState {
            provider: ProviderId::Acp,
            agent: Some("gemini".into()),
            agent_name: Some("Gemini CLI".into()),
            ..OnboardingState::default()
        };
        assert_eq!(
            onboarding_target(&state),
            AuthTarget::agent(ProviderId::Acp, "gemini".into())
        );
    }
}
