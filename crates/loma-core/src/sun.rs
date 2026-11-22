//! Where the sun is: azimuth and altitude for a place and a moment, and the
//! day's sunrise and sunset. NOAA's solar position equations (accurate to a
//! small fraction of a degree for years around now). Times are UTC
//! milliseconds since 1970.

use crate::model::GeoPoint;

/// Where the sun is in the sky.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunPosition {
    /// Degrees clockwise from true north, in [0, 360).
    pub azimuth: f64,
    /// Degrees above the horizon (negative below), with refraction.
    pub altitude: f64,
}

/// The sun's centre at sunrise and sunset: refraction and its radius.
const HORIZON: f64 = -0.833;
const DAY_MS: f64 = 86_400_000.0;

/// Azimuth and geometric (unrefracted) altitude, in degrees.
fn geometric(at: GeoPoint, utc_ms: f64) -> (f64, f64) {
    let rad = f64::to_radians;
    let deg = f64::to_degrees;
    let jd = utc_ms / DAY_MS + 2440587.5;
    let t = (jd - 2451545.0) / 36525.0;
    let l0 = (280.46646 + t * (36000.76983 + t * 0.0003032)).rem_euclid(360.0);
    let m = 357.52911 + t * (35999.05029 - 0.0001537 * t);
    let e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let c = rad(m).sin() * (1.914602 - t * (0.004817 + 0.000014 * t))
        + rad(2.0 * m).sin() * (0.019993 - 0.000101 * t)
        + rad(3.0 * m).sin() * 0.000289;
    let omega = 125.04 - 1934.136 * t;
    let apparent_long = l0 + c - 0.00569 - 0.00478 * rad(omega).sin();
    let mean_obliquity =
        23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
    let obliquity = mean_obliquity + 0.00256 * rad(omega).cos();
    let decl = (rad(obliquity).sin() * rad(apparent_long).sin()).asin();
    let y = rad(obliquity / 2.0).tan().powi(2);
    let (l0r, mr) = (rad(l0), rad(m));
    let equation_of_time = 4.0
        * deg(y * (2.0 * l0r).sin() - 2.0 * e * mr.sin()
            + 4.0 * e * y * mr.sin() * (2.0 * l0r).cos()
            - 0.5 * y * y * (4.0 * l0r).sin()
            - 1.25 * e * e * (2.0 * mr).sin());
    let minutes = utc_ms.rem_euclid(DAY_MS) / 60000.0;
    let true_solar = (minutes + equation_of_time + 4.0 * at.lon).rem_euclid(1440.0);
    let hour_angle = rad(true_solar / 4.0 - 180.0);
    let lat = rad(at.lat);
    let cos_zenith =
        (lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos()).clamp(-1.0, 1.0);
    let altitude = 90.0 - deg(cos_zenith.acos());
    // measured from south, westwards; +180 turns it to from north, clockwise
    let azimuth = deg(hour_angle
        .sin()
        .atan2(hour_angle.cos() * lat.sin() - decl.tan() * lat.cos()))
        + 180.0;
    (crate::model::normalize_degrees(azimuth), altitude)
}

/// Atmospheric refraction (degrees) for a geometric altitude, as NOAA gives it.
fn refraction(altitude: f64) -> f64 {
    let te = altitude.to_radians().tan();
    let arcsec = if altitude > 85.0 {
        0.0
    } else if altitude > 5.0 {
        58.1 / te - 0.07 / te.powi(3) + 0.000086 / te.powi(5)
    } else if altitude > -0.575 {
        1735.0 + altitude * (-518.2 + altitude * (103.4 + altitude * (-12.79 + altitude * 0.711)))
    } else {
        -20.774 / te
    };
    arcsec / 3600.0
}

/// The sun as seen from `at` at `utc_ms`.
pub fn sun_position(at: GeoPoint, utc_ms: f64) -> SunPosition {
    let (azimuth, altitude) = geometric(at, utc_ms);
    SunPosition {
        azimuth,
        altitude: altitude + refraction(altitude),
    }
}

/// The first sunrise and the first sunset between `from_ms` and `to_ms`
/// (usually a local day). `None` where the sun doesn't rise or set then, as
/// in polar day and night.
pub fn sun_times(at: GeoPoint, from_ms: f64, to_ms: f64) -> (Option<f64>, Option<f64>) {
    const STEP: f64 = 10.0 * 60_000.0;
    let above = |t: f64| geometric(at, t).1 - HORIZON;
    // narrow a crossing between a and b down to under 30 seconds
    let cross = |mut a: f64, mut b: f64| {
        let rising = above(a) < 0.0;
        while b - a > 30_000.0 {
            let mid = (a + b) / 2.0;
            if (above(mid) < 0.0) == rising {
                a = mid;
            } else {
                b = mid;
            }
        }
        (a + b) / 2.0
    };
    let (mut rise, mut set) = (None, None);
    let mut t = from_ms;
    let mut prev = above(t);
    while t < to_ms && (rise.is_none() || set.is_none()) {
        let next_t = (t + STEP).min(to_ms);
        let next = above(next_t);
        if prev < 0.0 && next >= 0.0 && rise.is_none() {
            rise = Some(cross(t, next_t));
        } else if prev >= 0.0 && next < 0.0 && set.is_none() {
            set = Some(cross(t, next_t));
        }
        t = next_t;
        prev = next;
    }
    (rise, set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Date;

    /// UTC milliseconds for a date and time.
    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> f64 {
        let days = Date::from_ymd(y, m, d)
            .unwrap()
            .days_since(Date::from_ymd(1970, 1, 1).unwrap());
        (days as f64 * 24.0 + h as f64) * 3_600_000.0 + min as f64 * 60_000.0
    }

    const GREENWICH: GeoPoint = GeoPoint {
        lat: 51.4779,
        lon: -0.0015,
    };
    const SYDNEY: GeoPoint = GeoPoint {
        lat: -33.87,
        lon: 151.21,
    };
    const SVALBARD: GeoPoint = GeoPoint {
        lat: 78.22,
        lon: 15.65,
    };

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn midsummer_noon_at_greenwich_is_high_and_due_south() {
        let s = sun_position(GREENWICH, utc(2024, 6, 20, 12, 2));
        assert!(near(s.altitude, 61.9, 0.5), "{s:?}");
        assert!(near(s.azimuth, 180.0, 2.0), "{s:?}");
    }

    #[test]
    fn midwinter_noon_at_greenwich_is_low() {
        let s = sun_position(GREENWICH, utc(2024, 12, 21, 12, 2));
        assert!(near(s.altitude, 15.1, 0.5), "{s:?}");
    }

    #[test]
    fn noon_in_sydney_is_due_north() {
        let s = sun_position(SYDNEY, utc(2024, 6, 21, 1, 57));
        let off_north = s.azimuth.min(360.0 - s.azimuth);
        assert!(off_north < 3.0, "{s:?}");
        assert!(near(s.altitude, 32.7, 0.7), "{s:?}");
    }

    #[test]
    fn the_sun_rises_in_the_east_and_sets_in_the_west() {
        // Greenwich, midsummer: about east-north-east early, west-north-west late
        let morning = sun_position(GREENWICH, utc(2024, 6, 21, 6, 0));
        let evening = sun_position(GREENWICH, utc(2024, 6, 21, 18, 0));
        assert!((60.0..110.0).contains(&morning.azimuth), "{morning:?}");
        assert!((250.0..300.0).contains(&evening.azimuth), "{evening:?}");
    }

    #[test]
    fn the_sun_is_down_at_midnight() {
        assert!(sun_position(GREENWICH, utc(2024, 6, 21, 0, 0)).altitude < 0.0);
    }

    #[test]
    fn sunrise_and_sunset_at_greenwich_in_june() {
        let (rise, set) = sun_times(GREENWICH, utc(2024, 6, 21, 0, 0), utc(2024, 6, 22, 0, 0));
        let minutes = |t: f64| (t - utc(2024, 6, 21, 0, 0)) / 60_000.0;
        let rise = minutes(rise.unwrap());
        let set = minutes(set.unwrap());
        assert!(near(rise, 3.0 * 60.0 + 43.0, 4.0), "rise {rise}");
        assert!(near(set, 20.0 * 60.0 + 21.0, 4.0), "set {set}");
    }

    #[test]
    fn polar_day_and_night_have_no_sunrise_or_sunset() {
        let june = sun_times(SVALBARD, utc(2024, 6, 21, 0, 0), utc(2024, 6, 22, 0, 0));
        assert_eq!(june, (None, None));
        assert!(sun_position(SVALBARD, utc(2024, 6, 21, 11, 0)).altitude > 0.0);
        let december = sun_times(SVALBARD, utc(2024, 12, 21, 0, 0), utc(2024, 12, 22, 0, 0));
        assert_eq!(december, (None, None));
        assert!(sun_position(SVALBARD, utc(2024, 12, 21, 11, 0)).altitude < 0.0);
    }
}
