//! Time zones for gardens: the zone nearest a place (from a built-in table
//! of the time zone database's zones), readable city names, and turning a
//! garden's clock time into a moment. The rules of each zone (summer time
//! and all) come from the browser; here an offset function stands in for them.

use loma_core::model::GeoPoint;

#[path = "timezones_data.rs"]
mod data;

/// The zone whose representative place is nearest `at` (along the Earth's
/// surface, so it works across the date line). Usually the garden's zone;
/// near a border it can be the neighbour's, which the person can correct.
pub fn nearest_zone(at: GeoPoint) -> &'static str {
    let (lat1, lon1) = (at.lat.to_radians(), at.lon.to_radians());
    // haversine, without the constant factors (only the order matters)
    let distance = |lat: f64, lon: f64| {
        let (lat2, lon2) = (lat.to_radians(), lon.to_radians());
        let a = ((lat2 - lat1) / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
        a.sqrt().asin()
    };
    data::ZONES
        .iter()
        .min_by(|a, b| distance(a.1, a.2).total_cmp(&distance(b.1, b.2)))
        .map(|z| z.0)
        .unwrap_or("UTC")
}

/// The place a zone is named after: "Australia/Sydney" → "Sydney".
pub fn zone_city(zone: &str) -> String {
    zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
}

/// Every zone in the table, in alphabetical order.
pub fn zone_names() -> impl Iterator<Item = &'static str> {
    data::ZONES.iter().map(|z| z.0)
}

/// The moment whose clock reading in a zone is `wall_ms` (the reading
/// written as if it were UTC). `offset_ms(t)` is the zone's offset from UTC
/// at moment `t`. A reading skipped when clocks go forward lands just after
/// the gap; a reading repeated when they go back gives one of its moments.
pub fn local_to_utc(wall_ms: f64, offset_ms: impl Fn(f64) -> f64) -> f64 {
    let first = offset_ms(wall_ms - offset_ms(wall_ms));
    let t = wall_ms - first;
    let second = offset_ms(t);
    if second == first {
        return t;
    }
    let t2 = wall_ms - second;
    if offset_ms(t2) == second {
        return t2;
    }
    // neither fits: the reading is in a gap; use the offset from before it
    wall_ms - first.min(second)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lat: f64, lon: f64) -> GeoPoint {
        GeoPoint { lat, lon }
    }

    #[test]
    fn the_nearest_zone_to_a_garden() {
        assert_eq!(nearest_zone(at(51.48, 0.0)), "Europe/London");
        assert_eq!(nearest_zone(at(-33.87, 151.21)), "Australia/Sydney");
        assert_eq!(nearest_zone(at(40.41, 49.87)), "Asia/Baku");
        assert_eq!(nearest_zone(at(40.71, -74.0)), "America/New_York");
        // either side of the date line
        assert_eq!(nearest_zone(at(-18.1, 178.4)), "Pacific/Fiji");
        assert_eq!(nearest_zone(at(-13.8, -171.8)), "Pacific/Apia");
    }

    #[test]
    fn zones_read_as_cities() {
        assert_eq!(zone_city("Australia/Sydney"), "Sydney");
        assert_eq!(zone_city("America/Argentina/Buenos_Aires"), "Buenos Aires");
        assert_eq!(zone_city("UTC"), "UTC");
    }

    #[test]
    fn zone_names_are_sorted_and_complete() {
        let names: Vec<_> = zone_names().collect();
        assert!(names.len() > 400);
        assert!(names.windows(2).all(|w| w[0] < w[1]));
        assert!(names.contains(&"Asia/Baku"));
    }

    const H: f64 = 3_600_000.0;
    const T0: f64 = 1_000_000_000_000.0;

    /// +1 h before T0, +2 h from T0 (clocks go forward at T0).
    fn spring(t: f64) -> f64 {
        if t < T0 { H } else { 2.0 * H }
    }

    /// +2 h before T0, +1 h from T0 (clocks go back at T0).
    fn autumn(t: f64) -> f64 {
        if t < T0 { 2.0 * H } else { H }
    }

    #[test]
    fn clock_times_become_moments() {
        for u in [T0 - 5.0 * H, T0 + 5.0 * H] {
            assert_eq!(local_to_utc(u + spring(u), spring), u);
            assert_eq!(local_to_utc(u + autumn(u), autumn), u);
        }
    }

    #[test]
    fn a_time_skipped_by_summer_time_lands_after_the_gap() {
        // local times from T0+1h to T0+2h never happen; 1.5 h in reads 2.5 h
        let t = local_to_utc(T0 + 1.5 * H, spring);
        assert_eq!(t, T0 + 0.5 * H);
    }

    #[test]
    fn a_repeated_hour_picks_one_of_its_two_moments() {
        let t = local_to_utc(T0 + 1.5 * H, autumn);
        assert!(t == T0 - 0.5 * H || t == T0 + 0.5 * H, "{t}");
    }
}
