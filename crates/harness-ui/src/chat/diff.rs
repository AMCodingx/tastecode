use super::*;
use harness_protocol::{DiffFile, DiffFileStatus, DiffHunk, DiffLine, PlanStepStatus, SessionDiff};

#[derive(Default)]
pub(super) struct DiffUiState {
    source: Option<String>,
    summary: Option<DiffSummary>,
    reviewing: bool,
    snapshot: Option<SessionDiff>,
    loading: bool,
    show_slow_load: bool,
    busy: bool,
    status: Option<String>,
    stale_refresh: bool,
    show_all_files: bool,
}

impl DiffUiState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn sync_summary(&mut self, source: Option<&str>) {
        if self.source.as_deref() == source {
            return;
        }
        self.source = source.map(str::to_owned);
        self.summary = source.and_then(parse_diff_summary);
        self.reviewing = false;
        self.snapshot = None;
        self.loading = false;
        self.show_slow_load = false;
        self.busy = false;
        self.status = None;
        self.stale_refresh = false;
        self.show_all_files = false;
    }

    pub(super) fn apply_snapshot(&mut self, diff: SessionDiff) {
        self.snapshot = Some(diff);
        self.loading = false;
        self.show_slow_load = false;
        self.busy = false;
        self.status = self
            .stale_refresh
            .then(|| "The diff changed and was refreshed. Choose the hunk decision again.".into());
        self.stale_refresh = false;
    }

    pub(super) fn apply_error(&mut self, message: String, stale: bool) -> bool {
        self.busy = false;
        if stale {
            self.loading = true;
            self.show_slow_load = false;
            self.stale_refresh = true;
            self.status = None;
            true
        } else {
            self.loading = false;
            self.show_slow_load = false;
            self.status = Some(message);
            false
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiffSummary {
    added: usize,
    removed: usize,
    files: Vec<DiffSummaryFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiffSummaryFile {
    path: String,
    added: usize,
    removed: usize,
}

impl ChatView {
    pub(super) fn sync_diff_summary(&mut self) {
        self.diff_ui
            .sync_summary(self.state.diff.as_ref().map(|(_, source)| source.as_str()));
    }

    pub(super) fn control_surface(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.state.running {
            let steps = &self.state.plan.as_ref()?.1;
            let current = steps
                .iter()
                .find(|step| step.status == PlanStepStatus::Running)
                .or_else(|| {
                    steps
                        .iter()
                        .find(|step| step.status == PlanStepStatus::Pending)
                })?;
            return Some(
                div()
                    .flex_none()
                    .w_full()
                    .px(px(24.0))
                    .pb(px(4.0))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(CHAT_WIDTH))
                            .mx_auto()
                            .text_size(px(14.0))
                            .text_color(self.theme.text_3.hsla())
                            .child(current.text.clone()),
                    )
                    .into_any_element(),
            );
        }
        self.diff_card(cx)
    }

    fn toggle_diff_review(&mut self, cx: &mut Context<Self>) {
        if self.diff_ui.summary.is_none() || self.state.running {
            return;
        }
        self.diff_ui.reviewing = !self.diff_ui.reviewing;
        self.diff_ui.status = None;
        if self.diff_ui.reviewing {
            self.request_diff_snapshot(cx);
        } else {
            self.diff_ui.loading = false;
            self.diff_ui.show_slow_load = false;
            self.diff_ui.busy = false;
        }
        cx.notify();
    }

    fn request_diff_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
        else {
            return;
        };
        self.diff_ui.loading = true;
        self.diff_ui.show_slow_load = false;
        cx.emit(ChatEvent::RequestDiff { thread_id });
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = view.update(cx, |this, cx| {
                if this.diff_ui.reviewing && this.diff_ui.loading && this.diff_ui.snapshot.is_none()
                {
                    this.diff_ui.show_slow_load = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn refresh_diff(&mut self, cx: &mut Context<Self>) {
        if self.diff_ui.busy {
            return;
        }
        self.diff_ui.status = None;
        self.request_diff_snapshot(cx);
        cx.notify();
    }

    fn decide_hunk(
        &mut self,
        path: String,
        hunk_id: String,
        decision: DiffDecision,
        cx: &mut Context<Self>,
    ) {
        if self.diff_ui.busy {
            return;
        }
        let Some(snapshot) = &self.diff_ui.snapshot else {
            return;
        };
        let Some(thread_id) = self
            .session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
        else {
            return;
        };
        self.diff_ui.busy = true;
        self.diff_ui.status = None;
        cx.emit(ChatEvent::ReviewHunk {
            thread_id,
            version: snapshot.version.clone(),
            path,
            hunk_id,
            decision,
        });
        cx.notify();
    }

    fn toggle_all_diff_files(&mut self, cx: &mut Context<Self>) {
        self.diff_ui.show_all_files = !self.diff_ui.show_all_files;
        cx.notify();
    }

    fn diff_card(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let summary = self.diff_ui.summary.as_ref()?;
        let theme = self.theme;
        let visible_files = if self.diff_ui.show_all_files {
            summary.files.len()
        } else {
            summary.files.len().min(3)
        };
        let file_rows = summary
            .files
            .iter()
            .take(visible_files)
            .map(|file| diff_summary_file(file, theme));
        let hidden = summary.files.len().saturating_sub(visible_files);
        let reviewing = self.diff_ui.reviewing;

        Some(
            div()
                .flex_none()
                .w_full()
                .px(px(24.0))
                .pb(px(7.0))
                .child(
                    div()
                        .w_full()
                        .max_w(px(CHAT_WIDTH))
                        .mx_auto()
                        .overflow_hidden()
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(theme.line.hsla())
                        .child(
                            div()
                                .min_h(px(76.0))
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .px(px(14.0))
                                .py(px(12.0))
                                .child(
                                    div()
                                        .size(px(42.0))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(9.0))
                                        .bg(theme.surface.hsla())
                                        .text_color(theme.text_2.hsla())
                                        .child(svg_icon("icons/file-diff.svg", 20.0)),
                                )
                                .child(
                                    div()
                                        .min_w(px(0.0))
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(format!(
                                                    "Edited {} file{}",
                                                    summary.files.len(),
                                                    if summary.files.len() == 1 { "" } else { "s" }
                                                )),
                                        )
                                        .child(diff_stat(summary.added, summary.removed, theme)),
                                )
                                .child(diff_pill_button(
                                    "diff-toggle-review",
                                    if reviewing { "Close" } else { "Review" },
                                    false,
                                    theme,
                                    Some({
                                        let weak = cx.weak_entity();
                                        Rc::new(move |cx: &mut App| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.toggle_diff_review(cx);
                                            });
                                        })
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .border_t_1()
                                .border_color(theme.line.hsla())
                                .px(px(18.0))
                                .py(px(11.0))
                                .flex()
                                .flex_col()
                                .gap(px(9.0))
                                .children(file_rows)
                                .when(summary.files.len() > 3, |list| {
                                    let weak = cx.weak_entity();
                                    list.child(
                                        div()
                                            .id("diff-toggle-files")
                                            .h(px(30.0))
                                            .flex()
                                            .items_center()
                                            .gap(px(7.0))
                                            .text_size(px(12.0))
                                            .text_color(theme.text_2.hsla())
                                            .cursor_pointer()
                                            .hover(move |style| style.text_color(theme.text.hsla()))
                                            .on_click(move |_event, _window, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.toggle_all_diff_files(cx);
                                                });
                                            })
                                            .child(if self.diff_ui.show_all_files {
                                                "Show fewer files".into()
                                            } else {
                                                format!(
                                                    "Show {hidden} more file{}",
                                                    if hidden == 1 { "" } else { "s" }
                                                )
                                            }),
                                    )
                                }),
                        )
                        .when(reviewing, |card| card.child(self.diff_review(cx))),
                )
                .into_any_element(),
        )
    }

    fn diff_review(&self, cx: &Context<Self>) -> AnyElement {
        let theme = self.theme;
        let refresh_action: Option<UiAction> = (!self.diff_ui.busy).then(|| {
            let weak = cx.weak_entity();
            Rc::new(move |cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.refresh_diff(cx));
            }) as UiAction
        });
        let snapshot = self.diff_ui.snapshot.as_ref();
        let file_count = snapshot.map_or(0, |snapshot| snapshot.files.len());

        div()
            .id("diff-review-scroll")
            .max_h(px(520.0))
            .overflow_y_scroll()
            .border_t_1()
            .border_color(theme.line.hsla())
            .child(
                div()
                    .min_h(px(42.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(14.0))
                    .py(px(7.0))
                    .text_size(px(11.5))
                    .text_color(theme.text_3.hsla())
                    .child(div().flex_1().child(if snapshot.is_some() {
                        format!(
                            "{file_count} file{} in current snapshot",
                            if file_count == 1 { "" } else { "s" }
                        )
                    } else {
                        "Review unavailable".into()
                    }))
                    .child(diff_pill_button(
                        "diff-refresh",
                        "↻ Refresh",
                        false,
                        theme,
                        refresh_action,
                    )),
            )
            .when_some(self.diff_ui.status.clone(), |review, status| {
                review.child(diff_review_status(status, theme))
            })
            .when(
                snapshot.is_none() && self.diff_ui.loading && self.diff_ui.show_slow_load,
                |review| review.child(diff_review_status("Loading review…", theme)),
            )
            .when(
                snapshot.is_some_and(|snapshot| snapshot.files.is_empty()),
                |review| review.child(diff_review_status("No changed files remain.", theme)),
            )
            .when_some(snapshot, |review, snapshot| {
                review.children(
                    snapshot
                        .files
                        .iter()
                        .enumerate()
                        .map(|(index, file)| self.diff_review_file(file, index, cx)),
                )
            })
            .into_any_element()
    }

    fn diff_review_file(
        &self,
        file: &DiffFile,
        file_index: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let status = if let Some(previous) = &file.previous_path {
            format!("{previous} → {}", diff_file_status(file.status))
        } else {
            diff_file_status(file.status).into()
        };
        div()
            .border_t_1()
            .border_color(theme.line.hsla())
            .child(
                div()
                    .min_h(px(54.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(14.0))
                    .py(px(7.0))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .truncate()
                            .font_family("Geist Mono")
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(file.path.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_3.hsla())
                            .child(status),
                    ),
            )
            .when(file.binary, |section| {
                section.child(diff_review_status("Binary file", theme))
            })
            .children(file.hunks.iter().enumerate().map(|(hunk_index, hunk)| {
                self.diff_hunk(&file.path, hunk, file_index, hunk_index, cx)
            }))
            .into_any_element()
    }

    fn diff_hunk(
        &self,
        path: &str,
        hunk: &DiffHunk,
        file_index: usize,
        hunk_index: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let busy = self.diff_ui.busy;
        let decisions = [
            ("✓ Accept", DiffDecision::Accept),
            ("× Reject", DiffDecision::Reject),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (label, decision))| {
            let selected = hunk.decision == Some(decision);
            let enabled = !busy && !selected;
            let action: Option<UiAction> = enabled.then(|| {
                let weak = cx.weak_entity();
                let path = path.to_owned();
                let hunk_id = hunk.id.clone();
                Rc::new(move |cx: &mut App| {
                    let path = path.clone();
                    let hunk_id = hunk_id.clone();
                    let _ = weak.update(cx, |this, cx| {
                        this.decide_hunk(path, hunk_id, decision, cx);
                    });
                }) as UiAction
            });
            diff_pill_button(
                (
                    if index == 0 {
                        "diff-accept"
                    } else {
                        "diff-reject"
                    },
                    file_index * 1_000 + hunk_index,
                ),
                label,
                selected,
                theme,
                action,
            )
            .into_any_element()
        });
        let reviewed = hunk.decision.is_some();

        div()
            .id(("diff-hunk", file_index * 1_000 + hunk_index))
            .border_t_1()
            .border_color(theme.line.hsla())
            .bg(if reviewed {
                theme.surface.hsla().opacity(0.45)
            } else {
                theme.background.hsla()
            })
            .child(
                div()
                    .min_h(px(48.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(7.0))
                    .bg(theme.surface_2.hsla())
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .truncate()
                            .font_family("Geist Mono")
                            .text_size(px(10.5))
                            .text_color(theme.text_3.hsla())
                            .child(hunk.header.clone()),
                    )
                    .child(div().flex().items_center().gap(px(6.0)).children(decisions)),
            )
            .children(hunk.lines.iter().map(|line| diff_line(line, theme)))
            .into_any_element()
    }
}

fn diff_summary_file(file: &DiffSummaryFile, theme: Theme) -> AnyElement {
    let (directory, name) = split_path(&file.path);
    div()
        .min_h(px(30.0))
        .flex()
        .items_center()
        .gap(px(14.0))
        .text_size(px(12.0))
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .flex()
                .truncate()
                .text_color(theme.text_2.hsla())
                .when(!directory.is_empty(), |path| {
                    path.child(div().text_color(theme.text_3.hsla()).child(directory))
                })
                .child(name),
        )
        .child(diff_stat(file.added, file.removed, theme))
        .into_any_element()
}

fn diff_stat(added: usize, removed: usize, theme: Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .font_family("Geist Mono")
        .text_size(px(11.5))
        .child(
            div()
                .text_color(theme.success.hsla())
                .child(format!("+{added}")),
        )
        .child(
            div()
                .text_color(theme.error.hsla())
                .child(format!("−{removed}")),
        )
}

fn diff_pill_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    selected: bool,
    theme: Theme,
    action: Option<UiAction>,
) -> impl IntoElement {
    let enabled = action.is_some();
    let reject = label.contains("Reject");
    div()
        .id(id)
        .h(px(30.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(15.0))
        .border_1()
        .border_color(if selected && reject {
            theme.error.hsla().opacity(0.55)
        } else if selected {
            theme.success.hsla().opacity(0.55)
        } else {
            theme.line_strong.hsla()
        })
        .bg(if selected && reject {
            theme.error.hsla().opacity(0.12)
        } else if selected {
            theme.success.hsla().opacity(0.12)
        } else {
            theme.background.hsla()
        })
        .text_size(px(11.5))
        .text_color(if selected && reject {
            theme.error.hsla()
        } else if selected {
            theme.success.hsla()
        } else {
            theme.text.hsla()
        })
        .opacity(if enabled || selected { 1.0 } else { 0.45 })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface.hsla()))
                .active(|style| style.opacity(0.72))
        })
        .when_some(action, |button, action| {
            button.on_click(move |_event, _window, cx| action(cx))
        })
        .child(label)
}

fn diff_review_status(status: impl Into<SharedString>, theme: Theme) -> impl IntoElement {
    div()
        .px(px(14.0))
        .py(px(12.0))
        .text_size(px(11.5))
        .text_color(theme.text_3.hsla())
        .child(status.into())
}

fn diff_line(line: &DiffLine, theme: Theme) -> AnyElement {
    let (old_line, new_line, marker, text, background, color) = match line {
        DiffLine::Context {
            old_line,
            new_line,
            text,
            ..
        } => (
            Some(*old_line),
            Some(*new_line),
            " ",
            text,
            theme.background.hsla(),
            theme.text_2.hsla(),
        ),
        DiffLine::Addition { new_line, text, .. } => (
            None,
            Some(*new_line),
            "+",
            text,
            theme.success.hsla().opacity(0.11),
            theme.response_text.hsla(),
        ),
        DiffLine::Deletion { old_line, text, .. } => (
            Some(*old_line),
            None,
            "−",
            text,
            theme.error.hsla().opacity(0.11),
            theme.response_text.hsla(),
        ),
    };
    div()
        .min_h(px(22.0))
        .w_full()
        .flex()
        .items_center()
        .bg(background)
        .font_family("Geist Mono")
        .text_size(px(10.5))
        .child(diff_line_number(old_line, theme))
        .child(diff_line_number(new_line, theme))
        .child(
            div()
                .w(px(20.0))
                .flex_none()
                .text_color(if marker == "+" {
                    theme.success.hsla()
                } else if marker == "−" {
                    theme.error.hsla()
                } else {
                    theme.text_3.hsla()
                })
                .child(marker),
        )
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .text_color(color)
                .whitespace_nowrap()
                .child(if text.is_empty() {
                    " ".into()
                } else {
                    text.clone()
                }),
        )
        .into_any_element()
}

fn diff_line_number(line: Option<u32>, theme: Theme) -> impl IntoElement {
    div()
        .w(px(38.0))
        .flex_none()
        .flex()
        .justify_end()
        .pr(px(7.0))
        .text_color(theme.text_3.hsla())
        .child(line.map_or_else(String::new, |line| line.to_string()))
}

fn diff_file_status(status: DiffFileStatus) -> &'static str {
    match status {
        DiffFileStatus::Added => "added",
        DiffFileStatus::Modified => "modified",
        DiffFileStatus::Deleted => "deleted",
        DiffFileStatus::Renamed => "renamed",
    }
}

fn split_path(path: &str) -> (String, String) {
    let separator = path.rfind(['/', '\\']);
    match separator {
        Some(index) => (path[..=index].into(), path[index + 1..].into()),
        None => (String::new(), path.into()),
    }
}

fn parse_diff_summary(diff: &str) -> Option<DiffSummary> {
    let mut files = Vec::<DiffSummaryFile>::new();
    let mut current = None;
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            let path = path_from_git_header(line);
            files.push(DiffSummaryFile {
                path,
                added: 0,
                removed: 0,
            });
            current = Some(files.len() - 1);
            in_hunk = false;
        } else if !in_hunk
            && (line.starts_with("+++ ") || line.starts_with("--- ") || line.starts_with("index "))
        {
            if let Some(index) = current
                && let Some(destination) = line.strip_prefix("+++ ")
            {
                let destination = destination.trim();
                if destination != "/dev/null" {
                    files[index].path = strip_git_prefix(destination);
                }
            }
        } else if line.starts_with("@@") {
            in_hunk = true;
        } else if line.starts_with('+') {
            let index = *current.get_or_insert_with(|| {
                files.push(DiffSummaryFile {
                    path: "Changes".into(),
                    added: 0,
                    removed: 0,
                });
                files.len() - 1
            });
            files[index].added += 1;
        } else if line.starts_with('-') {
            let index = *current.get_or_insert_with(|| {
                files.push(DiffSummaryFile {
                    path: "Changes".into(),
                    added: 0,
                    removed: 0,
                });
                files.len() - 1
            });
            files[index].removed += 1;
        }
    }

    if files.is_empty() {
        return None;
    }
    let added = files.iter().map(|file| file.added).sum();
    let removed = files.iter().map(|file| file.removed).sum();
    Some(DiffSummary {
        added,
        removed,
        files,
    })
}

fn path_from_git_header(line: &str) -> String {
    if let Some(marker) = line.rfind(" b/") {
        return strip_git_prefix(&line[marker + 1..]);
    }
    if let Some(marker) = line.rfind(" \"b/") {
        return strip_git_prefix(line[marker + 2..].trim_end_matches('"'));
    }
    "Changes".into()
}

fn strip_git_prefix(path: &str) -> String {
    let path = path.trim_matches('"');
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_distinguishes_headers_from_changes_inside_hunks() {
        let summary = parse_diff_summary(
            "diff --git a/src/app.ts b/src/app.ts\nindex 123..456 100644\n--- a/src/app.ts\n+++ b/src/app.ts\n@@ -1,2 +1,2 @@\n--- reset the sequence\n+++ continue the sequence\n context",
        )
        .unwrap();

        assert_eq!(summary.added, 1);
        assert_eq!(summary.removed, 1);
        assert_eq!(summary.files[0].path, "src/app.ts");
    }

    #[test]
    fn summary_counts_each_file_independently() {
        let summary = parse_diff_summary(
            "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.ts b/b.ts\n--- a/b.ts\n+++ b/b.ts\n@@ -0,0 +1 @@\n+added",
        )
        .unwrap();

        assert_eq!(summary.files.len(), 2);
        assert_eq!((summary.added, summary.removed), (2, 1));
        assert_eq!((summary.files[1].added, summary.files[1].removed), (1, 0));
    }
}
