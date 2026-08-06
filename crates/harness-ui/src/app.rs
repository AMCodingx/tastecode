use crate::assets::{HarnessAssets, register_fonts};
use crate::chat::{ChatEvent, ChatView, SessionContext};
use crate::client_state::{ClientState, ClientUpdate};
use crate::sidebar::{SelectSession, sidebar};
use crate::theme::{TITLEBAR_HEIGHT, Theme};
use anyhow::Result;
use gpui::{
    App, Application, Bounds, Context, Entity, FontWeight, MouseButton, Render, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowOptions, div, point, prelude::*, px,
    size, svg,
};
use gpui_component::Root;
use std::rc::Rc;

const APP_WIDTH: f32 = 1180.0;
const APP_HEIGHT: f32 = 820.0;

pub fn run() -> Result<()> {
    Application::new()
        .with_assets(HarnessAssets)
        .run(|cx: &mut App| {
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

            cx.open_window(options, |window, cx| {
                window.set_window_title("Personal Harness");
                let app = cx.new(|cx| HarnessApp::new(window, cx));
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
    state: ClientState,
    chat: Entity<ChatView>,
    selected_thread_id: Option<String>,
    fixture: bool,
}

impl HarnessApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fixture = std::env::var_os("HARNESS_NATIVE_FIXTURE").is_some();
        let mut state = ClientState::new(fixture);
        let chat = cx.new(|cx| ChatView::new(Theme::dark(), window, cx));

        cx.subscribe(&chat, |this, _chat, event, _cx| match event {
            ChatEvent::NeedHistory {
                thread_id,
                after_seq,
            } => this.state.request_history(thread_id, *after_seq),
            ChatEvent::Submit {
                thread_id,
                text,
                steer,
            } => this.state.send_turn(thread_id, text.clone(), *steer),
            ChatEvent::Interrupt { thread_id } => this.state.interrupt(thread_id),
        })
        .detach();

        if !fixture && let Some(events) = state.connect() {
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

        Self {
            theme: Theme::dark(),
            sidebar_collapsed: false,
            state,
            chat,
            selected_thread_id: None,
            fixture,
        }
    }

    fn apply_client_update(&mut self, update: ClientUpdate, cx: &mut Context<Self>) {
        for chat_update in update.chat {
            self.chat.update(cx, |chat, cx| {
                chat.apply_update(chat_update, cx);
            });
        }
        if update.shell_changed {
            cx.notify();
        }
    }

    fn select_session(&mut self, thread_id: String, cx: &mut Context<Self>) {
        let context = self.state.projects.iter().find_map(|project| {
            project
                .sessions
                .iter()
                .find(|session| session.id == thread_id)
                .map(|session| SessionContext {
                    thread_id: session.id.clone(),
                    title: session.title.clone(),
                    project_name: project.name.clone(),
                    provider: session.provider,
                    branch: session.worktree_branch.clone(),
                })
        });
        let Some(context) = context else {
            return;
        };

        self.selected_thread_id = Some(thread_id.clone());
        self.chat.update(cx, |chat, cx| {
            chat.begin_session(context, cx);
        });
        self.state.select_thread(&thread_id);
        cx.notify();
    }

    fn select_handler(&self, cx: &Context<Self>) -> SelectSession {
        let view = cx.weak_entity();
        Rc::new(move |thread_id, cx| {
            let _ = view.update(cx, |this, cx| {
                this.select_session(thread_id, cx);
            });
        })
    }

    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let collapsed = self.sidebar_collapsed;

        div()
            .h(px(TITLEBAR_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .px(px(12.0))
            .bg(theme.titlebar.hsla())
            .border_b_1()
            .border_color(theme.line.hsla())
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
                    .size(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .text_color(theme.titlebar_symbol.hsla())
                    .opacity(0.78)
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_2.hsla()).opacity(1.0))
                    .active(|style| style.opacity(0.7))
                    .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.sidebar_collapsed = !this.sidebar_collapsed;
                        cx.notify();
                    }))
                    .child(icon("icons/panel-left.svg", 15.0)),
            )
            .child(
                div()
                    .ml(px(8.0))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.titlebar_symbol.hsla())
                    .opacity(if collapsed { 0.62 } else { 0.78 })
                    .child("Personal Harness"),
            )
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.selected_thread_id.is_some() {
            self.chat.clone().into_any_element()
        } else {
            self.stage().into_any_element()
        };
        let select_session = self.select_handler(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family("Geist")
            .text_size(px(13.5))
            .text_color(self.theme.text.hsla())
            .bg(self.theme.background.hsla())
            .child(self.titlebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .flex()
                    .when(!self.sidebar_collapsed, |body| {
                        body.child(sidebar(
                            self.theme,
                            &self.state.projects,
                            self.state.connection,
                            self.state.projects_loaded,
                            self.fixture,
                            self.selected_thread_id.as_deref(),
                            select_session,
                        ))
                    })
                    .child(content),
            )
    }
}

fn icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}
