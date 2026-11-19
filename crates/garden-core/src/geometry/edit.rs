//! Transforms and exact dimension editing ("draw then dimension").

use std::f64::consts::TAU;

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
    pub fn scaled(&self, anchor: Point, sx: f64, sy: f64) -> Shape {
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
        s.hint = match self.hint {
            Some(PrimitiveHint::Rect {
                width,
                height,
                rotation,
            }) => Some(PrimitiveHint::Rect {
                width: width * ax,
                height: height * ay,
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
            Some(PrimitiveHint::Ellipse { rx, ry, rotation }) => Some(PrimitiveHint::Ellipse {
                rx: rx * ax,
                ry: ry * ay,
                rotation,
            }),
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
        Ok(s)
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
}
