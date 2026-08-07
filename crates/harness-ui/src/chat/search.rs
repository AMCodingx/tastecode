use super::ChatView;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, Entity, Focusable, FontWeight, KeyDownEvent,
    SharedString, Window, div, ease_out_quint, prelude::*, px, svg,
};
use gpui_component::input::{Input, InputEvent, InputState};
use harness_protocol::Item;

pub(super) struct ThreadSearchState {
    pub(super) input: Entity<InputState>,
    open: bool,
    reset_input: bool,
    focus_pending: bool,
    hits: Vec<usize>,
    cursor: usize,
    jumped: bool,
    open_transition: u64,
}

impl ThreadSearchState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<ChatView>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in thread"));
        cx.subscribe(&input, |this, _input, event, cx| match event {
            InputEvent::Change => this.thread_search_query_changed(cx),
            InputEvent::PressEnter { .. } => this.advance_thread_search(false, cx),
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        })
        .detach();
        Self {
            input,
            open: false,
            reset_input: false,
            focus_pending: false,
            hits: Vec::new(),
            cursor: 0,
            jumped: false,
            open_transition: 0,
        }
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.focus_pending = false;
        self.hits.clear();
        self.cursor = 0;
        self.jumped = false;
    }
}

impl ChatView {
    pub(super) fn handle_composer_paste_key(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if !event.is_held
            && self.composer.read(cx).focus_handle(cx).is_focused(window)
            && (modifiers.platform || modifiers.control)
            && !modifiers.shift
            && !modifiers.alt
            && event.keystroke.key.eq_ignore_ascii_case("v")
            && self.attach_pasted_image(cx)
        {
            cx.stop_propagation();
        }
    }

    fn open_thread_search(&mut self, cx: &mut Context<Self>) {
        if self.thread_search.open {
            self.thread_search.focus_pending = true;
        } else {
            self.thread_search.open = true;
            self.thread_search.reset_input = true;
            self.thread_search.focus_pending = true;
            self.thread_search.open_transition = self.thread_search.open_transition.wrapping_add(1);
            self.thread_search.hits.clear();
            self.thread_search.cursor = 0;
            self.thread_search.jumped = false;
        }
        cx.notify();
    }

    fn close_thread_search(&mut self, cx: &mut Context<Self>) {
        self.thread_search.close();
        cx.notify();
    }

    pub(super) fn prepare_thread_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.thread_search.reset_input {
            self.thread_search.input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
            self.thread_search.reset_input = false;
        }
        if self.thread_search.focus_pending {
            self.thread_search
                .input
                .update(cx, |input, cx| input.focus(window, cx));
            self.thread_search.focus_pending = false;
        }
    }

    fn thread_search_query_changed(&mut self, cx: &mut Context<Self>) {
        self.thread_search.cursor = 0;
        self.thread_search.jumped = false;
        self.refresh_thread_search_hits(cx);
        cx.notify();
    }

    pub(super) fn refresh_thread_search_hits(&mut self, cx: &gpui::App) {
        if !self.thread_search.open {
            return;
        }
        let query = self
            .thread_search
            .input
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        if query.is_empty() {
            self.thread_search.hits.clear();
            self.thread_search.cursor = 0;
            self.thread_search.jumped = false;
            return;
        }
        self.thread_search.hits = (0..self.state.timeline_len())
            .filter(|row| {
                self.state
                    .item_at_row(*row)
                    .is_some_and(|item| item_matches(item, &query))
            })
            .collect();
        if self.thread_search.hits.is_empty() {
            self.thread_search.cursor = 0;
            self.thread_search.jumped = false;
        } else {
            self.thread_search.cursor = self
                .thread_search
                .cursor
                .min(self.thread_search.hits.len() - 1);
        }
    }

    pub(super) fn refresh_thread_search_rows(&mut self, rows: &[usize], cx: &gpui::App) {
        if !self.thread_search.open || rows.is_empty() {
            return;
        }
        let query = self
            .thread_search
            .input
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        if query.is_empty() {
            return;
        }
        for row in rows {
            let matches = self
                .state
                .item_at_row(*row)
                .is_some_and(|item| item_matches(item, &query));
            match self.thread_search.hits.binary_search(row) {
                Ok(index) if !matches => {
                    self.thread_search.hits.remove(index);
                    if index < self.thread_search.cursor {
                        self.thread_search.cursor -= 1;
                    }
                }
                Err(index) if matches => {
                    self.thread_search.hits.insert(index, *row);
                }
                _ => {}
            }
        }
        if self.thread_search.hits.is_empty() {
            self.thread_search.cursor = 0;
            self.thread_search.jumped = false;
        } else {
            self.thread_search.cursor = self
                .thread_search
                .cursor
                .min(self.thread_search.hits.len() - 1);
        }
    }

    fn advance_thread_search(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let Some((cursor, row)) = next_hit(
            self.thread_search.hits.len(),
            self.thread_search.cursor,
            self.thread_search.jumped,
            backwards,
        ) else {
            return;
        };
        self.thread_search.cursor = cursor;
        self.thread_search.jumped = true;
        let row = self.thread_search.hits[row];
        self.list_state.scroll_to_reveal_item(row);
        cx.notify();
    }

    pub(super) fn handle_thread_navigation_key(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if event.keystroke.key.eq_ignore_ascii_case("escape") && self.thread_search.open {
            cx.stop_propagation();
            self.close_thread_search(cx);
            return;
        }
        if self.thread_search.open
            && event.keystroke.key.eq_ignore_ascii_case("enter")
            && event.keystroke.modifiers.shift
        {
            cx.stop_propagation();
            self.advance_thread_search(true, cx);
            return;
        }
        if self.text_input_focused_for_search(window, cx) {
            return;
        }
        if (modifiers.platform || modifiers.control)
            && !modifiers.shift
            && !modifiers.alt
            && event.keystroke.key.eq_ignore_ascii_case("f")
        {
            cx.stop_propagation();
            self.open_thread_search(cx);
            return;
        }
        if modifiers.alt
            && !modifiers.shift
            && !modifiers.platform
            && !modifiers.control
            && (event.keystroke.key.eq_ignore_ascii_case("up")
                || event.keystroke.key.eq_ignore_ascii_case("down"))
        {
            let current = self.list_state.logical_scroll_top().item_ix;
            let turns = turn_starts(
                (0..self.state.timeline_len()).filter_map(|row| self.state.item_at_row(row)),
            );
            let backwards = event.keystroke.key.eq_ignore_ascii_case("up");
            if let Some(row) = neighbour_turn(&turns, current, backwards) {
                cx.stop_propagation();
                self.list_state.scroll_to_reveal_item(row);
                cx.notify();
            }
        }
    }

    fn text_input_focused_for_search(&self, window: &Window, cx: &gpui::App) -> bool {
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

    pub(super) fn thread_search_overlay(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.thread_search.open {
            return None;
        }
        let theme = self.theme;
        let query_empty = self.thread_search.input.read(cx).value().trim().is_empty();
        let count = if query_empty {
            SharedString::from("")
        } else if self.thread_search.hits.is_empty() {
            SharedString::from("None")
        } else {
            SharedString::from(format!(
                "{}/{}",
                self.thread_search.cursor + 1,
                self.thread_search.hits.len()
            ))
        };
        let previous = search_button(
            "thread-find-previous",
            "icons/chevron-up.svg",
            theme,
            cx.listener(|this, _event, _window, cx| {
                this.advance_thread_search(true, cx);
            }),
        );
        let next = search_button(
            "thread-find-next",
            "icons/chevron-down.svg",
            theme,
            cx.listener(|this, _event, _window, cx| {
                this.advance_thread_search(false, cx);
            }),
        );
        let close = search_button(
            "thread-find-close",
            "icons/x.svg",
            theme,
            cx.listener(|this, _event, _window, cx| {
                this.close_thread_search(cx);
            }),
        );
        Some(
            div()
                .id("thread-find")
                .occlude()
                .absolute()
                .top(px(10.0))
                .right(px(22.0))
                .h(px(38.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .pl(px(10.0))
                .pr(px(6.0))
                .py(px(4.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.line_strong.hsla())
                .bg(theme.surface_2.hsla())
                .shadow_lg()
                .child(
                    Input::new(&self.thread_search.input)
                        .w(px(170.0))
                        .appearance(false)
                        .bordered(false)
                        .cleanable(false)
                        .text_size(px(12.5)),
                )
                .child(
                    div()
                        .min_w(px(34.0))
                        .flex()
                        .justify_end()
                        .font_family("Geist Mono")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme.text_3.hsla())
                        .child(count),
                )
                .child(previous)
                .child(next)
                .child(close)
                .with_animation(
                    ("thread-find-in", self.thread_search.open_transition),
                    Animation::new(theme.motion.fast).with_easing(ease_out_quint()),
                    |find, delta| find.top(px(6.0 + 4.0 * delta)).opacity(delta),
                )
                .into_any_element(),
        )
    }
}

fn search_button(
    id: &'static str,
    icon_path: &'static str,
    theme: crate::Theme,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .size(px(28.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .text_color(theme.text_2.hsla())
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(theme.surface_3.hsla())
                .text_color(theme.text.hsla())
        })
        .active(|style| style.opacity(0.72))
        .on_click(listener)
        .child(svg().path(icon_path).size(px(12.0)))
        .into_any_element()
}

fn item_matches(item: &Item, term: &str) -> bool {
    [
        item.text.as_deref(),
        item.command.as_deref(),
        item.path.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(term))
}

fn next_hit(
    hit_count: usize,
    cursor: usize,
    jumped: bool,
    backwards: bool,
) -> Option<(usize, usize)> {
    if hit_count == 0 {
        return None;
    }
    let next = if !jumped {
        if backwards { hit_count - 1 } else { 0 }
    } else if backwards {
        cursor.checked_sub(1).unwrap_or(hit_count - 1)
    } else {
        (cursor + 1) % hit_count
    };
    Some((next, next))
}

fn turn_starts<'a>(items: impl Iterator<Item = &'a Item>) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut previous: Option<&str> = None;
    for (index, item) in items.enumerate() {
        if previous != Some(item.turn_id.as_str()) || item.turn_id.is_empty() {
            starts.push(index);
        }
        previous = Some(item.turn_id.as_str());
    }
    starts
}

fn neighbour_turn(turns: &[usize], current: usize, backwards: bool) -> Option<usize> {
    if backwards {
        turns.iter().rev().copied().find(|turn| *turn < current)
    } else {
        turns.iter().copied().find(|turn| *turn > current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_protocol::{ItemStatus, ItemType};

    fn item(id: &str, turn_id: &str, text: Option<&str>) -> Item {
        Item {
            id: id.into(),
            turn_id: turn_id.into(),
            item_type: ItemType::Message,
            status: ItemStatus::Completed,
            role: None,
            text: text.map(str::to_owned),
            command: None,
            exit_code: None,
            duration_ms: None,
            path: None,
            lines_added: None,
            lines_removed: None,
            created_at: 0.0,
        }
    }

    #[test]
    fn matching_checks_text_command_and_path_case_insensitively() {
        let mut candidate = item("1", "turn", Some("Hello World"));
        assert!(item_matches(&candidate, "world"));
        candidate.text = None;
        candidate.command = Some("Cargo Test".into());
        assert!(item_matches(&candidate, "cargo"));
        candidate.command = None;
        candidate.path = Some("src/Main.rs".into());
        assert!(item_matches(&candidate, "main.rs"));
    }

    #[test]
    fn first_navigation_lands_on_the_first_or_last_hit_then_wraps() {
        assert_eq!(next_hit(3, 0, false, false), Some((0, 0)));
        assert_eq!(next_hit(3, 0, false, true), Some((2, 2)));
        assert_eq!(next_hit(3, 2, true, false), Some((0, 0)));
        assert_eq!(next_hit(3, 0, true, true), Some((2, 2)));
    }

    #[test]
    fn turn_navigation_uses_consecutive_boundaries() {
        let items = [
            item("1", "a", None),
            item("2", "a", None),
            item("3", "b", None),
            item("4", "b", None),
            item("5", "c", None),
        ];
        let turns = turn_starts(items.iter());
        assert_eq!(turns, vec![0, 2, 4]);
        assert_eq!(neighbour_turn(&turns, 3, true), Some(2));
        assert_eq!(neighbour_turn(&turns, 2, false), Some(4));
    }
}
