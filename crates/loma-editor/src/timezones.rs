//! Time zones for gardens: the zone nearest a place (from a built-in table
//! of the time zone database's zones), readable city names, and turning a
//! garden's clock time into a moment. The rules of each zone (summer time
//! and all) come from the browser; here an offset function stands in for them.

use loma_core::model::GeoPoint;

#[path = "timezones_data.rs"]
mod data;

/// How far apart two places are along the Earth's surface (as an angle; only
/// the order matters).
fn distance(a: GeoPoint, lat: f64, lon: f64) -> f64 {
    let (lat1, lon1) = (a.lat.to_radians(), a.lon.to_radians());
    let (lat2, lon2) = (lat.to_radians(), lon.to_radians());
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    h.sqrt().asin()
}

/// The zone whose representative place is nearest `at` (along the Earth's
/// surface, so it works across the date line). Usually the garden's zone;
/// near a border it can be the neighbour's, which the person can correct.
pub fn nearest_zone(at: GeoPoint) -> &'static str {
    data::ZONES
        .iter()
        .min_by(|a, b| distance(at, a.1, a.2).total_cmp(&distance(at, b.1, b.2)))
        .map(|z| z.0)
        .unwrap_or("UTC")
}

/// A zone that says nothing about where a device is: privacy settings make
/// some browsers report UTC.
fn says_nothing(zone: &str) -> bool {
    matches!(zone, "UTC" | "GMT" | "Etc/UTC" | "Etc/GMT") || zone.starts_with("Etc/")
}

/// The zone to suggest for a garden at `at`, given the device's own zone:
/// the device's when it's one of the zones close by (most people plan their
/// own garden, and the nearest place can be across a border), else the
/// nearest.
pub fn suggest_zone(at: GeoPoint, device: Option<&str>) -> String {
    let nearest = nearest_zone(at);
    let near = data::ZONES
        .iter()
        .find(|z| z.0 == nearest)
        .map_or(0.0, |z| distance(at, z.1, z.2));
    let device_close = device
        .filter(|d| !says_nothing(d))
        .and_then(|d| data::ZONES.iter().find(|z| z.0 == d))
        .filter(|z| distance(at, z.1, z.2) <= near * 1.5 + 1e-4);
    device_close.map_or(nearest, |z| z.0).to_string()
}

/// The zone for "use my location": the device's own, unless it says
/// nothing, then the nearest.
pub fn zone_here(at: GeoPoint, device: Option<&str>) -> String {
    match device.filter(|d| !d.trim().is_empty() && !says_nothing(d)) {
        Some(d) => d.to_string(),
        None => nearest_zone(at).to_string(),
    }
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
/// the gap; a reading repeated when they go back gives one of its moments
/// (the later one east of UTC, the earlier one west of it; never a jump).
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
    fn the_device_zone_wins_when_it_is_close_by() {
        let vigo = at(42.24, -8.72);
        // nearest by table: Lisbon (an hour off); the device in Madrid knows better
        assert_eq!(suggest_zone(vigo, None), "Europe/Lisbon");
        assert_eq!(suggest_zone(vigo, Some("Europe/Madrid")), "Europe/Madrid");
        // a device far away doesn't override the garden's own area
        assert_eq!(suggest_zone(vigo, Some("Asia/Baku")), "Europe/Lisbon");
        // privacy settings that report UTC tell us nothing
        assert_eq!(suggest_zone(vigo, Some("UTC")), "Europe/Lisbon");
        assert_eq!(suggest_zone(vigo, Some("Etc/GMT")), "Europe/Lisbon");
    }

    #[test]
    fn my_location_takes_the_device_zone_unless_it_says_nothing() {
        let sydney = at(-33.87, 151.21);
        assert_eq!(
            zone_here(sydney, Some("Australia/Sydney")),
            "Australia/Sydney"
        );
        // an older name the browser may give is still the device's own
        assert_eq!(
            zone_here(at(28.6, 77.2), Some("Asia/Calcutta")),
            "Asia/Calcutta"
        );
        assert_eq!(zone_here(sydney, Some("UTC")), "Australia/Sydney");
        assert_eq!(zone_here(sydney, None), "Australia/Sydney");
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
