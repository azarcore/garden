//! The compass: which way true north is on the plan. Bearings are degrees
//! clockwise from the top of the plan (screen up), in [0, 360).

pub use loma_core::model::normalize_degrees as normalize;

/// The nearest multiple of `step` degrees, in [0, 360).
pub fn snap(deg: f64, step: f64) -> f64 {
    normalize((deg / step).round() * step)
}

/// The bearing of a pointer at (`px`, `py`) seen from the compass centre
/// (`cx`, `cy`), in screen coordinates (y down). `None` when the pointer is
/// too close to the centre for the direction to mean anything.
pub fn bearing_from_pointer(cx: f64, cy: f64, px: f64, py: f64) -> Option<f64> {
    let (dx, dy) = (px - cx, py - cy);
    if dx.hypot(dy) <= 2.0 {
        return None;
    }
    Some(normalize(dx.atan2(-dy).to_degrees()))
}

/// A typed bearing such as "23", "23.5°" or "-90", in [0, 360).
pub fn parse_bearing(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s.strip_suffix('°').unwrap_or(s).trim();
    let v: f64 = s.parse().ok()?;
    v.is_finite().then(|| normalize(v))
}

/// The compass for printouts and exports: a ring, a needle turned to
/// `north` (dark half pointing north) and an upright "N" past its tip.
/// `size` × `size` in `unit` ("mm", or "" for user units).
pub fn compass_svg(north: f64, size: f64, unit: &str) -> String {
    const INK: &str = "#1c241e";
    let (sin, cos) = north.to_radians().sin_cos();
    // "N" sits 13 units out from the centre (16, 16), along the needle
    let (nx, ny) = (16.0 + 13.0 * sin, 16.0 - 13.0 * cos);
    format!(
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{s}{u}" height="{s}{u}" viewBox="0 0 32 32">"#,
            r##"<circle cx="16" cy="16" r="9.5" fill="#ffffff" stroke="{ink}" stroke-width="0.8"/>"##,
            r#"<g transform="rotate({n:.2} 16 16)">"#,
            r#"<polygon points="16,7 18.6,16 13.4,16" fill="{ink}"/>"#,
            r##"<polygon points="16,25 18.6,16 13.4,16" fill="#ffffff" stroke="{ink}" stroke-width="0.6" stroke-linejoin="round"/>"##,
            r#"</g>"#,
            r#"<text x="{nx:.2}" y="{ny:.2}" font-family="sans-serif" font-size="6" font-weight="700" text-anchor="middle" dominant-baseline="central" fill="{ink}">N</text>"#,
            r#"</svg>"#
        ),
        s = size,
        u = unit,
        ink = INK,
        n = north,
        nx = nx,
        ny = ny,
    )
}

/// A freely dragged bearing, kept to a tenth of a degree (what the bearing
/// box shows), so opening the box and leaving it changes nothing.
pub fn free_bearing(deg: f64) -> f64 {
    normalize((deg * 10.0).round() / 10.0)
}

/// A bearing as the bearing box shows it: to a tenth of a degree, no
/// trailing ".0", and never "360".
pub fn bearing_text(deg: f64) -> String {
    let r = free_bearing(deg);
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}

/// `svg` (a whole plan framed on `world`, in its own units) with the compass
/// in its top-right corner: 8% of the shorter side, but always inside the
/// empty margin above `content` (the drawn garden) so it never covers it.
/// Unchanged if there's no room or no closing `</svg>`.
pub fn with_compass_in_corner(
    svg: &str,
    world: kurbo::Rect,
    content: kurbo::Rect,
    north: f64,
) -> String {
    let Some(end) = svg.rfind("</svg>") else {
        return svg.to_string();
    };
    let margin = content.y0 - world.y0;
    let size = (0.08 * world.width().min(world.height())).min(0.8 * margin);
    if size <= 0.0 {
        return svg.to_string();
    }
    // a quarter of its size in from the corner: within the margin, as size ≤ 0.8 × margin
    let (x, y) = (world.x1 - 1.25 * size, world.y0 + 0.25 * size);
    let compass =
        compass_svg(north, size, "").replacen("<svg ", &format!(r#"<svg x="{x}" y="{y}" "#), 1);
    format!("{}{compass}{}", &svg[..end], &svg[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn normalize_wraps_into_a_turn() {
        assert_eq!(normalize(-10.0), 350.0);
        assert_eq!(normalize(720.0), 0.0);
        assert!(normalize(-1e-20) < 360.0);
    }

    #[test]
    fn snap_rounds_to_the_step_and_wraps() {
        assert_eq!(snap(357.6, 5.0), 0.0);
        assert_eq!(snap(23.0, 5.0), 25.0);
        assert_eq!(snap(22.4, 5.0), 20.0);
    }

    #[test]
    fn bearing_from_pointer_is_clockwise_from_up() {
        let b = |px, py| bearing_from_pointer(50.0, 50.0, px, py);
        assert!(close(b(50.0, 0.0).unwrap(), 0.0));
        assert!(close(b(100.0, 50.0).unwrap(), 90.0));
        assert!(close(b(50.0, 100.0).unwrap(), 180.0));
        assert!(close(b(0.0, 50.0).unwrap(), 270.0));
        assert_eq!(b(51.0, 50.0), None, "too close to the centre to tell");
    }

    #[test]
    fn parse_bearing_reads_degrees() {
        assert_eq!(parse_bearing("23"), Some(23.0));
        assert_eq!(parse_bearing(" 23.5° "), Some(23.5));
        assert_eq!(parse_bearing("360"), Some(0.0));
        assert_eq!(parse_bearing("-90"), Some(270.0));
        for bad in ["", "abc", "NaN", "1e400", "°"] {
            assert_eq!(parse_bearing(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn compass_svg_is_turned_and_labelled() {
        let svg = compass_svg(90.0, 12.0, "mm");
        assert!(svg.starts_with("<svg"), "{svg}");
        assert!(svg.contains(r#"width="12mm""#), "{svg}");
        assert!(svg.contains("rotate(90"), "{svg}");
        assert!(svg.contains(">N<"), "{svg}");
        assert!(compass_svg(0.0, 40.0, "").contains(r#"width="40""#));
    }

    #[test]
    fn compass_goes_in_the_top_right_corner_of_an_export() {
        let plan = r#"<svg viewBox="0 0 10000 5000"><g/></svg>"#;
        let world = kurbo::Rect::new(0.0, 0.0, 10_000.0, 5_000.0);
        let garden = world.inset(-1000.0);
        let out = with_compass_in_corner(plan, world, garden, 45.0);
        // 8% of the short side, a quarter of that in from the corner
        assert!(out.contains(r#"<svg x="9500" y="100" "#), "{out}");
        assert!(out.contains(r#"width="400""#), "{out}");
        assert!(out.contains("rotate(45"), "{out}");
        assert!(out.ends_with("</svg></svg>"), "{out}");
        assert_eq!(
            with_compass_in_corner("no svg here", world, garden, 0.0),
            "no svg here"
        );
    }

    #[test]
    fn export_compass_stays_in_the_margin_of_a_big_garden() {
        // a 20 × 15 m garden framed with a 1 m margin
        let world = kurbo::Rect::new(0.0, 0.0, 22_000.0, 17_000.0);
        let garden = world.inset(-1000.0);
        let out = with_compass_in_corner("<svg></svg>", world, garden, 0.0);
        assert!(out.contains(r#"width="800""#), "fits the 1 m margin: {out}");
        assert!(out.contains(r#"<svg x="21000" y="200" "#), "{out}");
        // no margin at all: no compass rather than one over the plan
        let tight = with_compass_in_corner("<svg></svg>", world, world, 0.0);
        assert_eq!(tight, "<svg></svg>");
    }

    #[test]
    fn free_bearings_keep_a_tenth_of_a_degree() {
        assert_eq!(free_bearing(31.6342), 31.6);
        assert_eq!(free_bearing(359.97), 0.0);
        assert_eq!(free_bearing(-0.04), 0.0);
    }

    #[test]
    fn bearing_text_rounds_to_a_tenth_and_wraps() {
        assert_eq!(bearing_text(23.0), "23");
        assert_eq!(bearing_text(31.6342), "31.6");
        assert_eq!(bearing_text(359.97), "0");
    }
}
