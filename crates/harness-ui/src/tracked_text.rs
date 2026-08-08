use gpui::{
    App, Bounds, Element, ElementId, FontFeatures, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, Size, TextAlign, TextRun, Window, point, size,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Paints one line of uniformly styled text with CSS-compatible letter spacing.
///
/// GPUI 0.2.2 does not expose tracking in `TextStyle`, but its shaped glyph
/// positions are public. Keeping the adjustment here preserves GPUI's native
/// shaping, font fallback, glyph cache, and rasterization while matching the
/// handful of tracked labels in the web oracle.
pub(crate) fn tracked_text(text: impl Into<SharedString>, letter_spacing_em: f32) -> TrackedText {
    TrackedText {
        text: text.into(),
        letter_spacing_em,
    }
}

pub(crate) struct TrackedText {
    text: SharedString,
    letter_spacing_em: f32,
}

#[derive(Clone, Default)]
pub(crate) struct TrackedTextLayout(Rc<RefCell<Option<TrackedTextLayoutState>>>);

struct TrackedTextLayoutState {
    line: gpui::ShapedLine,
    line_height: Pixels,
    letter_spacing: Pixels,
    content_width: Pixels,
    color: gpui::Hsla,
    align: TextAlign,
    bounds: Option<Bounds<Pixels>>,
}

impl Element for TrackedText {
    type RequestLayoutState = TrackedTextLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        debug_assert!(
            !self.text.contains('\n'),
            "tracked text is intentionally a single-line primitive"
        );
        let layout = TrackedTextLayout::default();
        let layout_for_measure = layout.clone();
        let text = self.text.clone();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = text_style
            .line_height
            .to_pixels(font_size.into(), window.rem_size());
        let letter_spacing = font_size * self.letter_spacing_em;
        let mut run = text_style.to_run(text.len());
        disable_spacing_ligatures(&mut run);
        let color = text_style.color;
        let align = text_style.text_align;

        let layout_id = window.request_measured_layout(
            Default::default(),
            move |known_dimensions, _available_space, window, _cx| {
                let line = window.text_system().shape_line(
                    text.clone(),
                    font_size,
                    std::slice::from_ref(&run),
                    None,
                );
                let cluster_count = shaped_cluster_count(&line);
                let content_width = tracked_width(line.width, letter_spacing, cluster_count);
                let intrinsic = size(content_width.ceil(), line_height.ceil());
                let measured = Size {
                    width: known_dimensions.width.unwrap_or(intrinsic.width),
                    height: known_dimensions.height.unwrap_or(intrinsic.height),
                };
                layout_for_measure
                    .0
                    .borrow_mut()
                    .replace(TrackedTextLayoutState {
                        line,
                        line_height,
                        letter_spacing,
                        content_width,
                        color,
                        align,
                        bounds: None,
                    });
                measured
            },
        );
        (layout_id, layout)
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        layout
            .0
            .borrow_mut()
            .as_mut()
            .expect("tracked text must be measured before prepaint")
            .bounds = Some(bounds);
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let layout = layout.0.borrow();
        let state = layout
            .as_ref()
            .expect("tracked text must be measured before paint");
        let bounds = state
            .bounds
            .expect("tracked text must be prepainted before paint");
        let horizontal_offset = match state.align {
            TextAlign::Left => Pixels::ZERO,
            TextAlign::Center => (bounds.size.width - state.content_width) / 2.0,
            TextAlign::Right => bounds.size.width - state.content_width,
        };
        let padding_top = (state.line_height - state.line.ascent - state.line.descent) / 2.0;
        let baseline_y = bounds.origin.y + padding_top + state.line.ascent;
        let mut cluster_ordinal = 0usize;
        let mut previous_cluster = None;

        for run in &state.line.runs {
            for glyph in &run.glyphs {
                if previous_cluster.is_some_and(|index| index != glyph.index) {
                    cluster_ordinal += 1;
                }
                previous_cluster = Some(glyph.index);
                let origin = point(
                    bounds.origin.x
                        + horizontal_offset
                        + glyph.position.x
                        + state.letter_spacing * cluster_ordinal as f32,
                    baseline_y,
                );
                if glyph.is_emoji {
                    let _ = window.paint_emoji(origin, run.font_id, glyph.id, state.line.font_size);
                } else {
                    let _ = window.paint_glyph(
                        origin,
                        run.font_id,
                        glyph.id,
                        state.line.font_size,
                        state.color,
                    );
                }
            }
        }
    }
}

impl IntoElement for TrackedText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

fn shaped_cluster_count(line: &gpui::ShapedLine) -> usize {
    let mut count = 0;
    let mut previous = None;
    for glyph in line.runs.iter().flat_map(|run| &run.glyphs) {
        if previous != Some(glyph.index) {
            count += 1;
            previous = Some(glyph.index);
        }
    }
    count
}

fn tracked_width(base: Pixels, letter_spacing: Pixels, cluster_count: usize) -> Pixels {
    let gaps = cluster_count.saturating_sub(1) as f32;
    (base + letter_spacing * gaps).max(Pixels::ZERO)
}

fn disable_spacing_ligatures(run: &mut TextRun) {
    let mut features = run.font.features.tag_value_list().to_vec();
    for tag in ["liga", "clig", "dlig", "hlig", "calt"] {
        if let Some((_, value)) = features.iter_mut().find(|(feature, _)| feature == tag) {
            *value = 0;
        } else {
            features.push((tag.into(), 0));
        }
    }
    run.font.features = FontFeatures(Arc::new(features));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_changes_only_inter_cluster_gaps() {
        assert_eq!(
            tracked_width(gpui::px(100.0), gpui::px(2.0), 0),
            gpui::px(100.0)
        );
        assert_eq!(
            tracked_width(gpui::px(100.0), gpui::px(2.0), 1),
            gpui::px(100.0)
        );
        assert_eq!(
            tracked_width(gpui::px(100.0), gpui::px(2.0), 4),
            gpui::px(106.0)
        );
        assert_eq!(
            tracked_width(gpui::px(100.0), gpui::px(-2.0), 4),
            gpui::px(94.0)
        );
    }

    #[test]
    fn excessive_negative_tracking_never_reports_a_negative_width() {
        assert_eq!(
            tracked_width(gpui::px(4.0), gpui::px(-8.0), 3),
            Pixels::ZERO
        );
    }

    #[test]
    fn tracked_runs_disable_ligatures_that_would_hide_character_gaps() {
        let mut run = gpui::TextStyle::default().to_run(5);
        disable_spacing_ligatures(&mut run);
        for tag in ["liga", "clig", "dlig", "hlig", "calt"] {
            assert_eq!(
                run.font
                    .features
                    .tag_value_list()
                    .iter()
                    .find(|(feature, _)| feature == tag)
                    .map(|(_, value)| *value),
                Some(0)
            );
        }
    }
}
