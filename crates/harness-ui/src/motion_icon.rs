use crate::theme::{Theme, web_ease_out};
use crate::zoom::px;
use gpui::{
    App, Bounds, Element, ElementId, FontWeight, GlobalElementId, Hitbox, InspectorElementId,
    InteractiveElement, Interactivity, IntoElement, LayoutId, Pixels, SharedString,
    StyleRefinement, Styled, TransformationMatrix, Window,
};
use std::time::{Duration, Instant};

const REST_WEIGHT: f32 = 1.0;
const HOVER_WEIGHT: f32 = 2.0;
const HOVER_ROTATION_DEGREES: f32 = 3.0;

pub(crate) fn motion_icon(
    id: impl Into<ElementId>,
    path: impl Into<SharedString>,
    size: f32,
    hover_group: impl Into<SharedString>,
    theme: Theme,
) -> MotionIcon {
    MotionIcon::new(id, path, theme.motion.fast, theme.reduced_motion)
        .size(px(size))
        .font_weight(FontWeight(REST_WEIGHT))
        .hover(|style| style.font_weight(FontWeight(HOVER_WEIGHT)))
        .group_hover(hover_group, |style| {
            style.font_weight(FontWeight(HOVER_WEIGHT))
        })
}

pub(crate) struct MotionIcon {
    interactivity: Interactivity,
    path: SharedString,
    duration: Duration,
    reduced_motion: bool,
}

impl MotionIcon {
    fn new(
        id: impl Into<ElementId>,
        path: impl Into<SharedString>,
        duration: Duration,
        reduced_motion: bool,
    ) -> Self {
        let mut interactivity = Interactivity::new();
        interactivity.element_id = Some(id.into());
        Self {
            interactivity,
            path: path.into(),
            duration,
            reduced_motion,
        }
    }
}

#[derive(Clone, Copy)]
struct HoverMotionState {
    from: f32,
    target: f32,
    started: Instant,
    duration: Duration,
}

impl HoverMotionState {
    fn sample(self, now: Instant) -> (f32, bool) {
        if self.duration.is_zero() {
            return (self.target, false);
        }
        let progress =
            now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32();
        if progress >= 1.0 {
            return (self.target, false);
        }
        let eased = web_ease_out(progress.clamp(0.0, 1.0));
        (self.from + (self.target - self.from) * eased, true)
    }
}

fn hover_motion_progress(
    state: Option<HoverMotionState>,
    hovered: bool,
    duration: Duration,
    now: Instant,
) -> (f32, HoverMotionState, bool) {
    let target = f32::from(hovered);
    let Some(mut state) = state else {
        return (
            target,
            HoverMotionState {
                from: target,
                target,
                started: now,
                duration: Duration::ZERO,
            },
            false,
        );
    };
    let (current, _) = state.sample(now);
    if (state.target - target).abs() >= f32::EPSILON {
        state = HoverMotionState {
            from: current,
            target,
            started: now,
            duration: duration.mul_f32((target - current).abs()),
        };
    }
    let (progress, animating) = state.sample(now);
    (progress, state, animating)
}

fn rotation_matrix(
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    progress: f32,
) -> TransformationMatrix {
    let angle = (HOVER_ROTATION_DEGREES * progress).to_radians();
    let cosine = angle.cos();
    let sine = angle.sin();
    let center = bounds.center();
    let center_x = f32::from(center.x) * scale_factor;
    let center_y = f32::from(center.y) * scale_factor;
    TransformationMatrix {
        rotation_scale: [[cosine, -sine], [sine, cosine]],
        translation: [
            center_x - cosine * center_x + sine * center_y,
            center_y - sine * center_x - cosine * center_y,
        ],
    }
}

impl Element for MotionIcon {
    type RequestLayoutState = ();
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| window.request_layout(style, None, cx),
        );
        (layout, ())
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            bounds.size,
            window,
            cx,
            |_, _, hitbox, _, _| hitbox,
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let path = self.path.clone();
        let duration = self.duration;
        let reduced_motion = self.reduced_motion;
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            move |style, window, cx| {
                let hovered =
                    !reduced_motion && style.text.font_weight == Some(FontWeight(HOVER_WEIGHT));
                let progress = window.with_element_state(
                    global_id.expect("motion icon always has an element id"),
                    |state, window| {
                        let (progress, state, animating) =
                            hover_motion_progress(state, hovered, duration, Instant::now());
                        if animating {
                            window.request_animation_frame();
                        }
                        (progress, state)
                    },
                );
                let color = window.text_style().color;
                let transformation = rotation_matrix(bounds, window.scale_factor(), progress);
                let _ = window.paint_svg(bounds, path, transformation, color, cx);
            },
        );
    }
}

impl IntoElement for MotionIcon {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for MotionIcon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for MotionIcon {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_motion_mounts_still_and_reverses_from_its_current_angle() {
        let duration = Duration::from_millis(180);
        let start = Instant::now();
        let (mounted, state, animating) = hover_motion_progress(None, false, duration, start);
        assert_eq!(mounted, 0.0);
        assert!(!animating);

        let (_, state, animating) = hover_motion_progress(
            Some(state),
            true,
            duration,
            start + Duration::from_millis(1),
        );
        assert!(animating);
        let (entered, state, _) = hover_motion_progress(
            Some(state),
            true,
            duration,
            start + Duration::from_millis(91),
        );
        assert!(entered > 0.5 && entered < 1.0);

        let (reversed, _, animating) = hover_motion_progress(
            Some(state),
            false,
            duration,
            start + Duration::from_millis(91),
        );
        assert!((reversed - entered).abs() < 0.001);
        assert!(animating);
    }
}
