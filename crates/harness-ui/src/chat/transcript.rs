use super::presentation::{RowPresentation, TurnPresentation, is_activity};
use super::{ChatEvent, ChatView};
use crate::theme::{CHAT_WIDTH, Theme, ThemeMode};
use chrono::{DateTime, Local};
use gpui::{
    Animation, AnimationExt, AnyElement, App, ClipboardItem, Entity, SharedString, StyleRefinement,
    Styled, Transformation, Window, div, ease_out_quint, list, percentage, prelude::*, px,
    relative, rems, svg,
};
use gpui_component::scroll::ScrollableElement;
use gpui_component::text::{TextView, TextViewStyle};
use harness_protocol::{CheckpointSummary, Item, ItemStatus, ItemType, MessageRole};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WORKING_RAIL_HEIGHT: f32 = 54.0;
type ClickHandler = Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;

#[derive(Clone)]
struct TranscriptRowSnapshot {
    row: usize,
    item: Item,
    presentation: RowPresentation,
    turn: Option<TurnPresentation>,
    visible_activity: Vec<Item>,
    live: bool,
    show_working_rail: bool,
    working: Option<WorkingSnapshot>,
    expanded: bool,
    activity_expanded: bool,
    copied: bool,
    checkpoint_id: Option<u64>,
    theme: Theme,
}

#[derive(Clone)]
struct WorkingSnapshot {
    label: String,
    elapsed_ms: f64,
}

struct CompletionRailSnapshot {
    turn_id: String,
    row: usize,
    activity: Vec<Item>,
    elapsed_ms: f64,
    expanded: bool,
    theme: Theme,
}

impl ChatView {
    pub(super) fn ensure_working_tick(&mut self, cx: &mut gpui::Context<Self>) {
        if !self.state.running || self.working_tick_scheduled {
            return;
        }
        self.working_tick_scheduled = true;
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = view.update(cx, |this, cx| {
                this.working_tick_scheduled = false;
                if this.state.running {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn refresh_work_label(&mut self) {
        let Some(active_turn) = self.state.active_turn() else {
            self.last_work_turn_id = None;
            self.last_specific_work_label = None;
            return;
        };
        if self.last_work_turn_id.as_deref() != Some(active_turn.turn.id.as_str()) {
            self.last_work_turn_id = Some(active_turn.turn.id.clone());
            self.last_specific_work_label = None;
        }
        if let Some(label) = active_turn
            .items
            .iter()
            .rev()
            .find(|item| item.status == ItemStatus::Started && is_activity(item))
            .map(summarise_live)
            .filter(|label| label != "Working")
        {
            self.last_specific_work_label = Some(label);
        }
    }

    pub(super) fn timeline(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        if self.loading && self.state.timeline_len() == 0 {
            return centered_transcript_label("Loading conversation…", self.theme);
        }
        if let Some(error) = &self.error
            && self.state.timeline_len() == 0
        {
            return centered_transcript_label(error.clone(), self.theme);
        }
        if self.state.timeline_len() == 0 && !self.state.running {
            return centered_transcript_label("Start a conversation in this project.", self.theme);
        }

        let orphan_rail = self.orphan_working_rail();
        let view: Entity<Self> = cx.entity();
        let transcript = list(self.list_state.clone(), move |row, window, cx| {
            let snapshot = view.read(cx).transcript_row_snapshot(row);
            snapshot.map_or_else(
                || div().into_any_element(),
                |snapshot| render_transcript_row(snapshot, view.clone(), window, cx),
            )
        })
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .px(px(18.0));

        div()
            .size_full()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(transcript)
            .when_some(orphan_rail, |container, rail| {
                container.child(
                    div()
                        .w_full()
                        .max_w(px(CHAT_WIDTH + 48.0))
                        .mx_auto()
                        .h(px(WORKING_RAIL_HEIGHT))
                        .px(px(18.0))
                        .child(working_rail(rail, self.theme)),
                )
            })
            .into_any_element()
    }

    fn transcript_row_snapshot(&self, row: usize) -> Option<TranscriptRowSnapshot> {
        let item = self.state.item_at_row(row)?.clone();
        let turn = self.presentation.turn(&item.turn_id).cloned();
        let active_turn = self.state.active_turn();
        let live =
            self.state.running && active_turn.is_some_and(|turn| turn.turn.id == item.turn_id);
        let presentation = if live {
            RowPresentation::Normal
        } else {
            self.presentation.row(row)
        };
        let show_working_rail = live
            && turn
                .as_ref()
                .is_some_and(|presentation| presentation.first_response_row == Some(row));
        let visible_activity = match &presentation {
            RowPresentation::ActivityLead { turn_id } => self
                .presentation
                .turn(turn_id)
                .into_iter()
                .flat_map(|turn| turn.visible_activity_rows.iter())
                .filter_map(|row| self.state.item_at_row(*row).cloned())
                .collect(),
            _ => Vec::new(),
        };
        let checkpoint_id = checkpoint_for(&item, &self.stage_settings.checkpoints).map(|it| it.id);

        Some(TranscriptRowSnapshot {
            row,
            expanded: self.expanded_transcript_items.contains(&item.id),
            activity_expanded: self.expanded_activities.contains(&item.turn_id),
            copied: self.copied_transcript_item.as_deref() == Some(item.id.as_str()),
            working: show_working_rail.then(|| self.working_snapshot()),
            item,
            presentation,
            turn,
            visible_activity,
            live,
            show_working_rail,
            checkpoint_id,
            theme: self.theme,
        })
    }

    fn orphan_working_rail(&self) -> Option<WorkingSnapshot> {
        let turn = self.state.active_turn()?;
        let presentation = self.presentation.turn(&turn.turn.id)?;
        presentation
            .first_response_row
            .is_none()
            .then(|| self.working_snapshot())
    }

    fn working_snapshot(&self) -> WorkingSnapshot {
        let started_at = self
            .state
            .active_turn()
            .map_or_else(now_ms, |turn| turn.turn.created_at);
        WorkingSnapshot {
            label: self
                .last_specific_work_label
                .clone()
                .unwrap_or_else(|| "Working".into()),
            elapsed_ms: (now_ms() - started_at).max(0.0),
        }
    }

    fn toggle_transcript_item(
        &mut self,
        item_id: String,
        row: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.expanded_transcript_items.remove(&item_id) {
            self.expanded_transcript_items.insert(item_id);
        }
        self.list_state.splice(row..row + 1, 1);
        cx.notify();
    }

    fn toggle_activity(&mut self, turn_id: String, row: usize, cx: &mut gpui::Context<Self>) {
        if !self.expanded_activities.remove(&turn_id) {
            self.expanded_activities.insert(turn_id);
        }
        self.list_state.splice(row..row + 1, 1);
        cx.notify();
    }

    fn edit_prompt(&mut self, text: String, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.composer.update(cx, |composer, cx| {
            composer.set_value(text, window, cx);
            composer.focus(window, cx);
        });
        cx.notify();
    }

    fn mark_transcript_copied(&mut self, item_id: String, cx: &mut gpui::Context<Self>) {
        self.copied_transcript_item = Some(item_id.clone());
        self.copy_generation = self.copy_generation.wrapping_add(1);
        let generation = self.copy_generation;
        cx.notify();
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1_600))
                .await;
            let _ = view.update(cx, |this, cx| {
                if this.copy_generation == generation
                    && this.copied_transcript_item.as_deref() == Some(item_id.as_str())
                {
                    this.copied_transcript_item = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

fn render_transcript_row(
    snapshot: TranscriptRowSnapshot,
    view: Entity<ChatView>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if snapshot.presentation == RowPresentation::Suppressed {
        return div().h(px(0.0)).into_any_element();
    }

    let rail = snapshot
        .show_working_rail
        .then(|| snapshot.working.clone())
        .flatten();
    let body = match &snapshot.presentation {
        RowPresentation::ActivityLead { turn_id } => completion_rail(
            CompletionRailSnapshot {
                turn_id: turn_id.clone(),
                row: snapshot.row,
                activity: snapshot.visible_activity.clone(),
                elapsed_ms: snapshot.turn.as_ref().map_or(0.0, |turn| turn.elapsed_ms),
                expanded: snapshot.activity_expanded,
                theme: snapshot.theme,
            },
            view.clone(),
            window,
            cx,
        ),
        RowPresentation::FinalAnswer {
            show_completion_rail,
            ..
        } => assistant_message(&snapshot, *show_completion_rail, view.clone(), window, cx),
        RowPresentation::Normal => match (snapshot.item.item_type, snapshot.item.role) {
            (ItemType::Message, Some(MessageRole::User)) => user_message(&snapshot, view.clone()),
            (ItemType::Message, _) => assistant_message(&snapshot, false, view.clone(), window, cx),
            _ => auxiliary_item(&snapshot, view.clone()),
        },
        RowPresentation::Suppressed => unreachable!(),
    };

    div()
        .w_full()
        .max_w(px(CHAT_WIDTH + 48.0))
        .mx_auto()
        .when_some(rail, |row, rail| {
            row.child(
                div()
                    .h(px(WORKING_RAIL_HEIGHT))
                    .child(working_rail(rail, snapshot.theme)),
            )
        })
        .pb(px(if snapshot.live && is_activity(&snapshot.item) {
            10.0
        } else {
            12.0
        }))
        .child(body)
        .into_any_element()
}

fn centered_transcript_label(label: impl Into<SharedString>, theme: Theme) -> AnyElement {
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

fn user_message(snapshot: &TranscriptRowSnapshot, view: Entity<ChatView>) -> AnyElement {
    let Some(text) = snapshot.item.text.clone().filter(|text| !text.is_empty()) else {
        return div().into_any_element();
    };
    let theme = snapshot.theme;
    let group: SharedString = format!("prompt-actions:{}", snapshot.item.id).into();
    let copy_id = snapshot.item.id.clone();
    let copy_text = text.clone();
    let edit_text = text.clone();
    let checkpoint_id = snapshot.checkpoint_id;

    div()
        .w_full()
        .flex()
        .justify_end()
        .child(
            div()
                .group(group.clone())
                .relative()
                .max_w(relative(0.88))
                .px(px(13.0))
                .py(px(9.0))
                .rounded(px(16.0))
                .bg(theme.surface_2.hsla())
                .text_size(px(15.0))
                .line_height(relative(1.52))
                .whitespace_normal()
                .child(text)
                .child(
                    div()
                        .absolute()
                        .right(px(0.0))
                        .bottom(px(-30.0))
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(6.0))
                        .opacity(0.0)
                        .group_hover(group, |actions| actions.opacity(1.0))
                        .child(transcript_action_button(
                            format!("copy-prompt:{}", snapshot.item.id),
                            if snapshot.copied {
                                "icons/check.svg"
                            } else {
                                "icons/copy.svg"
                            },
                            theme,
                            {
                                let view = view.clone();
                                move |_event, _window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_text.clone(),
                                    ));
                                    view.update(cx, |this, cx| {
                                        this.mark_transcript_copied(copy_id.clone(), cx);
                                    });
                                }
                            },
                        ))
                        .child(transcript_action_button(
                            format!("edit-prompt:{}", snapshot.item.id),
                            "icons/pencil.svg",
                            theme,
                            {
                                let view = view.clone();
                                move |_event, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.edit_prompt(edit_text.clone(), window, cx);
                                    });
                                }
                            },
                        ))
                        .when_some(checkpoint_id, |actions, checkpoint_id| {
                            actions.child(transcript_action_button(
                                format!("revert-prompt:{}", snapshot.item.id),
                                "icons/rotate-ccw.svg",
                                theme,
                                {
                                    let view = view.clone();
                                    move |_event, _window, cx| {
                                        view.update(cx, |_this, cx| {
                                            cx.emit(ChatEvent::OpenCheckpoint { checkpoint_id });
                                        });
                                    }
                                },
                            ))
                        }),
                ),
        )
        .into_any_element()
}

fn assistant_message(
    snapshot: &TranscriptRowSnapshot,
    show_completion_rail: bool,
    view: Entity<ChatView>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let text = snapshot.item.text.clone().unwrap_or_default();
    let theme = snapshot.theme;
    let group: SharedString = format!("response-actions:{}", snapshot.item.id).into();
    let copy_id = snapshot.item.id.clone();
    let copy_text = text.clone();
    let show_actions =
        !snapshot.live && snapshot.item.status == ItemStatus::Completed && !text.trim().is_empty();
    let elapsed_ms = snapshot.turn.as_ref().map_or(0.0, |turn| turn.elapsed_ms);

    div()
        .group(group.clone())
        .relative()
        .w_full()
        .pb(px(32.0))
        .text_size(px(15.0))
        .line_height(relative(1.52))
        .text_color(theme.response_text.hsla())
        .when(show_completion_rail, |reply| {
            reply.child(div().mb(px(18.0)).child(completion_summary(
                format!("empty-completion:{}", snapshot.item.id),
                elapsed_ms,
                false,
                theme,
                None,
            )))
        })
        .child(markdown_view(
            format!("assistant-markdown:{}", snapshot.item.id),
            text,
            theme,
            window,
            cx,
        ))
        .when(show_actions, |reply| {
            reply.child(
                div()
                    .absolute()
                    .bottom(px(0.0))
                    .left(px(0.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .text_size(px(12.5))
                    .text_color(theme.text_3.hsla())
                    .opacity(0.0)
                    .group_hover(group, |actions| actions.opacity(1.0))
                    .child(transcript_action_button(
                        format!("copy-response:{}", snapshot.item.id),
                        if snapshot.copied {
                            "icons/check.svg"
                        } else {
                            "icons/copy.svg"
                        },
                        theme,
                        {
                            let view = view.clone();
                            move |_event, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                                view.update(cx, |this, cx| {
                                    this.mark_transcript_copied(copy_id.clone(), cx);
                                });
                            }
                        },
                    ))
                    .child(format_message_time(snapshot.item.created_at)),
            )
        })
        .into_any_element()
}

fn completion_rail(
    snapshot: CompletionRailSnapshot,
    view: Entity<ChatView>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let has_visible_activity = !snapshot.activity.is_empty();
    let toggle_turn_id = snapshot.turn_id.clone();
    let row = snapshot.row;
    let theme = snapshot.theme;
    div()
        .w_full()
        .child(completion_summary(
            format!("completion:{}", snapshot.turn_id),
            snapshot.elapsed_ms,
            snapshot.expanded,
            theme,
            has_visible_activity.then(|| {
                Rc::new({
                    let view = view.clone();
                    move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                        view.update(cx, |this, cx| {
                            this.toggle_activity(toggle_turn_id.clone(), row, cx);
                        });
                    }
                }) as ClickHandler
            }),
        ))
        .when(snapshot.expanded && has_visible_activity, |rail| {
            rail.child(
                div()
                    .mt(px(21.0))
                    .mb(px(6.0))
                    .flex()
                    .flex_col()
                    .gap(px(22.0))
                    .children(snapshot.activity.iter().map(|item| {
                        match item.item_type {
                            ItemType::Message => div()
                                .text_size(px(15.0))
                                .line_height(relative(1.52))
                                .text_color(theme.response_text.hsla())
                                .child(markdown_view(
                                    format!("activity-markdown:{}", item.id),
                                    item.text.clone().unwrap_or_default(),
                                    theme,
                                    window,
                                    cx,
                                ))
                                .into_any_element(),
                            _ => div()
                                .min_h(px(24.0))
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .text_size(px(15.0))
                                .text_color(theme.text_3.hsla())
                                .child(transcript_svg("icons/file-pen-line.svg", 15.0))
                                .child("Edited files")
                                .into_any_element(),
                        }
                    })),
            )
        })
        .into_any_element()
}

fn completion_summary(
    id: String,
    elapsed_ms: f64,
    expanded: bool,
    theme: Theme,
    on_click: Option<ClickHandler>,
) -> AnyElement {
    div()
        .id(SharedString::from(id))
        .min_h(px(36.0))
        .pt(px(1.0))
        .px(px(2.0))
        .pb(px(9.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(px(15.0))
        .text_color(theme.text_3.hsla())
        .when(on_click.is_some(), |summary| {
            summary
                .cursor_pointer()
                .hover(move |style| style.text_color(theme.text_2.hsla()))
        })
        .child(format!("Worked for {}", worked_for(elapsed_ms)))
        .when(on_click.is_some(), |summary| {
            summary.child(
                transcript_svg("icons/chevron-right.svg", 15.0).when(expanded, |icon| {
                    icon.with_transformation(Transformation::rotate(percentage(0.25)))
                }),
            )
        })
        .when_some(on_click, |summary, on_click| {
            summary.on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .into_any_element()
}

fn working_rail(working: WorkingSnapshot, theme: Theme) -> AnyElement {
    div()
        .min_h(px(36.0))
        .pt(px(1.0))
        .px(px(2.0))
        .pb(px(9.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(px(15.0))
        .text_color(theme.text_3.hsla())
        .child(
            div()
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path("icons/loader-circle.svg")
                        .size(px(16.0))
                        .text_color(theme.text_3.hsla())
                        .with_animation(
                            "transcript-working-orb",
                            Animation::new(Duration::from_millis(900)).repeat(),
                            |spinner, delta| {
                                spinner
                                    .with_transformation(Transformation::rotate(percentage(delta)))
                            },
                        ),
                ),
        )
        .child(div().child(working.label).with_animation(
            "transcript-working-label",
            Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
            |label, delta| label.opacity(delta),
        ))
        .child(
            div()
                .text_color(theme.text_3.hsla().opacity(0.72))
                .child(format!("·  {}", worked_for(working.elapsed_ms))),
        )
        .into_any_element()
}

fn auxiliary_item(snapshot: &TranscriptRowSnapshot, view: Entity<ChatView>) -> AnyElement {
    let item = &snapshot.item;
    let theme = snapshot.theme;
    let live = snapshot.live && is_activity(item);
    let label = if live {
        summarise_live(item)
    } else {
        summarise(item)
    };
    let item_id = item.id.clone();
    let row = snapshot.row;
    let output = item.text.clone().unwrap_or_default();
    let icon = glyph(item);
    let failed = item.item_type == ItemType::Error;

    div()
        .w_full()
        .child(
            div()
                .id(SharedString::from(format!("aux-row:{}", item.id)))
                .min_h(px(if live { 30.0 } else { 0.0 }))
                .py(px(if live { 2.0 } else { 1.0 }))
                .flex()
                .items_center()
                .gap(px(if live { 10.0 } else { 8.0 }))
                .cursor_pointer()
                .text_color(if failed {
                    theme.error.hsla()
                } else if live {
                    theme.text_2.hsla()
                } else {
                    theme.text_3.hsla()
                })
                .hover(move |style| {
                    if failed {
                        style.text_color(theme.error.hsla())
                    } else {
                        style.text_color(theme.text_2.hsla())
                    }
                })
                .on_click(move |_event, _window, cx| {
                    view.update(cx, |this, cx| {
                        this.toggle_transcript_item(item_id.clone(), row, cx);
                    });
                })
                .child(
                    div()
                        .w(px(if live { 22.0 } else { 14.0 }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(transcript_svg(icon, if live { 16.0 } else { 13.0 })),
                )
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .font_family(if live { "Geist" } else { "Geist Mono" })
                        .text_size(px(if live { 15.0 } else { 12.5 }))
                        .child(label),
                )
                .when_some(
                    item.exit_code.filter(|exit_code| *exit_code != 0),
                    |row, exit_code| {
                        row.child(
                            div()
                                .font_family("Geist Mono")
                                .text_size(px(11.5))
                                .text_color(theme.error.hsla())
                                .child(format!("exit {exit_code}")),
                        )
                    },
                )
                .when(
                    !live && item.duration_ms.is_some_and(|duration| duration >= 1_000.0),
                    |row| {
                        row.child(
                            div()
                                .font_family("Geist Mono")
                                .text_size(px(11.5))
                                .text_color(theme.text_3.hsla())
                                .child(duration(item.duration_ms.unwrap_or_default())),
                        )
                    },
                )
                .when(!live && item.status == ItemStatus::Started, |row| {
                    row.child(
                        svg()
                            .path("icons/loader-circle.svg")
                            .size(px(13.0))
                            .text_color(theme.text_3.hsla())
                            .with_animation(
                                SharedString::from(format!("aux-spinner:{}", item.id)),
                                Animation::new(Duration::from_millis(900)).repeat(),
                                |spinner, delta| {
                                    spinner.with_transformation(Transformation::rotate(percentage(
                                        delta,
                                    )))
                                },
                            ),
                    )
                }),
        )
        .when(snapshot.expanded && !output.trim().is_empty(), |aux| {
            aux.child(
                div()
                    .id(SharedString::from(format!("aux-output:{}", item.id)))
                    .mt(px(8.0))
                    .mb(px(2.0))
                    .ml(px(if live { 32.0 } else { 0.0 }))
                    .max_h(px(300.0))
                    .overflow_y_scrollbar()
                    .rounded(px(8.0))
                    .bg(theme.surface.hsla())
                    .px(px(11.0))
                    .py(px(9.0))
                    .font_family("Geist Mono")
                    .text_size(px(12.5))
                    .line_height(relative(1.45))
                    .text_color(theme.text_2.hsla())
                    .whitespace_normal()
                    .child(output),
            )
        })
        .into_any_element()
}

fn markdown_view(
    id: String,
    text: String,
    theme: Theme,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let code_style = StyleRefinement::default()
        .bg(theme.surface.hsla())
        .border_1()
        .border_color(theme.line.hsla())
        .rounded(px(8.0))
        .px(px(13.0))
        .py(px(11.0))
        .font_family("Geist Mono")
        .text_size(px(12.5));
    let mut text_style = TextViewStyle::default()
        .paragraph_gap(rems(0.5))
        .heading_font_size(|level, _base| match level {
            1 => px(20.0),
            2 => px(15.0),
            _ => px(13.5),
        })
        .code_block(code_style);
    text_style.heading_base_font_size = px(15.0);
    text_style.is_dark = theme.mode == ThemeMode::Dark;

    TextView::markdown(SharedString::from(id), text, window, cx)
        .style(text_style)
        .selectable(true)
        .w_full()
        .max_w(px(690.0))
        .font_family("Geist")
        .text_size(px(15.0))
        .line_height(relative(1.52))
        .text_color(theme.response_text.hsla())
        .code_block_actions(move |block, _window, _cx| {
            let code = block.code().to_string();
            let id = format!("copy-code:{}", stable_hash(&code));
            div()
                .id(SharedString::from(id))
                .size(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.0))
                .border_1()
                .border_color(theme.line.hsla())
                .bg(theme.surface_2.hsla())
                .text_color(theme.text_3.hsla())
                .cursor_pointer()
                .hover(move |style| style.text_color(theme.text.hsla()))
                .on_click(move |_event, _window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                })
                .child(transcript_svg("icons/copy.svg", 13.0))
        })
        .into_any_element()
}

fn transcript_action_button(
    id: String,
    icon: &'static str,
    theme: Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(SharedString::from(id))
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .text_color(theme.text_3.hsla())
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(theme.surface.hsla())
                .text_color(theme.text_2.hsla())
        })
        .on_click(on_click)
        .child(transcript_svg(icon, 15.0))
        .into_any_element()
}

fn transcript_svg(path: &'static str, size: f32) -> gpui::Svg {
    svg().path(path).size(px(size))
}

fn checkpoint_for<'a>(
    item: &Item,
    checkpoints: &'a [CheckpointSummary],
) -> Option<&'a CheckpointSummary> {
    if item.item_type != ItemType::Message
        || item.role != Some(MessageRole::User)
        || item.text.as_ref().is_none_or(|text| text.is_empty())
    {
        return None;
    }
    checkpoints.iter().rev().find(|checkpoint| {
        item.text.as_deref() == Some(checkpoint.label.as_str())
            && checkpoint.created_at <= item.created_at
    })
}

fn format_message_time(timestamp_ms: f64) -> SharedString {
    DateTime::from_timestamp_millis(timestamp_ms.round() as i64).map_or_else(
        || SharedString::from(""),
        |date| SharedString::from(date.with_timezone(&Local).format("%-I:%M %p").to_string()),
    )
}

fn duration(ms: f64) -> String {
    if ms < 60_000.0 {
        return format!("{}s", (ms / 100.0).round() / 10.0);
    }
    format!(
        "{}m {}s",
        (ms / 60_000.0).floor() as u64,
        ((ms % 60_000.0) / 1_000.0).round() as u64
    )
}

fn worked_for(ms: f64) -> String {
    let seconds = (ms / 1_000.0).round().max(1.0) as u64;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    let remainder = seconds % 60;
    if remainder == 0 {
        format!("{minutes}m")
    } else {
        format!("{minutes}m {remainder}s")
    }
}

fn glyph(item: &Item) -> &'static str {
    match item.item_type {
        ItemType::Command => "icons/square-terminal.svg",
        ItemType::Reasoning => "icons/brain.svg",
        ItemType::FileChange => "icons/file-pen-line.svg",
        ItemType::ToolCall => {
            let text = tool_text(item);
            if text.contains("image") {
                "icons/images.svg"
            } else if contains_any(&text, &["read", "open", "file"]) {
                "icons/book-open.svg"
            } else if text.contains("search") {
                "icons/search.svg"
            } else {
                "icons/wrench.svg"
            }
        }
        ItemType::Plan => "icons/list-checks.svg",
        ItemType::Error => "icons/circle-alert.svg",
        _ => "icons/circle-question-mark.svg",
    }
}

fn summarise_live(item: &Item) -> String {
    let ongoing = item.status == ItemStatus::Started;
    match item.item_type {
        ItemType::Command => if ongoing {
            "Running a command"
        } else {
            "Ran a command"
        }
        .into(),
        ItemType::Reasoning => "Thinking".into(),
        ItemType::FileChange => if ongoing {
            "Editing files"
        } else {
            "Edited files"
        }
        .into(),
        ItemType::ToolCall => {
            let text = tool_text(item);
            if let Some(label) = super::design_phase_label(&text) {
                return label.into();
            }
            if text.contains("image") {
                if ongoing {
                    "Viewing an image"
                } else {
                    "Viewed an image"
                }
                .into()
            } else if contains_any(&text, &["read", "open", "file"]) {
                if ongoing {
                    "Reading files"
                } else {
                    "Read files"
                }
                .into()
            } else if text.contains("search") {
                if ongoing { "Searching" } else { "Searched" }.into()
            } else if ongoing {
                "Using a tool".into()
            } else {
                "Used a tool".into()
            }
        }
        ItemType::Plan => if ongoing {
            "Updating the plan"
        } else {
            "Updated the plan"
        }
        .into(),
        _ => summarise(item),
    }
}

fn summarise(item: &Item) -> String {
    match item.item_type {
        ItemType::Command => item.command.clone().unwrap_or_else(|| "command".into()),
        ItemType::Reasoning => "Thinking".into(),
        ItemType::FileChange => "Edited files".into(),
        ItemType::ToolCall => item.text.clone().unwrap_or_else(|| "Tool call".into()),
        ItemType::Plan => "Plan".into(),
        ItemType::Error => item.text.clone().unwrap_or_else(|| "Error".into()),
        ItemType::Unknown => "unknown".into(),
        ItemType::Message => item.text.clone().unwrap_or_default(),
    }
}

fn tool_text(item: &Item) -> String {
    format!(
        "{} {}",
        item.text.as_deref().unwrap_or_default(),
        item.command.as_deref().unwrap_or_default()
    )
    .to_lowercase()
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64() * 1_000.0)
}

fn stable_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(item_type: ItemType, status: ItemStatus, text: &str) -> Item {
        Item {
            id: "item".into(),
            turn_id: "turn".into(),
            item_type,
            status,
            role: None,
            text: Some(text.into()),
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
    fn durations_match_the_web_presentation() {
        assert_eq!(duration(950.0), "1s");
        assert_eq!(duration(61_200.0), "1m 1s");
        assert_eq!(worked_for(0.0), "1s");
        assert_eq!(worked_for(60_000.0), "1m");
        assert_eq!(worked_for(62_000.0), "1m 2s");
    }

    #[test]
    fn live_tool_labels_are_capability_neutral_and_contextual() {
        assert_eq!(
            summarise_live(&item(
                ItemType::ToolCall,
                ItemStatus::Started,
                "search_query"
            )),
            "Searching"
        );
        assert_eq!(
            summarise_live(&item(ItemType::ToolCall, ItemStatus::Started, "open file")),
            "Reading files"
        );
        assert_eq!(
            summarise_live(&item(
                ItemType::ToolCall,
                ItemStatus::Started,
                "design:build"
            )),
            "Building the website"
        );
    }
}
