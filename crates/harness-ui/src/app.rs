use crate::assets::{HarnessAssets, register_fonts};
use crate::client_state::ClientState;
use crate::sidebar::sidebar;
use crate::theme::{TITLEBAR_HEIGHT, Theme};
use anyhow::Result;
use gpui::{
    App, Application, Bounds, Context, FontWeight, MouseButton, Render, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, div, point, prelude::*, px, size, svg,
};

const APP_WIDTH: f32 = 1180.0;
const APP_HEIGHT: f32 = 820.0;

pub fn run() -> Result<()> {
    Application::new()
        .with_assets(HarnessAssets)
        .run(|cx: &mut App| {
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
                cx.new(HarnessApp::new)
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
    fixture: bool,
}

impl HarnessApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let fixture = std::env::var_os("HARNESS_NATIVE_FIXTURE").is_some();
        let mut state = ClientState::new(fixture);

        if !fixture && let Some(events) = state.connect() {
            cx.spawn(async move |view, cx| {
                while let Ok(event) = events.recv().await {
                    let result = view.update(cx, |this, cx| {
                        if this.state.handle_event(event) {
                            cx.notify();
                        }
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
            fixture,
        }
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
                        ))
                    })
                    .child(self.stage()),
            )
    }
}

fn icon(path: &'static str, size: f32) -> impl IntoElement {
    svg().path(path).size(px(size))
}
