use crate::{
    DisplayPoint, EditorSnapshot, RowExt,
    editor_settings::CursorAnimationSettings,
    movement::TextLayoutDetails,
    scroll::{ScrollOffset, ScrollPixelOffset},
};
use clock::ReplicaId;
use gpui::{
    Bounds, Hsla, Pixels, TextAlign, Window, fill, linear_color_stop, linear_gradient, point, px,
};
use std::time::{Duration, Instant};

const TRAIL_FADE_START: f32 = 0.0;
const TRAIL_FADE_END: f32 = 1.0;
const FULL_CIRCLE_DEGREES: f32 = 360.0;
const TRAIL_POLYGON_POINTS: usize = 6;

type TrailPolygon = [gpui::Point<Pixels>; TRAIL_POLYGON_POINTS];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CursorAnimationKey {
    pub(crate) replica_id: ReplicaId,
    pub(crate) selection_id: usize,
}

impl CursorAnimationKey {
    pub(crate) const LOCAL_NEWEST_SELECTION_ID: usize = usize::MAX;

    pub(crate) fn for_selection(
        replica_id: ReplicaId,
        selection_id: usize,
        is_newest: bool,
    ) -> Self {
        let selection_id = if replica_id == ReplicaId::LOCAL && is_newest {
            Self::LOCAL_NEWEST_SELECTION_ID
        } else {
            selection_id
        };

        Self {
            replica_id,
            selection_id,
        }
    }
}

pub(crate) fn cursor_origin_for_display_point(
    snapshot: &EditorSnapshot,
    cursor_position: DisplayPoint,
    text_layout_details: &TextLayoutDetails,
    text_align: TextAlign,
    content_width: Pixels,
    scroll_position: gpui::Point<ScrollOffset>,
    scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
    line_height: Pixels,
) -> gpui::Point<Pixels> {
    let row_layout = snapshot
        .display_snapshot
        .layout_row(cursor_position.row(), text_layout_details);
    let alignment_offset = match text_align {
        TextAlign::Left => Pixels::ZERO,
        TextAlign::Center => (content_width - row_layout.width) / 2.0,
        TextAlign::Right => content_width - row_layout.width,
    };
    let x = row_layout.x_for_index(cursor_position.column() as usize) + alignment_offset
        - scroll_pixel_position.x.into();
    let y = ((cursor_position.row().as_f64() - scroll_position.y)
        * ScrollPixelOffset::from(line_height))
    .into();

    point(x, y)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CursorAnimationState {
    logical_position: DisplayPoint,
    start_origin: gpui::Point<Pixels>,
    target_origin: gpui::Point<Pixels>,
    started_at: Instant,
    duration: Duration,
    draw_trail: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorAnimationFrame {
    pub(crate) origin: gpui::Point<Pixels>,
    pub(crate) trail_origin: Option<gpui::Point<Pixels>>,
    pub(crate) is_animating: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CursorShapeTrail {
    bounds: Bounds<Pixels>,
    gradient_angle: f32,
    polygon: Option<TrailPolygon>,
}

impl CursorAnimationState {
    fn new(
        logical_position: DisplayPoint,
        origin: gpui::Point<Pixels>,
        started_at: Instant,
        duration: Duration,
    ) -> Self {
        Self {
            logical_position,
            start_origin: origin,
            target_origin: origin,
            started_at,
            duration,
            draw_trail: false,
        }
    }

    pub(crate) fn settled(
        logical_position: DisplayPoint,
        origin: gpui::Point<Pixels>,
        now: Instant,
        duration: Duration,
    ) -> Self {
        let started_at = now.checked_sub(duration).unwrap_or(now);
        Self::new(logical_position, origin, started_at, duration)
    }

    fn frame(&self, now: Instant, movement_enabled: bool) -> CursorAnimationFrame {
        let animation_delta = cursor_animation_delta(self.started_at, self.duration, now);
        let eased_delta = ease_cursor_delta(animation_delta);
        let trail_origin = self
            .draw_trail
            .then(|| interpolate_point(self.start_origin, self.target_origin, eased_delta));

        CursorAnimationFrame {
            origin: if movement_enabled && !self.draw_trail {
                interpolate_point(self.start_origin, self.target_origin, eased_delta)
            } else {
                self.target_origin
            },
            trail_origin,
            is_animating: animation_delta < 1.0,
        }
    }

    pub(crate) fn update(
        state: Option<Self>,
        logical_position: DisplayPoint,
        target_origin: gpui::Point<Pixels>,
        now: Instant,
        duration: Duration,
        settings: CursorAnimationSettings,
        trail_enabled: bool,
    ) -> (CursorAnimationFrame, Self) {
        let Some(mut state) = state else {
            return (
                CursorAnimationFrame {
                    origin: target_origin,
                    trail_origin: None,
                    is_animating: false,
                },
                Self::settled(logical_position, target_origin, now, duration),
            );
        };

        if state.logical_position != logical_position {
            let current = state.frame(now, settings.movement);
            let start_origin = if state.draw_trail {
                current.trail_origin.unwrap_or(current.origin)
            } else if settings.movement || trail_enabled {
                current.origin
            } else {
                target_origin
            };
            state = Self {
                logical_position,
                start_origin,
                target_origin,
                started_at: now,
                duration,
                draw_trail: trail_enabled,
            };
        } else if state.target_origin != target_origin {
            state = Self::settled(logical_position, target_origin, now, duration);
        } else if state.duration != duration {
            state.duration = duration;
        }

        let frame = state.frame(now, settings.movement);
        if frame.is_animating {
            (frame, state)
        } else {
            (
                CursorAnimationFrame {
                    origin: target_origin,
                    trail_origin: None,
                    is_animating: false,
                },
                Self::settled(logical_position, target_origin, now, duration),
            )
        }
    }
}

impl CursorShapeTrail {
    pub(crate) fn between(
        trail_bounds: Bounds<Pixels>,
        target_bounds: Bounds<Pixels>,
        trail_origin: gpui::Point<Pixels>,
        target_origin: gpui::Point<Pixels>,
    ) -> Self {
        let bounds = union_bounds(trail_bounds, target_bounds);
        let polygon = is_diagonal_motion(trail_origin, target_origin).then(|| {
            cursor_trail_polygon_between(trail_bounds, target_bounds, trail_origin, target_origin)
        });

        Self {
            bounds,
            gradient_angle: cursor_trail_gradient_angle(trail_origin, target_origin),
            polygon,
        }
    }

    pub(crate) fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    pub(crate) fn paint(
        &self,
        origin: gpui::Point<Pixels>,
        solid_bounds: Bounds<Pixels>,
        color: Hsla,
        window: &mut Window,
    ) {
        let bounds = window.pixel_snap_bounds(Bounds {
            origin: self.bounds.origin + origin,
            size: self.bounds.size,
        });
        let trail_background = linear_gradient(
            self.gradient_angle,
            linear_color_stop(color.opacity(0.0), TRAIL_FADE_START),
            linear_color_stop(color, TRAIL_FADE_END),
        );

        if let Some(polygon) = &self.polygon {
            let polygon: TrailPolygon = std::array::from_fn(|index| polygon[index] + origin);
            let mut builder = gpui::PathBuilder::fill();
            builder.add_polygon(&polygon, true);
            if let Ok(path) = builder.build() {
                window.paint_path(path, trail_background);
            } else {
                window.paint_quad(fill(bounds, trail_background));
            }
        } else {
            window.paint_quad(fill(bounds, trail_background));
        }

        window.paint_quad(fill(solid_bounds, color));
    }
}

fn cursor_animation_delta(started_at: Instant, duration: Duration, now: Instant) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }

    (now.saturating_duration_since(started_at).as_secs_f32() / duration.as_secs_f32())
        .clamp(0.0, 1.0)
}

fn ease_cursor_delta(delta: f32) -> f32 {
    let delta = delta.clamp(0.0, 1.0);
    1.0 - (1.0 - delta).powi(3)
}

fn interpolate_point(
    start: gpui::Point<Pixels>,
    end: gpui::Point<Pixels>,
    delta: f32,
) -> gpui::Point<Pixels> {
    point(
        interpolate_pixels(start.x, end.x, delta),
        interpolate_pixels(start.y, end.y, delta),
    )
}

fn interpolate_pixels(start: Pixels, end: Pixels, delta: f32) -> Pixels {
    start + (end - start) * delta
}

fn union_bounds(first: Bounds<Pixels>, second: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(
            first.left().min(second.left()),
            first.top().min(second.top()),
        ),
        point(
            first.right().max(second.right()),
            first.bottom().max(second.bottom()),
        ),
    )
}

fn cursor_trail_gradient_angle(
    trail_origin: gpui::Point<Pixels>,
    target_origin: gpui::Point<Pixels>,
) -> f32 {
    let delta_x = target_origin.x - trail_origin.x;
    let delta_y = target_origin.y - trail_origin.y;
    if delta_x == Pixels::ZERO && delta_y == Pixels::ZERO {
        return 0.0;
    }

    let delta_x = delta_x / px(1.0);
    let delta_y = delta_y / px(1.0);
    let mut angle = delta_x.atan2(-delta_y).to_degrees();
    if angle < 0.0 {
        angle += FULL_CIRCLE_DEGREES;
    }
    angle
}

fn is_diagonal_motion(
    trail_origin: gpui::Point<Pixels>,
    target_origin: gpui::Point<Pixels>,
) -> bool {
    trail_origin.x != target_origin.x && trail_origin.y != target_origin.y
}

fn cursor_trail_polygon_between(
    trail_bounds: Bounds<Pixels>,
    target_bounds: Bounds<Pixels>,
    trail_origin: gpui::Point<Pixels>,
    target_origin: gpui::Point<Pixels>,
) -> TrailPolygon {
    let trail = RectCorners::from_bounds(trail_bounds);
    let target = RectCorners::from_bounds(target_bounds);

    match (
        target_origin.x > trail_origin.x,
        target_origin.y > trail_origin.y,
    ) {
        (true, true) => [
            trail.top_left,
            trail.top_right,
            target.top_right,
            target.bottom_right,
            target.bottom_left,
            trail.bottom_left,
        ],
        (true, false) => [
            trail.bottom_left,
            trail.top_left,
            target.top_left,
            target.top_right,
            target.bottom_right,
            trail.bottom_right,
        ],
        (false, true) => [
            trail.top_right,
            trail.bottom_right,
            target.bottom_right,
            target.bottom_left,
            target.top_left,
            trail.top_left,
        ],
        (false, false) => [
            trail.bottom_right,
            trail.bottom_left,
            target.bottom_left,
            target.top_left,
            target.top_right,
            trail.top_right,
        ],
    }
}

#[derive(Clone, Copy)]
struct RectCorners {
    top_left: gpui::Point<Pixels>,
    top_right: gpui::Point<Pixels>,
    bottom_right: gpui::Point<Pixels>,
    bottom_left: gpui::Point<Pixels>,
}

impl RectCorners {
    fn from_bounds(bounds: Bounds<Pixels>) -> Self {
        Self {
            top_left: bounds.origin,
            top_right: point(bounds.right(), bounds.top()),
            bottom_right: point(bounds.right(), bounds.bottom()),
            bottom_left: point(bounds.left(), bounds.bottom()),
        }
    }
}

#[cfg(test)]
#[path = "cursor_animation_tests.rs"]
mod tests;
