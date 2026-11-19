use kurbo::{BezPath, ParamCurve, ParamCurveArclen, ParamCurveNearest, PathEl, Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

const ARC_TOLERANCE: f64 = 0.01;
const ACCURACY: f64 = 1e-3;
const EPS: f64 = 1e-9;
/// Magic number for approximating a quarter ellipse with one cubic.
pub(crate) const KAPPA: f64 = 0.552_284_749_8;

/// One piece of an outline, starting where the previous piece ended.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Segment {
    Line { to: Point },
    /// Circular arc from the previous point to `to`, turning by `sweep` radians
    /// (positive = increasing angle in y-down coords). |sweep| in (0, 2π).
    Arc { to: Point, sweep: f64 },
    Cubic { c1: Point, c2: Point, to: Point },
}

impl Segment {
    pub fn to(&self) -> Point {
        match *self {
            Segment::Line { to } | Segment::Arc { to, .. } | Segment::Cubic { to, .. } => to,
        }
    }
}

/// Remembers which primitive tool made a shape, for nicer editing handles.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PrimitiveHint {
    Rect { width: f64, height: f64, rotation: f64 },
    Circle { radius: f64 },
    Ellipse { rx: f64, ry: f64, rotation: f64 },
}

/// An outline (open or closed) built from segments. Units: millimetres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub start: Point,
    pub segments: Vec<Segment>,
    pub closed: bool,
    pub hint: Option<PrimitiveHint>,
}

/// Centre, radius and start angle of an arc, or `None` if it degenerates to a line.
pub(crate) fn arc_geometry(from: Point, to: Point, sweep: f64) -> Option<(Point, f64, f64)> {
    let chord = to - from;
    let c = chord.hypot();
    if !sweep.is_finite() || sweep.abs() < EPS || sweep.abs() >= std::f64::consts::TAU || c < EPS {
        return None;
    }
    let half = (sweep.abs() / 2.0).sin();
    if half.abs() < EPS {
        return None;
    }
    let r = c / (2.0 * half);
    let d = chord / c;
    let u = if sweep > 0.0 { Vec2::new(d.y, -d.x) } else { Vec2::new(-d.y, d.x) };
    let mid_angle = u.y.atan2(u.x);
    let start_angle = mid_angle - sweep / 2.0;
    let center = from - Vec2::new(start_angle.cos(), start_angle.sin()) * r;
    Some((center, r, start_angle))
}

impl Shape {
    pub fn polyline(points: &[Point], closed: bool) -> Shape {
        let start = points.first().copied().unwrap_or(Point::ZERO);
        let segments = points.iter().skip(1).map(|&to| Segment::Line { to }).collect();
        Shape { start, segments, closed, hint: None }
    }

    pub fn rect(origin: Point, width: f64, height: f64) -> Shape {
        let pts = [
            origin + Vec2::new(width, 0.0),
            origin + Vec2::new(width, height),
            origin + Vec2::new(0.0, height),
            origin,
        ];
        Shape {
            start: origin,
            segments: pts.iter().map(|&to| Segment::Line { to }).collect(),
            closed: true,
            hint: Some(PrimitiveHint::Rect { width, height, rotation: 0.0 }),
        }
    }

    pub fn circle(center: Point, radius: f64) -> Shape {
        let start = center + Vec2::new(radius, 0.0);
        let opposite = center - Vec2::new(radius, 0.0);
        let pi = std::f64::consts::PI;
        Shape {
            start,
            segments: vec![Segment::Arc { to: opposite, sweep: pi }, Segment::Arc { to: start, sweep: pi }],
            closed: true,
            hint: Some(PrimitiveHint::Circle { radius }),
        }
    }

    pub fn ellipse(center: Point, rx: f64, ry: f64) -> Shape {
        let (kx, ky) = (KAPPA * rx, KAPPA * ry);
        let c = center.to_vec2();
        let p = |x: f64, y: f64| Point::new(x, y) + c;
        let segments = vec![
            Segment::Cubic { c1: p(rx, ky), c2: p(kx, ry), to: p(0.0, ry) },
            Segment::Cubic { c1: p(-kx, ry), c2: p(-rx, ky), to: p(-rx, 0.0) },
            Segment::Cubic { c1: p(-rx, -ky), c2: p(-kx, -ry), to: p(0.0, -ry) },
            Segment::Cubic { c1: p(kx, -ry), c2: p(rx, -ky), to: p(rx, 0.0) },
        ];
        Shape { start: p(rx, 0.0), segments, closed: true, hint: Some(PrimitiveHint::Ellipse { rx, ry, rotation: 0.0 }) }
    }

    /// Start point plus every segment end, without a duplicate closing point.
    pub fn vertices(&self) -> Vec<Point> {
        let mut v = Vec::with_capacity(self.segments.len() + 1);
        v.push(self.start);
        v.extend(self.segments.iter().map(Segment::to));
        if self.closed && v.len() > 1 && v.last() == Some(&self.start) {
            v.pop();
        }
        v
    }

    pub fn to_bez_path(&self) -> BezPath {
        let mut path = BezPath::new();
        path.move_to(self.start);
        let mut current = self.start;
        for seg in &self.segments {
            match *seg {
                Segment::Line { to } => path.line_to(to),
                Segment::Arc { to, sweep } => match arc_geometry(current, to, sweep) {
                    Some((center, r, start_angle)) => {
                        let arc = kurbo::Arc::new(center, Vec2::new(r, r), start_angle, sweep, 0.0);
                        path.extend(arc.append_iter(ARC_TOLERANCE));
                        // land exactly on the stored end point
                        if let Some(PathEl::CurveTo(_, _, end)) = path.elements_mut().last_mut() {
                            *end = to;
                        }
                    }
                    None => path.line_to(to),
                },
                Segment::Cubic { c1, c2, to } => path.curve_to(c1, c2, to),
            }
            current = seg.to();
        }
        if self.closed {
            path.close_path();
        }
        path
    }

    /// Outline length, including the implicit closing edge when closed.
    pub fn length(&self) -> f64 {
        finite(self.to_bez_path().segments().map(|s| s.arclen(ACCURACY)).sum())
    }

    /// Enclosed area (absolute); 0 for open shapes.
    pub fn area(&self) -> f64 {
        if !self.closed {
            return 0.0;
        }
        finite(kurbo::Shape::area(&self.to_bez_path()).abs())
    }

    pub fn bounding_box(&self) -> Rect {
        if self.segments.is_empty() {
            return Rect::from_points(self.start, self.start);
        }
        kurbo::Shape::bounding_box(&self.to_bez_path())
    }

    /// Whether `p` is inside a closed shape (non-zero winding). Always false when open.
    pub fn contains(&self, p: Point) -> bool {
        self.closed && !self.segments.is_empty() && kurbo::Shape::winding(&self.to_bez_path(), p) != 0
    }

    /// Distance from `p` to the outline.
    pub fn distance_to(&self, p: Point) -> f64 {
        finite(self.nearest_point(p).distance(p))
    }

    /// Closest point on the outline to `p`.
    pub fn nearest_point(&self, p: Point) -> Point {
        let path = self.to_bez_path();
        path.segments()
            .map(|seg| {
                let n = seg.nearest(p, ACCURACY);
                (n.distance_sq, seg.eval(n.t))
            })
            .filter(|(d, pt)| d.is_finite() && pt.x.is_finite() && pt.y.is_finite())
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, pt)| pt)
            .unwrap_or(self.start)
    }

    /// Polyline approximation within `tolerance` mm (used by 3D).
    pub fn flatten(&self, tolerance: f64) -> Vec<Point> {
        let tol = if tolerance.is_finite() && tolerance > 1e-3 { tolerance } else { 1e-3 };
        let mut out = Vec::new();
        kurbo::flatten(self.to_bez_path(), tol, |el| match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => out.push(p),
            _ => {}
        });
        out.retain(|p| p.x.is_finite() && p.y.is_finite());
        out
    }
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-2
    }

    #[test]
    fn rect_measures() {
        let r = Shape::rect(Point::new(0., 0.), 4000., 3000.);
        assert!(approx(r.area(), 12_000_000.));
        assert!(approx(r.length(), 14_000.));
        assert!(r.contains(Point::new(100., 100.)));
        assert!(!r.contains(Point::new(-1., 0.)));
        assert_eq!(r.vertices().len(), 4);
    }

    #[test]
    fn circle_measures() {
        let c = Shape::circle(Point::new(0., 0.), 1000.);
        // arcs are flattened to cubics within 0.01 mm, so allow a few mm² on a 3 m² circle
        assert!((c.area() - std::f64::consts::PI * 1e6).abs() < 50.0);
        assert!((c.length() - 2. * std::f64::consts::PI * 1000.).abs() < 1.0);
    }

    #[test]
    fn arc_direction_follows_sweep_sign() {
        let s = Shape {
            start: Point::new(0., 0.),
            segments: vec![Segment::Arc { to: Point::new(2., 0.), sweep: std::f64::consts::PI }],
            closed: false,
            hint: None,
        };
        assert!(approx(s.length(), std::f64::consts::PI));
        assert!(approx(s.bounding_box().y0, -1.));
    }

    #[test]
    fn open_path_has_no_area_and_contains_nothing() {
        let p = Shape::polyline(&[Point::new(0., 0.), Point::new(1000., 0.), Point::new(1000., 1000.)], false);
        assert_eq!(p.area(), 0.);
        assert!(!p.contains(Point::new(900., 100.)));
        assert!(approx(p.distance_to(Point::new(500., 300.)), 300.));
    }

    #[test]
    fn ellipse_area() {
        assert!((Shape::ellipse(Point::ZERO, 2000., 1000.).area() - std::f64::consts::PI * 2e6).abs() < 2e3);
    }

    #[test]
    fn degenerate_shapes_are_finite() {
        let cases = [
            Shape::polyline(&[], true),
            Shape::polyline(&[Point::new(5., 5.)], true),
            Shape::polyline(&[Point::new(0., 0.), Point::new(0., 0.), Point::new(0., 0.)], true),
            Shape {
                start: Point::ZERO,
                segments: vec![
                    Segment::Arc { to: Point::ZERO, sweep: 1.0 },
                    Segment::Arc { to: Point::new(1., 0.), sweep: 0.0 },
                ],
                closed: false,
                hint: None,
            },
        ];
        for s in cases {
            for v in [s.length(), s.area(), s.distance_to(Point::new(3., 4.))] {
                assert!(v.is_finite());
            }
            let _ = s.contains(Point::new(1., 1.));
            assert!(s.flatten(0.5).iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        }
    }

    #[test]
    fn shape_serde_round_trip() {
        let c = Shape::circle(Point::new(10., 20.), 5.);
        let back: Shape = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
