//! Calendar days, with no time of day and no time zone.
//! Stored and shown as "YYYY-MM-DD".

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A calendar day, counted in days since 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date(i32);

impl Date {
    /// None for days that don't exist (2025-02-29, month 13, …) or years outside 1..=9999.
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<Date> {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date(days_from_civil(year, month, day)))
    }

    /// Exactly "YYYY-MM-DD".
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let digits = |r: std::ops::Range<usize>| -> Option<u32> {
            let part = &s[r];
            part.bytes()
                .all(|c| c.is_ascii_digit())
                .then(|| part.parse().ok())
                .flatten()
        };
        let year = i32::try_from(digits(0..4)?).ok()?;
        Date::from_ymd(year, digits(5..7)?, digits(8..10)?)
    }

    pub fn ymd(self) -> (i32, u32, u32) {
        civil_from_days(self.0)
    }

    pub fn year(self) -> i32 {
        self.ymd().0
    }

    pub fn add_days(self, days: i32) -> Date {
        Date(self.0 + days)
    }

    /// `self` minus `earlier`, in days.
    pub fn days_since(self, earlier: Date) -> i32 {
        self.0 - earlier.0
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// Howard Hinnant's days-from-civil and civil-from-days.
fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i32::from(month <= 2);
    (year, month, day)
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let s = String::deserialize(d)?;
        Date::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a date: {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn round_trips_text() {
        let leap = d("2024-02-29");
        assert_eq!(leap.to_string(), "2024-02-29");
        assert_eq!(leap.ymd(), (2024, 2, 29));
        let json = serde_json::to_string(&leap).unwrap();
        assert_eq!(json, "\"2024-02-29\"");
        assert_eq!(serde_json::from_str::<Date>(&json).unwrap(), leap);
        for bad in [
            "2025-02-29",
            "2024-13-01",
            "2024-1-5",
            "",
            "2024-04-31",
            "0000-01-01",
            "2024-06-01x",
        ] {
            assert_eq!(Date::parse(bad), None, "{bad}");
        }
        assert!(serde_json::from_str::<Date>("\"2024-02-30\"").is_err());
    }

    #[test]
    fn adds_across_edges() {
        assert_eq!(d("2024-02-28").add_days(1), d("2024-02-29"));
        assert_eq!(d("2023-02-28").add_days(1), d("2023-03-01"));
        assert_eq!(d("2024-12-31").add_days(1), d("2025-01-01"));
        assert_eq!(d("2025-01-01").add_days(-1), d("2024-12-31"));
        assert_eq!(d("2024-06-01").add_days(21), d("2024-06-22"));
        assert_eq!(d("2025-01-01").days_since(d("2024-01-01")), 366);
        assert_eq!(d("2100-03-01").days_since(d("2100-02-28")), 1);
        assert_eq!(d("2000-03-01").days_since(d("2000-02-28")), 2);
    }

    #[test]
    fn orders_by_day() {
        assert!(d("2024-06-01") < d("2024-06-02"));
        assert_eq!(d("1970-01-01"), Date(0));
        assert_eq!(d("1969-12-31"), Date(-1));
        assert_eq!(d("2024-06-15").year(), 2024);
    }
}
