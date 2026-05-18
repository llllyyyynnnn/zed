use crate::{DisplayPoint, editor_settings::CursorAnimationSettings};
use gpui::{Bounds, Hsla, Pixels, Window, fill, linear_color_stop, linear_gradient, point, px};
use smallvec::{SmallVec, smallvec};
use std::{
    cmp::Ordering,
    time::{Duration, Instant},
};

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
    polygon: Option<SmallVec<[gpui::Point<Pixels>; 8]>>,
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

    fn settled(
        logical_position: DisplayPoint,
        origin: gpui::Point<Pixels>,
        now: Instant,
        duration: Duration,
    ) -> Self {
        let started_at = now.checked_sub(duration).unwrap_or(now);
        Self::new(logical_position, origin, started_at, duration)
    }

    fn frame(&self, now: Instant, movement_enabled: bool) -> CursorAnimationFrame {
        let delta = cursor_animation_delta(self.started_at, self.duration, now);
        let trail_origin = self.draw_trail.then(|| {
            interpolate_point(
                self.start_origin,
                self.target_origin,
                cursor_trail_delta(delta),
            )
        });
        CursorAnimationFrame {
            origin: if movement_enabled && !self.draw_trail {
                interpolate_point(
                    self.start_origin,
                    self.target_origin,
                    cursor_motion_delta(delta),
                )
            } else {
                self.target_origin
            },
            trail_origin,
            is_animating: delta < 1.0,
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
        let polygon = cursor_trail_is_diagonal(trail_origin, target_origin)
            .then(|| cursor_trail_polygon_between(trail_bounds, target_bounds));

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
        bounds: Bounds<Pixels>,
        solid_bounds: Bounds<Pixels>,
        color: Hsla,
        window: &mut Window,
    ) {
        let trail_background = linear_gradient(
            self.gradient_angle,
            linear_color_stop(color.opacity(0.0), 0.0),
            linear_color_stop(color, 1.0),
        );
        if let Some(polygon) = &self.polygon {
            let mut builder = gpui::PathBuilder::fill();
            let polygon = polygon
                .iter()
                .map(|point| *point + origin)
                .collect::<SmallVec<[gpui::Point<Pixels>; 8]>>();
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

fn cursor_motion_delta(delta: f32) -> f32 {
    cursor_eased_delta(delta)
}

fn cursor_trail_delta(delta: f32) -> f32 {
    cursor_eased_delta(delta)
}

fn cursor_eased_delta(delta: f32) -> f32 {
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
        angle += 360.0;
    }
    angle
}

fn cursor_trail_is_diagonal(
    trail_origin: gpui::Point<Pixels>,
    target_origin: gpui::Point<Pixels>,
) -> bool {
    trail_origin.x != target_origin.x && trail_origin.y != target_origin.y
}

fn cursor_trail_polygon_between(
    trail_bounds: Bounds<Pixels>,
    target_bounds: Bounds<Pixels>,
) -> SmallVec<[gpui::Point<Pixels>; 8]> {
    let mut points: SmallVec<[gpui::Point<Pixels>; 8]> = smallvec![
        trail_bounds.origin,
        point(trail_bounds.right(), trail_bounds.top()),
        point(trail_bounds.right(), trail_bounds.bottom()),
        point(trail_bounds.left(), trail_bounds.bottom()),
        target_bounds.origin,
        point(target_bounds.right(), target_bounds.top()),
        point(target_bounds.right(), target_bounds.bottom()),
        point(target_bounds.left(), target_bounds.bottom()),
    ];
    points.sort_by(|first, second| {
        first
            .x
            .partial_cmp(&second.x)
            .unwrap_or(Ordering::Equal)
            .then_with(|| first.y.partial_cmp(&second.y).unwrap_or(Ordering::Equal))
    });
    points.dedup_by(|first, second| first.x == second.x && first.y == second.y);

    if points.len() <= 3 {
        return points;
    }

    let mut lower = SmallVec::<[gpui::Point<Pixels>; 8]>::new();
    for point in points.iter().copied() {
        while lower.len() >= 2
            && cursor_trail_polygon_cross(lower[lower.len() - 2], lower[lower.len() - 1], point)
                <= 0.0
        {
            lower.pop();
        }
        lower.push(point);
    }

    let mut upper = SmallVec::<[gpui::Point<Pixels>; 8]>::new();
    for point in points.iter().rev().copied() {
        while upper.len() >= 2
            && cursor_trail_polygon_cross(upper[upper.len() - 2], upper[upper.len() - 1], point)
                <= 0.0
        {
            upper.pop();
        }
        upper.push(point);
    }

    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn cursor_trail_polygon_cross(
    origin: gpui::Point<Pixels>,
    first: gpui::Point<Pixels>,
    second: gpui::Point<Pixels>,
) -> f32 {
    let first_x = (first.x - origin.x) / px(1.0);
    let first_y = (first.y - origin.y) / px(1.0);
    let second_x = (second.x - origin.x) / px(1.0);
    let second_y = (second.y - origin.y) / px(1.0);
    first_x * second_y - first_y * second_x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DisplayRow, editor_settings::CursorAnimationSettings};
    use gpui::size;

    fn assert_pixels_near(actual: Pixels, expected: Pixels) {
        assert!(
            ((actual - expected) / px(1.0)).abs() < 0.001,
            "expected {actual:?} to be near {expected:?}"
        );
    }

    fn cursor_animation_test_settings() -> CursorAnimationSettings {
        CursorAnimationSettings {
            enabled: true,
            movement: true,
            shape: true,
            duration_ms: 100,
        }
    }

    fn bar_bounds(origin: gpui::Point<Pixels>, line_height: Pixels) -> Bounds<Pixels> {
        Bounds {
            origin,
            size: size(px(2.0), line_height),
        }
    }

    #[test]
    fn test_cursor_animation_delta_and_trail_delta() {
        let started_at = Instant::now();
        let duration = Duration::from_millis(100);

        assert_eq!(
            cursor_animation_delta(started_at, Duration::ZERO, started_at),
            1.0
        );
        assert_eq!(
            cursor_animation_delta(started_at, duration, started_at + Duration::from_millis(50)),
            0.5
        );
        assert_eq!(cursor_motion_delta(0.5), 0.875);
        assert_eq!(cursor_trail_delta(0.0), 0.0);
        assert_eq!(cursor_trail_delta(0.5), 0.875);
        assert_eq!(cursor_trail_delta(1.0), 1.0);
    }

    #[test]
    fn test_cursor_animation_restarts_from_current_origin() {
        let started_at = Instant::now();
        let duration = Duration::from_millis(100);
        let mid_animation = started_at + Duration::from_millis(50);
        let old_position = DisplayPoint::new(DisplayRow(0), 0);
        let new_position = DisplayPoint::new(DisplayRow(0), 1);
        let state = CursorAnimationState {
            logical_position: old_position,
            start_origin: point(px(0.0), px(0.0)),
            target_origin: point(px(100.0), px(0.0)),
            started_at,
            duration,
            draw_trail: true,
        };

        let (frame, state) = CursorAnimationState::update(
            Some(state),
            new_position,
            point(px(200.0), px(0.0)),
            mid_animation,
            duration,
            cursor_animation_test_settings(),
            true,
        );

        assert!(frame.is_animating);
        assert_pixels_near(frame.origin.x, px(200.0));
        assert_eq!(frame.trail_origin, Some(point(px(87.5), px(0.0))));
        assert_pixels_near(state.start_origin.x, px(87.5));
        assert_pixels_near(state.target_origin.x, px(200.0));
        assert_eq!(state.logical_position, new_position);
    }

    #[test]
    fn test_cursor_animation_snaps_when_layout_moves_without_cursor_change() {
        let started_at = Instant::now();
        let duration = Duration::from_millis(100);
        let mid_animation = started_at + Duration::from_millis(50);
        let position = DisplayPoint::new(DisplayRow(0), 0);
        let state = CursorAnimationState {
            logical_position: position,
            start_origin: point(px(0.0), px(0.0)),
            target_origin: point(px(100.0), px(0.0)),
            started_at,
            duration,
            draw_trail: true,
        };

        let (frame, state) = CursorAnimationState::update(
            Some(state),
            position,
            point(px(60.0), px(0.0)),
            mid_animation,
            duration,
            cursor_animation_test_settings(),
            true,
        );

        assert!(!frame.is_animating);
        assert_pixels_near(frame.origin.x, px(60.0));
        assert_eq!(frame.trail_origin, None);
        assert_pixels_near(state.target_origin.x, px(60.0));
    }

    #[test]
    fn test_cursor_trail_bounds_do_not_overshoot_endpoints() {
        let horizontal_bounds = CursorShapeTrail::between(
            bar_bounds(point(px(100.0), px(0.0)), px(20.0)),
            bar_bounds(point(px(80.0), px(0.0)), px(20.0)),
            point(px(100.0), px(0.0)),
            point(px(80.0), px(0.0)),
        )
        .bounds;

        assert_pixels_near(horizontal_bounds.left(), px(80.0));
        assert_pixels_near(horizontal_bounds.right(), px(102.0));
        assert_pixels_near(horizontal_bounds.top(), px(0.0));
        assert_pixels_near(horizontal_bounds.bottom(), px(20.0));

        let vertical_bounds = CursorShapeTrail::between(
            bar_bounds(point(px(0.0), px(0.0)), px(20.0)),
            bar_bounds(point(px(0.0), px(100.0)), px(20.0)),
            point(px(0.0), px(0.0)),
            point(px(0.0), px(100.0)),
        )
        .bounds;
        assert_pixels_near(vertical_bounds.left(), px(0.0));
        assert_pixels_near(vertical_bounds.right(), px(2.0));
        assert_pixels_near(vertical_bounds.top(), px(0.0));
        assert_pixels_near(vertical_bounds.bottom(), px(120.0));
    }

    #[test]
    fn test_cursor_trail_uses_polygon_for_diagonal_only() {
        let horizontal_trail = CursorShapeTrail::between(
            bar_bounds(point(px(0.0), px(0.0)), px(20.0)),
            bar_bounds(point(px(100.0), px(0.0)), px(20.0)),
            point(px(0.0), px(0.0)),
            point(px(100.0), px(0.0)),
        );
        assert!(horizontal_trail.polygon.is_none());

        let diagonal_trail = CursorShapeTrail::between(
            bar_bounds(point(px(100.0), px(0.0)), px(20.0)),
            bar_bounds(point(px(0.0), px(100.0)), px(20.0)),
            point(px(100.0), px(0.0)),
            point(px(0.0), px(100.0)),
        );
        assert!(
            diagonal_trail
                .polygon
                .as_ref()
                .is_some_and(|polygon| polygon.len() >= 4)
        );
        assert_pixels_near(diagonal_trail.bounds.left(), px(0.0));
        assert_pixels_near(diagonal_trail.bounds.right(), px(102.0));
        assert_pixels_near(diagonal_trail.bounds.top(), px(0.0));
        assert_pixels_near(diagonal_trail.bounds.bottom(), px(120.0));
    }

    #[test]
    fn test_cursor_trail_gradient_points_from_tail_to_current_cursor() {
        assert_eq!(
            cursor_trail_gradient_angle(point(px(0.0), px(0.0)), point(px(20.0), px(0.0))),
            90.0
        );
        assert_eq!(
            cursor_trail_gradient_angle(point(px(20.0), px(0.0)), point(px(0.0), px(0.0))),
            270.0
        );
        assert_eq!(
            cursor_trail_gradient_angle(point(px(0.0), px(0.0)), point(px(0.0), px(20.0))),
            180.0
        );
        assert_eq!(
            cursor_trail_gradient_angle(point(px(0.0), px(20.0)), point(px(0.0), px(0.0))),
            0.0
        );
        assert!(
            (cursor_trail_gradient_angle(point(px(0.0), px(0.0)), point(px(20.0), px(20.0)))
                - 135.0)
                .abs()
                < 0.001
        );
    }

    #[test]
    fn test_vertical_cursor_shape_animation_changes_height_not_width() {
        let started_at = Instant::now();
        let duration = Duration::from_millis(100);
        let old_position = DisplayPoint::new(DisplayRow(0), 0);
        let new_position = DisplayPoint::new(DisplayRow(1), 0);
        let state = CursorAnimationState::settled(
            old_position,
            point(px(0.0), px(0.0)),
            started_at,
            duration,
        );

        let (frame, state) = CursorAnimationState::update(
            Some(state),
            new_position,
            point(px(0.0), px(100.0)),
            started_at,
            duration,
            cursor_animation_test_settings(),
            true,
        );

        assert!(frame.is_animating);
        assert_pixels_near(frame.origin.x, px(0.0));
        assert_pixels_near(frame.origin.y, px(100.0));
        assert_eq!(frame.trail_origin, Some(point(px(0.0), px(0.0))));

        let frame = state.frame(
            started_at + Duration::from_millis(50),
            cursor_animation_test_settings().movement,
        );
        assert_eq!(frame.trail_origin, Some(point(px(0.0), px(87.5))));
    }

    #[test]
    fn test_diagonal_cursor_shape_animation_draws_trail() {
        let started_at = Instant::now();
        let duration = Duration::from_millis(100);
        let old_position = DisplayPoint::new(DisplayRow(0), 10);
        let new_position = DisplayPoint::new(DisplayRow(1), 0);
        let state = CursorAnimationState::settled(
            old_position,
            point(px(100.0), px(0.0)),
            started_at,
            duration,
        );

        let (frame, state) = CursorAnimationState::update(
            Some(state),
            new_position,
            point(px(0.0), px(100.0)),
            started_at,
            duration,
            cursor_animation_test_settings(),
            true,
        );

        assert!(frame.is_animating);
        assert_eq!(frame.trail_origin, Some(point(px(100.0), px(0.0))));
        assert_pixels_near(frame.origin.x, px(0.0));
        assert_pixels_near(frame.origin.y, px(100.0));
        assert!(state.draw_trail);

        let frame = state.frame(
            started_at + Duration::from_millis(50),
            cursor_animation_test_settings().movement,
        );
        assert_eq!(frame.trail_origin, Some(point(px(12.5), px(87.5))));
        assert_pixels_near(frame.origin.x, px(0.0));
        assert_pixels_near(frame.origin.y, px(100.0));
    }

    #[test]
    fn test_tall_cursor_shape_animation_draws_trail() {
        let started_at = Instant::now();
        let duration = Duration::from_millis(100);
        let old_position = DisplayPoint::new(DisplayRow(0), 0);
        let new_position = DisplayPoint::new(DisplayRow(3), 0);
        let state = CursorAnimationState::settled(
            old_position,
            point(px(0.0), px(0.0)),
            started_at,
            duration,
        );

        let (frame, state) = CursorAnimationState::update(
            Some(state),
            new_position,
            point(px(0.0), px(300.0)),
            started_at,
            duration,
            cursor_animation_test_settings(),
            true,
        );

        assert!(frame.is_animating);
        assert_eq!(frame.trail_origin, Some(point(px(0.0), px(0.0))));
        assert!(state.draw_trail);
    }

    #[test]
    fn test_cursor_shape_animation_can_run_without_movement() {
        let started_at = Instant::now();
        let duration = Duration::from_millis(100);
        let old_position = DisplayPoint::new(DisplayRow(0), 0);
        let new_position = DisplayPoint::new(DisplayRow(0), 1);
        let state = CursorAnimationState {
            logical_position: old_position,
            start_origin: point(px(0.0), px(0.0)),
            target_origin: point(px(100.0), px(0.0)),
            started_at,
            duration,
            draw_trail: false,
        };
        let settings = CursorAnimationSettings {
            movement: false,
            ..cursor_animation_test_settings()
        };

        let (frame, state) = CursorAnimationState::update(
            Some(state),
            new_position,
            point(px(200.0), px(0.0)),
            started_at + Duration::from_millis(50),
            duration,
            settings,
            true,
        );

        assert!(frame.is_animating);
        assert_pixels_near(frame.origin.x, px(200.0));
        assert_eq!(frame.trail_origin, Some(point(px(100.0), px(0.0))));

        let frame = state.frame(started_at + Duration::from_millis(100), settings.movement);
        assert_eq!(frame.trail_origin, Some(point(px(187.5), px(0.0))));
    }
}
