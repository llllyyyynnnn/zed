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
fn test_cursor_animation_delta_and_easing() {
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
    assert_eq!(ease_cursor_delta(0.0), 0.0);
    assert_eq!(ease_cursor_delta(0.5), 0.875);
    assert_eq!(ease_cursor_delta(1.0), 1.0);
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
    assert_eq!(
        diagonal_trail.polygon.as_ref().map(|polygon| polygon.len()),
        Some(TRAIL_POLYGON_POINTS)
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
        (cursor_trail_gradient_angle(point(px(0.0), px(0.0)), point(px(20.0), px(20.0))) - 135.0)
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
    let state =
        CursorAnimationState::settled(old_position, point(px(0.0), px(0.0)), started_at, duration);

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
    let state =
        CursorAnimationState::settled(old_position, point(px(0.0), px(0.0)), started_at, duration);

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
