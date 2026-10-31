use std::time::Duration;

use crate::config::{Motion, MotionCurve};

pub(crate) fn motion_interval(tick_hz: u16) -> Duration {
    Duration::from_secs_f64(1.0 / f64::from(tick_hz))
}

pub(crate) fn motion_speed(config: &Motion, held: Duration) -> f64 {
    let range = config.max_speed - config.initial_speed;
    if range <= 0.0 || config.acceleration == 0.0 {
        return config.initial_speed;
    }
    let progress = (config.acceleration * held.as_secs_f64() / range).clamp(0.0, 1.0);
    let eased = match config.curve {
        MotionCurve::Linear => progress,
        MotionCurve::EaseIn => progress * progress,
        MotionCurve::EaseOut => 1.0 - (1.0 - progress).powi(2),
        MotionCurve::EaseInOut if progress < 0.5 => 2.0 * progress * progress,
        MotionCurve::EaseInOut => 1.0 - (-2.0 * progress + 2.0).powi(2) / 2.0,
    };
    config.initial_speed + range * eased
}

pub(crate) fn motion_distance(speed: f64, elapsed: Duration) -> f64 {
    speed * elapsed.as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_rate_controls_timer_interval() {
        assert_eq!(motion_interval(100), Duration::from_millis(10));
    }

    #[test]
    fn linear_curve_preserves_constant_acceleration() {
        let motion = Motion {
            initial_speed: 60.0,
            acceleration: 100.0,
            max_speed: 160.0,
            tick_hz: 100,
            curve: MotionCurve::Linear,
        };
        assert_eq!(motion_speed(&motion, Duration::from_millis(500)), 110.0);
    }

    #[test]
    fn curves_have_expected_early_ramp_order() {
        let speed = |curve| {
            motion_speed(
                &Motion {
                    initial_speed: 0.0,
                    acceleration: 100.0,
                    max_speed: 100.0,
                    tick_hz: 100,
                    curve,
                },
                Duration::from_millis(250),
            )
        };
        assert!(speed(MotionCurve::EaseIn) < speed(MotionCurve::Linear));
        assert!(speed(MotionCurve::Linear) < speed(MotionCurve::EaseOut));
        assert!(speed(MotionCurve::EaseInOut) < speed(MotionCurve::Linear));
    }

    #[test]
    fn fractional_motion_is_not_rounded_up() {
        let distance = motion_distance(60.0, Duration::from_secs_f64(1.0 / 120.0));
        assert!((distance - 0.5).abs() < 1e-6);
    }
}
