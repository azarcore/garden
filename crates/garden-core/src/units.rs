//! Length display and parsing. All lengths are millimetres (`f64`).

use serde::{Deserialize, Serialize};

const MM_PER_INCH: f64 = 25.4;

/// Which unit system lengths are shown and typed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UnitSystem {
    #[default]
    Metric,
    Imperial,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ParseLengthError {
    #[error("no length entered")]
    Empty,
    #[error("not a length: {0}")]
    Invalid(String),
    #[error("length cannot be negative")]
    Negative,
}

/// Format a length for display, e.g. `"45 cm"`, `"1.25 m"`, `"5' 3 1/4\""`.
pub fn format_length(mm: f64, system: UnitSystem) -> String {
    if !mm.is_finite() {
        return "—".to_string();
    }
    let sign = if mm < 0.0 { "-" } else { "" };
    let mm = mm.abs();
    match system {
        UnitSystem::Metric => {
            if mm < 1000.0 {
                let cm = mm.round() / 10.0;
                if cm.fract() == 0.0 {
                    format!("{sign}{cm:.0} cm")
                } else {
                    format!("{sign}{cm:.1} cm")
                }
            } else {
                format!("{sign}{:.2} m", mm / 1000.0)
            }
        }
        UnitSystem::Imperial => {
            let quarters = (mm / MM_PER_INCH * 4.0).round() as u64;
            let feet = quarters / 48;
            let rem = quarters % 48;
            let whole = rem / 4;
            let frac = match rem % 4 {
                1 => "1/4",
                2 => "1/2",
                3 => "3/4",
                _ => "",
            };
            let inches = match (whole, frac.is_empty()) {
                (_, true) => whole.to_string(),
                (0, false) if feet == 0 => frac.to_string(),
                _ => format!("{whole} {frac}"),
            };
            if feet > 0 {
                format!("{sign}{feet}' {inches}\"")
            } else {
                format!("{sign}{inches}\"")
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl Unit {
    fn mm(self) -> f64 {
        match self {
            Unit::Mm => 1.0,
            Unit::Cm => 10.0,
            Unit::M => 1000.0,
            Unit::In => MM_PER_INCH,
            Unit::Ft => MM_PER_INCH * 12.0,
        }
    }

    fn parse(s: &str) -> Option<Unit> {
        Some(match s.to_ascii_lowercase().as_str() {
            "mm" => Unit::Mm,
            "cm" => Unit::Cm,
            "m" => Unit::M,
            "in" | "\"" | "″" => Unit::In,
            "ft" | "'" | "′" => Unit::Ft,
            _ => return None,
        })
    }
}

/// A comma followed by exactly three digits reads like a thousands separator.
fn has_thousands_comma(text: &str) -> bool {
    let c: Vec<char> = text.chars().collect();
    (0..c.len()).any(|k| {
        c[k] == ','
            && c.get(k + 1..k + 4)
                .is_some_and(|d| d.iter().all(char::is_ascii_digit))
            && !c.get(k + 4).is_some_and(char::is_ascii_digit)
    })
}

fn read_digits(chars: &[char], mut i: usize, allow_dot: bool) -> (String, usize) {
    let start = i;
    while i < chars.len() && (chars[i].is_ascii_digit() || (allow_dot && chars[i] == '.')) {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

/// Read `12`, `1.5`, `3/4` or a mixed number `1 1/2` starting at `i`.
fn read_number(chars: &[char], i: usize) -> Option<(f64, usize)> {
    let (whole, mut i) = read_digits(chars, i, true);
    let mut value: f64 = whole.parse().ok()?;
    if chars.get(i) == Some(&'/') {
        let (den, next) = read_digits(chars, i + 1, false);
        let den: f64 = den.parse().ok()?;
        if den == 0.0 || whole.contains('.') {
            return None;
        }
        return Some((value / den, next));
    }
    // mixed number: whole part, space, then n/d
    let mut j = i;
    while j < chars.len() && chars[j].is_whitespace() {
        j += 1;
    }
    let (num, k) = read_digits(chars, j, false);
    if j > i && !num.is_empty() && chars.get(k) == Some(&'/') {
        let (den, next) = read_digits(chars, k + 1, false);
        let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
        if den == 0.0 || whole.contains('.') {
            return None;
        }
        value += num / den;
        i = next;
    }
    Some((value, i))
}

/// Parse user input such as `"1,5 m"`, `"120cm"`, `"5' 3\""`, `"5ft 3in"`.
/// A bare number is metres (metric) or feet (imperial). Returns millimetres.
pub fn parse_length(input: &str, system: UnitSystem) -> Result<f64, ParseLengthError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(ParseLengthError::Empty);
    }
    if text.starts_with('-') {
        return Err(ParseLengthError::Negative);
    }
    let invalid = || ParseLengthError::Invalid(text.to_string());
    if has_thousands_comma(text) {
        // "1,500" could mean 1.5 or 1500: refuse rather than guess.
        return Err(invalid());
    }
    let normalized = text.replace(',', ".");
    let chars: Vec<char> = normalized.chars().collect();

    let mut pairs: Vec<(f64, Option<Unit>)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let (value, next) = read_number(&chars, i).ok_or_else(invalid)?;
        i = next;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let ustart = i;
        while i < chars.len()
            && !chars[i].is_whitespace()
            && !chars[i].is_ascii_digit()
            && chars[i] != '.'
        {
            i += 1;
            // quote marks are single-char units
            if matches!(chars[i - 1], '\'' | '"' | '′' | '″') {
                break;
            }
        }
        let unit_text: String = chars[ustart..i].iter().collect();
        let unit = if unit_text.is_empty() {
            None
        } else {
            Some(Unit::parse(&unit_text).ok_or_else(invalid)?)
        };
        pairs.push((value, unit));
        // allow the US "5'-3\"" style between feet and inches
        if unit == Some(Unit::Ft) {
            while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '-') {
                i += 1;
            }
        }
    }

    let default_unit = match system {
        UnitSystem::Metric => Unit::M,
        UnitSystem::Imperial => Unit::Ft,
    };
    let mut total = 0.0;
    let mut prev: Option<Unit> = None;
    let count = pairs.len();
    for (idx, (value, unit)) in pairs.into_iter().enumerate() {
        let unit = match unit {
            Some(u) => u,
            None if count == 1 => default_unit,
            None if idx == count - 1 && prev == Some(Unit::Ft) => Unit::In,
            None => return Err(invalid()),
        };
        total += value * unit.mm();
        prev = Some(unit);
    }
    if count == 0 || !total.is_finite() {
        return Err(invalid());
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_metric() {
        assert_eq!(format_length(450.0, UnitSystem::Metric), "45 cm");
        assert_eq!(format_length(455.0, UnitSystem::Metric), "45.5 cm");
        assert_eq!(format_length(1250.0, UnitSystem::Metric), "1.25 m");
        assert_eq!(format_length(100_000.0, UnitSystem::Metric), "100.00 m");
    }

    #[test]
    fn formats_imperial() {
        assert_eq!(format_length(1606.55, UnitSystem::Imperial), "5' 3 1/4\""); // 63.25 in
        assert_eq!(format_length(82.55, UnitSystem::Imperial), "3 1/4\"");
        assert_eq!(format_length(1524.0, UnitSystem::Imperial), "5' 0\"");
        assert_eq!(format_length(1828.0, UnitSystem::Imperial), "6' 0\""); // 71.97 in rounds to 72
    }

    #[test]
    fn parses_metric_forms() {
        for (s, mm) in [
            ("1.2m", 1200.0),
            ("120 cm", 1200.0),
            ("1200mm", 1200.0),
            ("1,5 m", 1500.0),
            ("  2  ", 2000.0),
        ] {
            assert!(
                (parse_length(s, UnitSystem::Metric).unwrap() - mm).abs() < 1e-9,
                "{s}"
            );
        }
    }

    #[test]
    fn parses_imperial_forms() {
        for s in ["5' 3\"", "5ft 3in", "5'3", "63in", "63\""] {
            assert!(
                (parse_length(s, UnitSystem::Imperial).unwrap() - 1600.2).abs() < 1e-6,
                "{s}"
            );
        }
        assert!((parse_length("2", UnitSystem::Imperial).unwrap() - 609.6).abs() < 1e-9);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            parse_length("", UnitSystem::Metric),
            Err(ParseLengthError::Empty)
        );
        assert_eq!(
            parse_length("-1m", UnitSystem::Metric),
            Err(ParseLengthError::Negative)
        );
        assert!(matches!(
            parse_length("abc", UnitSystem::Metric),
            Err(ParseLengthError::Invalid(_))
        ));
        assert!(matches!(
            parse_length("1.2.3m", UnitSystem::Metric),
            Err(ParseLengthError::Invalid(_))
        ));
    }
}
