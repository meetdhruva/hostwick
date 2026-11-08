//! Zoom: the terminal font size of a tab (Ctrl+= / Ctrl+- / Ctrl+0 and
//! Ctrl+wheel) and the UI scale of the whole app (Settings → Appearance).
//!
//! Pure arithmetic, so the limits are easy to test. A tab's zoom is a
//! percentage of the font size in Settings; the UI scale is a percentage of
//! every size the app draws.

use std::sync::atomic::{AtomicU32, Ordering};

use gpui_kit::Pixels;

/// A tab's zoom, in percent of the settings font size.
pub const MIN_ZOOM: u32 = 50;
pub const MAX_ZOOM: u32 = 300;
pub const DEFAULT_ZOOM: u32 = 100;
const ZOOM_STEP: u32 = 10;

/// Smallest and largest font size a zoomed terminal ever uses, in pixels.
pub const MIN_FONT: f32 = 6.0;
pub const MAX_FONT: f32 = 72.0;

/// The UI scale range offered in Settings, in percent.
pub const MIN_UI_SCALE: u32 = 90;
pub const MAX_UI_SCALE: u32 = 150;
pub const UI_SCALE_STEP: u32 = 10;

pub fn clamp_zoom(pct: u32) -> u32 {
    pct.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// One step bigger (`dir > 0`) or smaller (`dir < 0`), staying on multiples
/// of the step and inside the limits. 0 resets to 100 %.
pub fn step_zoom(current: u32, dir: i32) -> u32 {
    let cur = clamp_zoom(current);
    match dir {
        0 => DEFAULT_ZOOM,
        d if d > 0 => clamp_zoom((cur / ZOOM_STEP + 1) * ZOOM_STEP),
        _ => clamp_zoom(cur.div_ceil(ZOOM_STEP).saturating_sub(1) * ZOOM_STEP),
    }
}

/// The font size for a zoom, never outside [`MIN_FONT`]..=[`MAX_FONT`].
pub fn zoomed_font(base: f32, pct: u32) -> f32 {
    let base = if base.is_finite() { base } else { 14.0 };
    (base * clamp_zoom(pct) as f32 / 100.0).clamp(MIN_FONT, MAX_FONT)
}

/// Mouse-wheel notches → zoom steps: positive scrolls up (bigger). Zero
/// when the wheel did not move.
pub fn wheel_direction(delta_y: f32) -> i32 {
    if delta_y > 0.0 {
        1
    } else if delta_y < 0.0 {
        -1
    } else {
        0
    }
}

pub fn clamp_ui_scale(pct: u32) -> u32 {
    pct.clamp(MIN_UI_SCALE, MAX_UI_SCALE)
}

// ---- UI scale -----------------------------------------------------------------

static UI_SCALE: AtomicU32 = AtomicU32::new(100);

/// Set the UI scale (percent), clamped to the supported range.
pub fn set_ui_scale(pct: u32) {
    UI_SCALE.store(clamp_ui_scale(pct), Ordering::Relaxed);
}

/// `px` that follows the UI scale: every fixed size the app draws goes
/// through this, so one setting scales text, rows, bars and dialogs
/// together. (Component-library sizes follow the window's rem size, which
/// the root view keeps in step.)
pub fn px(v: f32) -> Pixels {
    gpui_kit::px(v * UI_SCALE.load(Ordering::Relaxed) as f32 / 100.0)
}

/// The window's rem size at the current UI scale (16 px at 100 %).
pub fn rem_px() -> f32 {
    16.0 * UI_SCALE.load(Ordering::Relaxed) as f32 / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_stay_on_the_grid_and_inside_the_limits() {
        assert_eq!(step_zoom(100, 1), 110);
        assert_eq!(step_zoom(100, -1), 90);
        assert_eq!(step_zoom(110, 0), 100);
        // Off-grid values snap to the next grid line in the chosen direction.
        assert_eq!(step_zoom(103, 1), 110);
        assert_eq!(step_zoom(103, -1), 100);
        // Limits.
        assert_eq!(step_zoom(MAX_ZOOM, 1), MAX_ZOOM);
        assert_eq!(step_zoom(MIN_ZOOM, -1), MIN_ZOOM);
        assert_eq!(step_zoom(10_000, -1), 290);
        assert_eq!(step_zoom(1, 1), 60);
        // Walking all the way up and down never leaves the range.
        let mut z = 100;
        for _ in 0..100 {
            z = step_zoom(z, 1);
            assert!((MIN_ZOOM..=MAX_ZOOM).contains(&z));
        }
        assert_eq!(z, MAX_ZOOM);
        for _ in 0..100 {
            z = step_zoom(z, -1);
        }
        assert_eq!(z, MIN_ZOOM);
    }

    #[test]
    fn zoomed_fonts_are_clamped() {
        assert_eq!(zoomed_font(14.0, 100), 14.0);
        assert!((zoomed_font(14.0, 120) - 16.8).abs() < 1e-4);
        assert_eq!(zoomed_font(48.0, 300), MAX_FONT);
        assert_eq!(zoomed_font(6.0, 50), MIN_FONT);
        assert_eq!(zoomed_font(f32::NAN, 100), 14.0);
        // Out-of-range zooms from a hand-edited file are clamped first.
        assert_eq!(zoomed_font(10.0, 5000), 30.0);
    }

    #[test]
    fn wheel_and_ui_scale() {
        assert_eq!(wheel_direction(3.0), 1);
        assert_eq!(wheel_direction(-0.1), -1);
        assert_eq!(wheel_direction(0.0), 0);
        assert_eq!(clamp_ui_scale(50), 90);
        assert_eq!(clamp_ui_scale(100), 100);
        assert_eq!(clamp_ui_scale(400), 150);
    }
}
