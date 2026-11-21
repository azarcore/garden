//! Transforms and exact dimension editing ("draw then dimension").

use std::f64::consts::{FRAC_PI_2, TAU};

use kurbo::{Affine, PathEl, Point, Vec2};

use super::shape::{PrimitiveHint, Segment, Shape, arc_geometry};

const ARC_TO_CUBIC_TOLERANCE: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Line,
    Arc,
    Cubic,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EdgeEditError {
    #[error("no such edge or corner")]
    OutOfRange,
    #[error("only straight edges can be dimensioned")]
    NotStraight,
    #[error("value out of range")]
    InvalidValue,
}

/// Replace an arc by cubic segments that follow it within tolerance.
fn arc_to_cubics(from: Point, to: Point, sweep: f64) -> Vec<Segment> {
    let Some((center, r, start_angle)) = arc_geometry(from, to, sweep) else {
        return vec![Segment::Line { to }];
    };
    let arc = kurbo::Arc::new(center, Vec2::new(r, r), start_angle, sweep, 0.0);
    let mut out: Vec<Segment> = arc
        .append_iter(ARC_TO_CUBIC_TOLERANCE)
        .filter_map(|el| match el {
            PathEl::CurveTo(c1, c2, to) => Some(Segment::Cubic { c1, c2, to }),
            _ => None,
        })
        .collect();
    match out.last_mut() {
        Some(Segment::Cubic { to: end, .. }) => *end = to,
        _ => out.push(Segment::Line { to }),
    }
    out
}

fn valid_positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

impl Shape {
    fn map_points(&self, a: Affine) -> Shape {
        let segments = self
            .segments
            .iter()
            .map(|s| match *s {
                Segment::Line { to } => Segment::Line { to: a * to },
                Segment::Arc { to, sweep } => Segment::Arc { to: a * to, sweep },
                Segment::Cubic { c1, c2, to } => Segment::Cubic {
                    c1: a * c1,
                    c2: a * c2,
                    to: a * to,
                },
            })
            .collect();
        Shape {
            start: a * self.start,
            segments,
            closed: self.closed,
            hint: self.hint,
        }
    }

    /// Copy of this shape with every arc replaced by cubics.
    fn arcs_as_cubics(&self) -> Shape {
        let mut current = self.start;
        let mut segments = Vec::with_capacity(self.segments.len());
        for s in &self.segments {
            match *s {
                Segment::Arc { to, sweep } => segments.extend(arc_to_cubics(current, to, sweep)),
                other => segments.push(other),
            }
            current = s.to();
        }
        Shape {
            segments,
            ..self.clone()
        }
    }

    pub fn translated(&self, d: Vec2) -> Shape {
        self.map_points(Affine::translate(d))
    }

    pub fn rotated(&self, center: Point, angle: f64) -> Shape {
        let mut s = self.map_points(Affine::rotate_about(angle, center));
        s.hint = match self.hint {
            Some(PrimitiveHint::Rect {
                width,
                height,
                rotation,
            }) => Some(PrimitiveHint::Rect {
                width,
                height,
                rotation: rotation + angle,
            }),
            Some(PrimitiveHint::Ellipse { rx, ry, rotation }) => Some(PrimitiveHint::Ellipse {
                rx,
                ry,
                rotation: rotation + angle,
            }),
            other => other,
        };
        s
    }

    /// Scale about `anchor`. Non-uniform scaling turns arcs into cubics.
    /// Non-finite factors leave the shape unchanged.
    pub fn scaled(&self, anchor: Point, sx: f64, sy: f64) -> Shape {
        if !sx.is_finite() || !sy.is_finite() {
            return self.clone();
        }
        let a = Affine::translate(anchor.to_vec2())
            * Affine::scale_non_uniform(sx, sy)
            * Affine::translate(-anchor.to_vec2());
        let uniform = sx == sy;
        let base = if uniform {
            self.clone()
        } else {
            self.arcs_as_cubics()
        };
        let mut s = base.map_points(a);
        let (ax, ay) = (sx.abs(), sy.abs());
        // Factors along a rotated primitive's own axes, if it stays that primitive.
        let local = |rotation: f64| -> Option<(f64, f64)> {
            if uniform {
                return Some((ax, ay));
            }
            let quarter_turns = rotation / FRAC_PI_2;
            if (quarter_turns - quarter_turns.round()).abs() > 1e-9 {
                return None; // a non-uniform scale skews it into a different shape
            }
            Some(if (quarter_turns.round() as i64).rem_euclid(2) == 1 {
                (ay, ax)
            } else {
                (ax, ay)
            })
        };
        s.hint = match self.hint {
            Some(PrimitiveHint::Rect {
                width,
                height,
                rotation,
            }) => local(rotation).map(|(fx, fy)| PrimitiveHint::Rect {
                width: width * fx,
                height: height * fy,
                rotation,
            }),
            Some(PrimitiveHint::Circle { radius }) if uniform => Some(PrimitiveHint::Circle {
                radius: radius * ax,
            }),
            Some(PrimitiveHint::Circle { radius }) => Some(PrimitiveHint::Ellipse {
                rx: radius * ax,
                ry: radius * ay,
                rotation: 0.0,
            }),
            Some(PrimitiveHint::Ellipse { rx, ry, rotation }) => {
                local(rotation).map(|(fx, fy)| PrimitiveHint::Ellipse {
                    rx: rx * fx,
                    ry: ry * fy,
                    rotation,
                })
            }
            None => None,
        };
        s
    }

    fn has_implicit_closing_edge(&self) -> bool {
        self.closed && self.segments.last().is_some_and(|s| s.to() != self.start)
    }

    pub fn edge_count(&self) -> usize {
        self.segments.len() + usize::from(self.has_implicit_closing_edge())
    }

    /// Start and end point of an edge, and whether it is straight.
    fn edge(&self, edge: usize) -> Option<(Point, Point, Option<&Segment>)> {
        let n = self.segments.len();
        let from = |i: usize| {
            if i == 0 {
                self.start
            } else {
                self.segments[i - 1].to()
            }
        };
        if edge < n {
            Some((
                from(edge),
                self.segments[edge].to(),
                Some(&self.segments[edge]),
            ))
        } else if edge == n && self.has_implicit_closing_edge() {
            Some((self.segments[n - 1].to(), self.start, None))
        } else {
            None
        }
    }

    fn edge_is_straight(&self, edge: usize) -> Option<bool> {
        self.edge(edge)
            .map(|(_, _, seg)| matches!(seg, None | Some(Segment::Line { .. })))
    }

    pub fn edge_length(&self, edge: usize) -> Option<f64> {
        let (a, b, seg) = self.edge(edge)?;
        match seg {
            None | Some(Segment::Line { .. }) => Some(a.distance(b)),
            Some(s) => Some(
                Shape {
                    start: a,
                    segments: vec![*s],
                    closed: false,
                    hint: None,
                }
                .length(),
            ),
        }
    }

    /// Move the end point of `edge` (the vertex shared with the next edge).
    fn set_edge_end(&mut self, edge: usize, p: Point) {
        let n = self.segments.len();
        let explicit_closing =
            self.closed && edge + 1 == n && self.segments[edge].to() == self.start;
        if edge == n || explicit_closing {
            self.start = p;
        }
        if edge < n {
            set_segment_end(&mut self.segments[edge], p);
        }
    }

    /// Incoming and outgoing edge indices of a vertex.
    fn corner_edges(&self, vertex: usize) -> Option<(usize, usize)> {
        let count = self.edge_count();
        if vertex >= self.vertices().len() || count == 0 {
            return None;
        }
        let incoming = if vertex > 0 {
            vertex - 1
        } else if self.closed {
            count - 1
        } else {
            return None;
        };
        if vertex >= count {
            return None;
        }
        Some((incoming, vertex))
    }

    /// +1 or -1 depending on which way the vertices wind (y-down shoelace).
    fn orientation(&self) -> f64 {
        let v = self.vertices();
        let twice_area: f64 = (0..v.len())
            .map(|i| {
                let (a, b) = (v[i], v[(i + 1) % v.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum();
        if twice_area < 0.0 { -1.0 } else { 1.0 }
    }

    /// Interior angle (radians, 0..2π) at `vertex` between two straight edges.
    pub fn corner_angle(&self, vertex: usize) -> Option<f64> {
        let (inc, out) = self.corner_edges(vertex)?;
        if !self.edge_is_straight(inc)? || !self.edge_is_straight(out)? {
            return None;
        }
        let (prev, v, _) = self.edge(inc)?;
        let (_, next, _) = self.edge(out)?;
        let (a, b) = (prev - v, next - v);
        if a.hypot() < 1e-9 || b.hypot() < 1e-9 {
            return None; // no direction to measure from
        }
        let theta = a.cross(b).atan2(a.dot(b));
        Some((-self.orientation() * theta).rem_euclid(TAU))
    }

    /// Make `edge` exactly `length` long by moving only its end vertex.
    pub fn set_edge_length(&self, edge: usize, length: f64) -> Result<Shape, EdgeEditError> {
        let (a, b, _) = self.edge(edge).ok_or(EdgeEditError::OutOfRange)?;
        if !self.edge_is_straight(edge).unwrap_or(false) {
            return Err(EdgeEditError::NotStraight);
        }
        if !valid_positive(length) {
            return Err(EdgeEditError::InvalidValue);
        }
        let dir = b - a;
        let dir = if dir.hypot() > 1e-12 {
            dir.normalize()
        } else {
            Vec2::new(1.0, 0.0)
        };
        let mut s = self.clone();
        s.set_edge_end(edge, a + dir * length);
        s.hint = None;
        Ok(s)
    }

    /// Set the interior angle at `vertex` by rotating the outgoing edge about it
    /// (keeping that edge's length); only the outgoing edge's end vertex moves.
    pub fn set_corner_angle(&self, vertex: usize, angle: f64) -> Result<Shape, EdgeEditError> {
        let (inc, out) = self.corner_edges(vertex).ok_or(EdgeEditError::OutOfRange)?;
        if !self.edge_is_straight(inc).unwrap_or(false)
            || !self.edge_is_straight(out).unwrap_or(false)
        {
            return Err(EdgeEditError::NotStraight);
        }
        if !valid_positive(angle) || angle >= TAU {
            return Err(EdgeEditError::InvalidValue);
        }
        let (prev, v, _) = self.edge(inc).ok_or(EdgeEditError::OutOfRange)?;
        let (_, next, _) = self.edge(out).ok_or(EdgeEditError::OutOfRange)?;
        let a = prev - v;
        let len = (next - v).hypot();
        let a_unit = if a.hypot() > 1e-12 {
            a.normalize()
        } else {
            Vec2::new(-1.0, 0.0)
        };
        let theta = -self.orientation() * angle;
        let b = Vec2::new(
            a_unit.x * theta.cos() - a_unit.y * theta.sin(),
            a_unit.x * theta.sin() + a_unit.y * theta.cos(),
        ) * len;
        let mut s = self.clone();
        s.set_edge_end(out, v + b);
        s.hint = None;
        // Some angles cannot be reached this way (e.g. a reflex corner in a
        // triangle flips the winding); refuse rather than silently differ.
        match s.corner_angle(vertex) {
            Some(got) if (got - angle).abs() < 1e-6 => Ok(s),
            _ => Err(EdgeEditError::InvalidValue),
        }
    }

    /// Change the kind of one segment, keeping its end points.
    pub fn convert_segment(&self, index: usize, to: SegmentKind) -> Result<Shape, EdgeEditError> {
        let mut s = self.clone();
        if index == s.segments.len() && s.has_implicit_closing_edge() {
            s.segments.push(Segment::Line { to: s.start });
        }
        let (from, end, seg) = match s.edge(index) {
            Some((a, b, Some(seg))) => (a, b, *seg),
            _ => return Err(EdgeEditError::OutOfRange),
        };
        let replacement = match (seg, to) {
            (_, SegmentKind::Line) => vec![Segment::Line { to: end }],
            (Segment::Arc { .. }, SegmentKind::Arc)
            | (Segment::Cubic { .. }, SegmentKind::Cubic) => vec![seg],
            (_, SegmentKind::Arc) => vec![Segment::Arc {
                to: end,
                sweep: std::f64::consts::FRAC_PI_2,
            }],
            (Segment::Arc { sweep, .. }, SegmentKind::Cubic) => arc_to_cubics(from, end, sweep),
            (_, SegmentKind::Cubic) => {
                let d = end - from;
                vec![Segment::Cubic {
                    c1: from + d / 3.0,
                    c2: from + d * (2.0 / 3.0),
                    to: end,
                }]
            }
        };
        s.segments.splice(index..=index, replacement);
        s.hint = None;
        Ok(s)
    }
}

/// Node edits: move, insert and remove points; move curve handles.
impl Shape {
    /// Copy whose closing edge (if any) is an explicit segment back to `start`.
    fn with_explicit_closing(&self) -> Shape {
        let mut s = self.clone();
        if s.has_implicit_closing_edge() {
            s.segments.push(Segment::Line { to: s.start });
        }
        s
    }

    /// Move vertex `i` (index as in `vertices()`); curve handles attached to it
    /// move with it.
    pub fn move_vertex(&self, i: usize, to: Point) -> Option<Shape> {
        let old = *self.vertices().get(i)?;
        let d = to - old;
        let mut s = self.clone();
        let n = s.segments.len();
        // the segment arriving at this vertex
        let incoming = if i > 0 {
            Some(i - 1)
        } else if s.closed && n > 0 && s.segments[n - 1].to() == s.start {
            Some(n - 1)
        } else {
            None
        };
        if let Some(k) = incoming {
            if let Segment::Cubic { c2, .. } = &mut s.segments[k] {
                *c2 += d;
            }
            set_segment_end(&mut s.segments[k], to);
        }
        if i == 0 {
            s.start = to;
        }
        // the segment leaving this vertex
        if let Some(Segment::Cubic { c1, .. }) = s.segments.get_mut(i) {
            *c1 += d;
        }
        s.hint = None;
        Some(s)
    }

    /// Move a cubic segment's first (`second == false`) or second control point.
    pub fn move_control(&self, segment: usize, second: bool, to: Point) -> Option<Shape> {
        let mut s = self.clone();
        match s.segments.get_mut(segment)? {
            Segment::Cubic { c1, c2, .. } => {
                if second {
                    *c2 = to;
                } else {
                    *c1 = to;
                }
            }
            _ => return None,
        }
        s.hint = None;
        Some(s)
    }

    /// Split the edge nearest `at` at its nearest point. Returns the new
    /// vertex index. None if that point is already a vertex.
    pub fn insert_vertex(&self, at: Point) -> Option<(Shape, usize)> {
        if !point_ok(at) {
            return None;
        }
        let mut s = self.with_explicit_closing();
        let mut best: Option<(f64, usize, f64)> = None; // (distance², segment, t)
        let mut current = s.start;
        for (k, seg) in s.segments.iter().enumerate() {
            let (t, p) = nearest_on_segment(current, seg, at);
            let d = p.distance_squared(at);
            if d.is_finite() && best.is_none_or(|b| d < b.0) {
                best = Some((d, k, t));
            }
            current = seg.to();
        }
        let (_, k, t) = best?;
        let from = if k == 0 {
            s.start
        } else {
            s.segments[k - 1].to()
        };
        let (first, second) = split_segment(from, &s.segments[k], t)?;
        if first.to().distance(from) < 1e-6 || first.to().distance(s.segments[k].to()) < 1e-6 {
            return None;
        }
        s.segments.splice(k..=k, [first, second]);
        s.hint = None;
        Some((s, k + 1))
    }

    /// Remove vertex `i`, joining its neighbours with a straight edge. None if
    /// too few points would remain (closed shapes keep 3, open lines 2).
    pub fn remove_vertex(&self, i: usize) -> Option<Shape> {
        let count = self.vertices().len();
        let min = if self.closed { 3 } else { 2 };
        if i >= count || count <= min {
            return None;
        }
        let mut s = if self.closed {
            self.with_explicit_closing()
        } else {
            self.clone()
        };
        let n = s.segments.len();
        if i == 0 {
            let new_start = s.segments[0].to();
            s.segments.remove(0);
            s.start = new_start;
            if s.closed {
                let last = s.segments.len() - 1;
                s.segments[last] = Segment::Line { to: new_start };
            }
        } else if i == n {
            // last point of an open line
            s.segments.pop();
        } else {
            let to = s.segments[i].to();
            s.segments.splice(i - 1..=i, [Segment::Line { to }]);
        }
        s.hint = None;
        Some(s)
    }
}

fn point_ok(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Parameter (0..1) and point on `seg` (starting at `from`) nearest `p`.
fn nearest_on_segment(from: Point, seg: &Segment, p: Point) -> (f64, Point) {
    use kurbo::{CubicBez, Line, ParamCurve, ParamCurveNearest};
    match *seg {
        Segment::Line { to } => {
            let l = Line::new(from, to);
            let t = l.nearest(p, 1e-6).t;
            (t, l.eval(t))
        }
        Segment::Cubic { c1, c2, to } => {
            let c = CubicBez::new(from, c1, c2, to);
            let t = c.nearest(p, 1e-6).t;
            (t, c.eval(t))
        }
        Segment::Arc { to, sweep } => match arc_geometry(from, to, sweep) {
            Some((center, r, start_angle)) => {
                let angle = (p.y - center.y).atan2(p.x - center.x);
                let along = ((angle - start_angle) * sweep.signum()).rem_euclid(TAU);
                let t = if along <= sweep.abs() {
                    along / sweep.abs()
                } else if along - sweep.abs() < TAU - along {
                    1.0
                } else {
                    0.0
                };
                let a = start_angle + sweep * t;
                (t, center + Vec2::new(a.cos(), a.sin()) * r)
            }
            None => nearest_on_segment(from, &Segment::Line { to }, p),
        },
    }
}

/// Split `seg` (starting at `from`) at parameter `t` into two pieces.
fn split_segment(from: Point, seg: &Segment, t: f64) -> Option<(Segment, Segment)> {
    use kurbo::{CubicBez, ParamCurve};
    if !(t > 0.0 && t < 1.0) {
        return None;
    }
    Some(match *seg {
        Segment::Line { to } => (
            Segment::Line {
                to: from.lerp(to, t),
            },
            Segment::Line { to },
        ),
        Segment::Cubic { c1, c2, to } => {
            let c = CubicBez::new(from, c1, c2, to);
            let (a, b) = (c.subsegment(0.0..t), c.subsegment(t..1.0));
            (
                Segment::Cubic {
                    c1: a.p1,
                    c2: a.p2,
                    to: a.p3,
                },
                Segment::Cubic {
                    c1: b.p1,
                    c2: b.p2,
                    to,
                },
            )
        }
        Segment::Arc { to, sweep } => {
            let (center, r, start_angle) = arc_geometry(from, to, sweep)?;
            let a = start_angle + sweep * t;
            let mid = center + Vec2::new(a.cos(), a.sin()) * r;
            (
                Segment::Arc {
                    to: mid,
                    sweep: sweep * t,
                },
                Segment::Arc {
                    to,
                    sweep: sweep * (1.0 - t),
                },
            )
        }
    })
}

fn set_segment_end(seg: &mut Segment, p: Point) {
    match seg {
        Segment::Line { to } | Segment::Arc { to, .. } | Segment::Cubic { to, .. } => *to = p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PrimitiveHint, Segment, Shape};
    use kurbo::Point;

    #[test]
    fn set_edge_lengths_then_angle_makes_exact_rectangle() {
        // a hand-drawn, slightly wrong quad
        let s = Shape::polyline(
            &[
                Point::new(0., 0.),
                Point::new(3900., 40.),
                Point::new(3950., 2980.),
                Point::new(-20., 3010.),
            ],
            true,
        );
        let s = s.set_edge_length(0, 4000.).unwrap();
        let s = s.set_corner_angle(1, std::f64::consts::FRAC_PI_2).unwrap();
        let s = s.set_edge_length(1, 3000.).unwrap();
        assert!((s.edge_length(0).unwrap() - 4000.).abs() < 1e-6);
        assert!((s.edge_length(1).unwrap() - 3000.).abs() < 1e-6);
        assert!((s.corner_angle(1).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert_eq!(s.vertices()[0], Point::new(0., 0.)); // first vertex never moves
    }

    #[test]
    fn closing_edge_counts() {
        assert_eq!(
            Shape::polyline(&[Point::ZERO, Point::new(1., 0.), Point::new(1., 1.)], true)
                .edge_count(),
            3
        );
    }

    #[test]
    fn closing_edge_of_rect_moves_start() {
        let r = Shape::rect(Point::ZERO, 10., 10.)
            .set_edge_length(3, 20.)
            .unwrap();
        assert_eq!(r.start, Point::new(0., -10.));
        assert_eq!(r.segments[3].to(), r.start);
    }

    #[test]
    fn edits_reject_curves_and_bad_values() {
        let c = Shape::circle(Point::ZERO, 10.);
        assert_eq!(c.set_edge_length(0, 5.), Err(EdgeEditError::NotStraight));
        let r = Shape::rect(Point::ZERO, 10., 10.);
        assert_eq!(r.set_edge_length(9, 5.), Err(EdgeEditError::OutOfRange));
        assert_eq!(r.set_edge_length(0, 0.), Err(EdgeEditError::InvalidValue));
        assert_eq!(
            r.set_edge_length(0, f64::NAN),
            Err(EdgeEditError::InvalidValue)
        );
        assert_eq!(r.set_corner_angle(1, 7.0), Err(EdgeEditError::InvalidValue));
    }

    #[test]
    fn non_uniform_scale_turns_circle_into_ellipse() {
        let e = Shape::circle(Point::ZERO, 1000.).scaled(Point::ZERO, 2., 1.);
        assert!(e.segments.iter().all(|s| !matches!(s, Segment::Arc { .. })));
        assert!(matches!(e.hint, Some(PrimitiveHint::Ellipse { .. })));
        assert!((e.area() - std::f64::consts::PI * 2e6).abs() < 2e3);
    }

    #[test]
    fn rotate_preserves_area_and_records_rotation() {
        let r = Shape::rect(Point::ZERO, 4000., 3000.).rotated(Point::ZERO, 0.3);
        assert!((r.area() - 12e6).abs() < 1.0);
        assert!(
            matches!(r.hint, Some(PrimitiveHint::Rect { rotation, .. }) if (rotation - 0.3).abs() < 1e-12)
        );
    }

    #[test]
    fn convert_line_to_cubic_keeps_endpoints() {
        let s = Shape::polyline(&[Point::ZERO, Point::new(300., 0.)], false)
            .convert_segment(0, SegmentKind::Cubic)
            .unwrap();
        assert!(matches!(s.segments[0], Segment::Cubic { to, .. } if to == Point::new(300., 0.)));
        assert!((s.length() - 300.).abs() < 1e-6);
    }

    #[test]
    fn translate_moves_every_point() {
        let s = Shape::circle(Point::ZERO, 5.).translated(kurbo::Vec2::new(10., 0.));
        assert_eq!(s.start, Point::new(15., 0.));
    }

    #[test]
    fn move_vertex_keeps_closing_point_together() {
        let r = Shape::rect(Point::ZERO, 1000., 1000.)
            .move_vertex(0, Point::new(-100., -100.))
            .unwrap();
        assert_eq!(r.start, Point::new(-100., -100.));
        assert_eq!(r.segments[3].to(), r.start);
        assert!(r.hint.is_none());
        assert!(
            Shape::rect(Point::ZERO, 1., 1.)
                .move_vertex(9, Point::ZERO)
                .is_none()
        );
    }

    #[test]
    fn controls_follow_their_vertex_and_move_alone() {
        let s = Shape {
            start: Point::ZERO,
            segments: vec![Segment::Cubic {
                c1: Point::new(100., 0.),
                c2: Point::new(900., 100.),
                to: Point::new(1000., 0.),
            }],
            closed: false,
            hint: None,
        };
        let m = s.move_vertex(1, Point::new(1000., 500.)).unwrap();
        assert!(matches!(m.segments[0], Segment::Cubic { c2, .. } if c2 == Point::new(900., 600.)));
        let c = s.move_control(0, false, Point::new(200., 300.)).unwrap();
        assert!(matches!(c.segments[0], Segment::Cubic { c1, .. } if c1 == Point::new(200., 300.)));
        assert!(
            Shape::rect(Point::ZERO, 1., 1.)
                .move_control(0, false, Point::ZERO)
                .is_none()
        );
    }

    #[test]
    fn insert_and_remove_points() {
        let r = Shape::rect(Point::ZERO, 1000., 1000.);
        let (s, i) = r.insert_vertex(Point::new(500., 10.)).unwrap();
        assert_eq!(i, 1);
        assert_eq!(s.vertices()[1], Point::new(500., 0.));
        assert!((s.area() - 1e6).abs() < 1e-6);
        let back = s.remove_vertex(1).unwrap();
        assert_eq!(back.vertices().len(), 4);
        assert!((back.area() - 1e6).abs() < 1e-6);
        let tri = Shape::polyline(&[Point::ZERO, Point::new(1., 0.), Point::new(0., 1.)], true);
        assert!(tri.remove_vertex(0).is_none());
        let line = Shape::polyline(&[Point::ZERO, Point::new(1., 0.)], false);
        assert!(line.remove_vertex(1).is_none());
    }

    #[test]
    fn removing_first_point_of_closed_shape_rejoins_it() {
        let r = Shape::rect(Point::ZERO, 1000., 1000.);
        let s = r.remove_vertex(0).unwrap();
        assert_eq!(s.vertices().len(), 3);
        assert!((s.area() - 5e5).abs() < 1e-6);
        let open = Shape::polyline(
            &[Point::ZERO, Point::new(1., 0.), Point::new(2., 0.)],
            false,
        );
        assert_eq!(open.remove_vertex(0).unwrap().start, Point::new(1., 0.));
        assert_eq!(open.remove_vertex(2).unwrap().vertices().len(), 2);
    }

    #[test]
    fn inserting_on_a_curve_keeps_its_shape() {
        let c = Shape::circle(Point::ZERO, 1000.);
        let (s, _) = c.insert_vertex(Point::new(0., 1000.)).unwrap();
        assert_eq!(s.segments.len(), 3);
        assert!((s.area() - c.area()).abs() < 1.0);
        let e = Shape::ellipse(Point::ZERO, 2000., 1000.);
        let (s, _) = e.insert_vertex(Point::new(1500., 700.)).unwrap();
        assert_eq!(s.segments.len(), 5);
        assert!((s.area() - e.area()).abs() < 1.0);
    }

    #[test]
    fn inserting_on_an_existing_point_does_nothing() {
        assert!(
            Shape::rect(Point::ZERO, 1000., 1000.)
                .insert_vertex(Point::new(1000., 0.))
                .is_none()
        );
    }
}
