//! How the 3D garden is lit for a sun position: the light's direction and
//! strength, the ambient light and the sky. Plain numbers (no Bevy), so it
//! can be tested anywhere.
//!
//! Axes as in the scene: plan x is 3D x, plan y is 3D z, so the top of the
//! plan is −z and up is +y.

/// Direct sunlight in full day (lux), as the viewer has always used.
pub const DAY_SUN: f32 = 9_900.0;
/// Ambient light by day and by night: night is dim, not black.
pub const DAY_AMBIENT: f32 = 770.0;
pub const NIGHT_AMBIENT: f32 = 250.0;
/// Sky colours (sRGB): the usual pale sky, and a dusky blue-grey.
pub const DAY_SKY: [f32; 3] = [0.874, 0.913, 0.886];
pub const NIGHT_SKY: [f32; 3] = [0.29, 0.33, 0.39];

/// How the scene is lit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// Unit vector from the garden toward the sun.
    pub dir_to_sun: [f32; 3],
    /// Direct sunlight (lux); 0 when the sun is down.
    pub sun: f32,
    /// Ambient brightness.
    pub ambient: f32,
    /// Whether the sun casts shadows (only when it's up).
    pub shadows: bool,
    /// Sky colour.
    pub sky: [f32; 3],
}

/// 0 at night, 1 in full day, easing in from just below the horizon to 10°.
fn daylight(altitude: f32) -> f32 {
    let t = ((altitude + 1.0) / 11.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `a` at t = 0, exactly `b` at t = 1.
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// The light for a sun at `bearing` (degrees clockwise from the top of the
/// plan) and `altitude` (degrees above the horizon).
pub fn light_for(bearing: f32, altitude: f32) -> Light {
    let (b, a) = (bearing.to_radians(), altitude.to_radians());
    let day = daylight(altitude);
    Light {
        dir_to_sun: [b.sin() * a.cos(), a.sin(), -b.cos() * a.cos()],
        sun: DAY_SUN * day,
        ambient: mix(NIGHT_AMBIENT, DAY_AMBIENT, day),
        shadows: altitude > 0.0,
        sky: [
            mix(NIGHT_SKY[0], DAY_SKY[0], day),
            mix(NIGHT_SKY[1], DAY_SKY[1], day),
            mix(NIGHT_SKY[2], DAY_SKY[2], day),
        ],
    }
}

/// The light for a garden with no location: a low sun from the south-west,
/// as before there was a real sun.
pub fn default_light() -> Light {
    let len = (30.0f32 * 30.0 + 36.0 * 36.0 + 30.0 * 30.0).sqrt();
    Light {
        dir_to_sun: [-30.0 / len, 36.0 / len, 30.0 / len],
        sun: DAY_SUN,
        ambient: DAY_AMBIENT,
        shadows: true,
        sky: DAY_SKY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn a_southern_sun_lights_from_plan_down() {
        // bearing 180: the bottom of the plan (+z), 45 degrees up
        let l = light_for(180.0, 45.0);
        assert!(
            close(
                l.dir_to_sun,
                [
                    0.0,
                    std::f32::consts::FRAC_1_SQRT_2,
                    std::f32::consts::FRAC_1_SQRT_2
                ]
            ),
            "{:?}",
            l.dir_to_sun
        );
        assert_eq!(l.sun, DAY_SUN);
        assert!(l.shadows);
    }

    #[test]
    fn a_sun_on_the_horizon_is_weak_and_level() {
        let l = light_for(90.0, 0.0);
        assert!(close(l.dir_to_sun, [1.0, 0.0, 0.0]), "{:?}", l.dir_to_sun);
        assert!(l.sun > 0.0 && l.sun < DAY_SUN / 4.0, "{}", l.sun);
    }

    #[test]
    fn night_has_no_sun_or_shadows_but_is_not_black() {
        let l = light_for(0.0, -10.0);
        assert_eq!(l.sun, 0.0);
        assert!(!l.shadows);
        assert_eq!(l.ambient, NIGHT_AMBIENT);
        assert!(l.ambient > 0.0);
    }

    #[test]
    fn a_high_sun_is_full_daylight() {
        let l = light_for(200.0, 60.0);
        assert_eq!(l.sun, DAY_SUN);
        assert_eq!(l.ambient, DAY_AMBIENT);
        assert_eq!(l.sky, DAY_SKY);
    }

    #[test]
    fn the_default_light_is_the_old_south_west_sun() {
        let d = default_light();
        let len = (30.0f32 * 30.0 + 36.0 * 36.0 + 30.0 * 30.0).sqrt();
        assert!(
            close(d.dir_to_sun, [-30.0 / len, 36.0 / len, 30.0 / len]),
            "{:?}",
            d.dir_to_sun
        );
        assert_eq!(d.sun, DAY_SUN);
        assert!(d.shadows);
    }
}
