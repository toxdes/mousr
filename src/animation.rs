use std::time::{Duration, Instant};

use crate::grid::{Rect, View};

pub(crate) struct LensAnimation {
    pub(crate) from: Rect,
    pub(crate) target: View,
    pub(crate) started: Instant,
    pub(crate) duration: Duration,
    pub(crate) frame_interval: Duration,
}

impl LensAnimation {
    pub(crate) fn frame(&self, now: Instant) -> (View, f32, bool) {
        let linear = (now.duration_since(self.started).as_secs_f32()
            / self.duration.as_secs_f32().max(f32::EPSILON))
        .clamp(0.0, 1.0);
        let progress = 1.0 - (1.0 - linear).powi(3);
        let mut view = self.target.clone();
        view.destination = interpolate_rect(self.from, self.target.destination, progress);
        (view, progress, linear >= 1.0)
    }
}

pub(crate) fn interpolate_rect(from: Rect, to: Rect, progress: f32) -> Rect {
    Rect {
        x: interpolate_u32(from.x, to.x, progress),
        y: interpolate_u32(from.y, to.y, progress),
        width: interpolate_u32(from.width, to.width, progress).max(1),
        height: interpolate_u32(from.height, to.height, progress).max(1),
    }
}

fn interpolate_u32(from: u32, to: u32, progress: f32) -> u32 {
    (from as f32 + (to as f32 - from as f32) * progress).round() as u32
}

pub(crate) fn lens_animation_steps(pixels: u64) -> u32 {
    if pixels > 8_294_400 {
        4
    } else if pixels > 3_686_400 {
        6
    } else {
        8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens_rectangle_interpolates_position_and_size() {
        assert_eq!(
            interpolate_rect(
                Rect {
                    x: 100,
                    y: 50,
                    width: 100,
                    height: 50,
                },
                Rect {
                    x: 0,
                    y: 0,
                    width: 400,
                    height: 200,
                },
                0.5,
            ),
            Rect {
                x: 50,
                y: 25,
                width: 250,
                height: 125,
            }
        );
    }

    #[test]
    fn lens_animation_uses_fewer_frames_on_large_outputs() {
        assert_eq!(lens_animation_steps(1920 * 1080), 8);
        assert_eq!(lens_animation_steps(3840 * 2160), 6);
        assert_eq!(lens_animation_steps(7680 * 4320), 4);
    }
}
