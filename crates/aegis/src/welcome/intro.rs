#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub opacity: f32,
    pub offset: f32,
    pub scale: f32,
}

pub fn ease_out(value: f32) -> f32 {
    1.0 - (1.0 - value).powi(3)
}

pub fn rise(
    time: f32,
    start: f32,
    duration: f32,
    distance: f32,
    initial_scale: f32,
    reduced: bool,
) -> Pose {
    let progress = if reduced {
        1.0
    } else {
        ((time - start) / duration).clamp(0.0, 1.0)
    };
    let eased = ease_out(progress);
    Pose {
        opacity: eased,
        offset: distance * (1.0 - eased),
        scale: initial_scale + (1.0 - initial_scale) * eased,
    }
}

pub fn chip(time: f32, index: usize, reduced: bool) -> Pose {
    if reduced {
        return Pose {
            opacity: 1.0,
            offset: 0.0,
            scale: 1.0,
        };
    }
    let progress = ((time - (1.05 + index as f32 * 0.1)) / 0.6).clamp(0.0, 1.0);
    let scale = if progress < 0.7 {
        0.85 + 0.19 * ease_out(progress / 0.7)
    } else {
        1.04 - 0.04 * ease_out((progress - 0.7) / 0.3)
    };
    Pose {
        opacity: ease_out(progress),
        offset: 0.0,
        scale,
    }
}

pub fn shine(time: f32, reduced: bool) -> Option<f32> {
    if reduced || !(1.1..=2.9).contains(&time) {
        None
    } else {
        Some((time - 1.1) / 1.8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intro_starts_hidden_and_finishes_visible() {
        assert_eq!(rise(0.0, 0.25, 1.0, 28.0, 0.955, false).opacity, 0.0);
        let end = rise(1.25, 0.25, 1.0, 28.0, 0.955, false);
        assert_eq!((end.opacity, end.offset, end.scale), (1.0, 0.0, 1.0));
    }

    #[test]
    fn chip_overshoots_then_settles() {
        assert!(chip(1.5, 0, false).scale > 1.0);
        assert!((chip(1.8, 0, false).scale - 1.0).abs() < 0.001);
    }

    #[test]
    fn reduced_motion_is_visible_immediately() {
        assert_eq!(rise(0.0, 1.0, 2.0, 28.0, 0.5, true).opacity, 1.0);
        assert_eq!(chip(0.0, 2, true).scale, 1.0);
        assert!(shine(1.5, true).is_none());
    }

    #[test]
    fn ease_out_is_smooth_and_monotonic() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.25) > 0.25);
        assert!(ease_out(0.5) > ease_out(0.25));
    }
}
