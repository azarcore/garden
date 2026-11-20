//! Smart guides: alignment, equal spacing and distances between objects'
//! bounding boxes while moving or drawing.

use garden_core::geometry::Point;
use garden_core::units::{UnitSystem, format_length};
use kurbo::{Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideKind {
    /// Edges or centres lined up.
    Align,
    /// A measured gap to a neighbour or the plot edge.
    Distance,
    /// One of two equal gaps.
    Equal,
}

/// A line to draw over the plan (world mm), with an optional label.
#[derive(Clone, Debug, PartialEq)]
pub struct GuideLine {
    pub kind: GuideKind,
    pub from: Point,
    pub to: Point,
    pub label: Option<String>,
}

/// Extra offset to snap a moving selection, and the lines that explain it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MoveGuides {
    pub shift: Vec2,
    pub lines: Vec<GuideLine>,
    pub aligned_x: bool,
    pub aligned_y: bool,
}

/// One axis of a rectangle: (min, max).
#[derive(Clone, Copy)]
struct Span(f64, f64);

impl Span {
    fn stops(self) -> [f64; 3] {
        [self.0, (self.0 + self.1) / 2.0, self.1]
    }
    fn overlap(self, o: Span) -> Option<Span> {
        let (a, b) = (self.0.max(o.0), self.1.min(o.1));
        (b > a).then_some(Span(a, b))
    }
    fn mid(self) -> f64 {
        (self.0 + self.1) / 2.0
    }
}

fn xs(r: Rect) -> Span {
    Span(r.x0, r.x1)
}

fn ys(r: Rect) -> Span {
    Span(r.y0, r.y1)
}

/// (span on the axis being measured, span on the other axis).
fn axes(r: Rect, vertical_axis: bool) -> (Span, Span) {
    if vertical_axis {
        (ys(r), xs(r))
    } else {
        (xs(r), ys(r))
    }
}

/// Closest match (within `tol`) of any of `value`'s stops to any stop of the
/// candidates: (offset to apply, matched coordinate, candidate index).
/// Ties go to the earliest candidate.
fn best_match(stops: &[f64], candidates: &[Span], tol: f64) -> Option<(f64, f64, usize)> {
    let mut best: Option<(f64, f64, usize)> = None;
    for (i, c) in candidates.iter().enumerate() {
        for target in c.stops() {
            for &v in stops {
                let d = target - v;
                if d.abs() <= tol && best.is_none_or(|b| d.abs() < b.0.abs()) {
                    best = Some((d, target, i));
                }
            }
        }
    }
    best
}

/// A guide line: vertical at x = `at` when `vertical`, else horizontal at y = `at`.
fn line(
    kind: GuideKind,
    vertical: bool,
    at: f64,
    from: f64,
    to: f64,
    label: Option<String>,
) -> GuideLine {
    let (from, to) = if vertical {
        (Point::new(at, from), Point::new(at, to))
    } else {
        (Point::new(from, at), Point::new(to, at))
    };
    GuideLine {
        kind,
        from,
        to,
        label,
    }
}

/// The nearest neighbour wholly before (`before`) or after the span on this
/// axis whose span on the other axis overlaps: (its near edge, overlap).
fn neighbour(moving: (Span, Span), others: &[(Span, Span)], before: bool) -> Option<(f64, Span)> {
    let (m, m_other) = moving;
    others
        .iter()
        .filter_map(|&(o, o_other)| {
            let overlap = m_other.overlap(o_other)?;
            if before && o.1 <= m.0 {
                Some((o.1, overlap))
            } else if !before && o.0 >= m.1 {
                Some((o.0, overlap))
            } else {
                None
            }
        })
        .min_by(|a, b| {
            let da = if before { m.0 - a.0 } else { a.0 - m.1 };
            let db = if before { m.0 - b.0 } else { b.0 - m.1 };
            da.total_cmp(&db)
        })
}

/// Alignment and equal-spacing snaps for `moving` among `others` (tolerance
/// `tol` mm), plus the guide lines to draw. `shift` is the extra offset to apply.
pub fn move_guides(moving: Rect, others: &[Rect], tol: f64) -> MoveGuides {
    let mut g = MoveGuides::default();
    let ox: Vec<Span> = others.iter().map(|r| xs(*r)).collect();
    let oy: Vec<Span> = others.iter().map(|r| ys(*r)).collect();
    let ax = best_match(&xs(moving).stops(), &ox, tol);
    let ay = best_match(&ys(moving).stops(), &oy, tol);
    if let Some((d, _, _)) = ax {
        g.shift.x = d;
        g.aligned_x = true;
    }
    if let Some((d, _, _)) = ay {
        g.shift.y = d;
        g.aligned_y = true;
    }
    // equal spacing on an axis that isn't aligned
    let pairs = |vertical_axis: bool| -> Vec<(Span, Span)> {
        others.iter().map(|r| axes(*r, vertical_axis)).collect()
    };
    for vertical_axis in [false, true] {
        let aligned = if vertical_axis {
            g.aligned_y
        } else {
            g.aligned_x
        };
        if aligned {
            continue;
        }
        let m = moving + g.shift;
        let mv = axes(m, vertical_axis);
        let os = pairs(vertical_axis);
        let (Some((before, ob)), Some((after, oa))) =
            (neighbour(mv, &os, true), neighbour(mv, &os, false))
        else {
            continue;
        };
        let (gap_before, gap_after) = (mv.0.0 - before, after - mv.0.1);
        let d = (gap_after - gap_before) / 2.0;
        if d.abs() > tol {
            continue;
        }
        let (start, end) = (mv.0.0 + d, mv.0.1 + d);
        // horizontal gaps are drawn as horizontal lines, and vice versa
        g.lines.push(line(
            GuideKind::Equal,
            vertical_axis,
            ob.mid(),
            before,
            start,
            None,
        ));
        g.lines.push(line(
            GuideKind::Equal,
            vertical_axis,
            oa.mid(),
            end,
            after,
            None,
        ));
        if vertical_axis {
            g.shift.y += d;
        } else {
            g.shift.x += d;
        }
    }
    let m = moving + g.shift;
    if let Some((_, at, i)) = ax {
        let o = others[i];
        g.lines.push(line(
            GuideKind::Align,
            true,
            at,
            m.y0.min(o.y0),
            m.y1.max(o.y1),
            None,
        ));
    }
    if let Some((_, at, i)) = ay {
        let o = others[i];
        g.lines.push(line(
            GuideKind::Align,
            false,
            at,
            m.x0.min(o.x0),
            m.x1.max(o.x1),
            None,
        ));
    }
    g
}

/// Distance lines from each side of `moving` to the nearest neighbour on that
/// side whose span overlaps it on the other axis, or to the inside of
/// `container` (the plot).
pub fn distance_guides(
    moving: Rect,
    others: &[Rect],
    container: Option<Rect>,
    units: UnitSystem,
) -> Vec<GuideLine> {
    let inside = container.filter(|c| c.contains_rect(moving));
    let mut lines = Vec::new();
    for vertical_axis in [false, true] {
        let mv = axes(moving, vertical_axis);
        let os: Vec<(Span, Span)> = others.iter().map(|r| axes(*r, vertical_axis)).collect();
        for before in [true, false] {
            let edge = if before { mv.0.0 } else { mv.0.1 };
            let target = neighbour(mv, &os, before).or_else(|| {
                let c = inside?;
                let cs = if vertical_axis { ys(c) } else { xs(c) };
                Some((if before { cs.0 } else { cs.1 }, mv.1))
            });
            let Some((to, across)) = target else { continue };
            let gap = (to - edge).abs();
            if gap <= 1e-6 {
                continue;
            }
            lines.push(line(
                GuideKind::Distance,
                vertical_axis,
                across.mid(),
                edge,
                to,
                Some(format_length(gap, units)),
            ));
        }
    }
    lines
}

/// Align a drawing point to others' left/centre/right and top/middle/bottom.
pub fn point_guides(p: Point, others: &[Rect], tol: f64) -> (Point, Vec<GuideLine>) {
    let ox: Vec<Span> = others.iter().map(|r| xs(*r)).collect();
    let oy: Vec<Span> = others.iter().map(|r| ys(*r)).collect();
    let mut out = p;
    let mut lines = Vec::new();
    if let Some((d, at, i)) = best_match(&[p.x], &ox, tol) {
        out.x += d;
        let o = others[i];
        lines.push(line(
            GuideKind::Align,
            true,
            at,
            p.y.min(o.y0),
            p.y.max(o.y1),
            None,
        ));
    }
    if let Some((d, at, i)) = best_match(&[p.y], &oy, tol) {
        out.y += d;
        let o = others[i];
        lines.push(line(
            GuideKind::Align,
            false,
            at,
            out.x.min(o.x0),
            out.x.max(o.x1),
            None,
        ));
    }
    (out, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use garden_core::geometry::Point;
    use garden_core::units::UnitSystem;
    use kurbo::{Rect, Vec2};

    fn r(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::new(x0, y0, x1, y1)
    }

    #[test]
    fn left_edges_align_within_tolerance() {
        let g = move_guides(
            r(1040., 3000., 2040., 3500.),
            &[r(1000., 0., 2500., 1000.)],
            100.,
        );
        assert_eq!(g.shift, Vec2::new(-40., 0.));
        assert!(g.aligned_x && !g.aligned_y);
        let line = g.lines.iter().find(|l| l.kind == GuideKind::Align).unwrap();
        assert_eq!((line.from.x, line.to.x), (1000., 1000.));
        assert_eq!((line.from.y, line.to.y), (0., 3500.));
    }

    #[test]
    fn centres_align_too_and_far_things_dont() {
        let g = move_guides(
            r(0., 960., 400., 1360.),
            &[r(5000., 1000., 5600., 1400.)],
            100.,
        );
        assert_eq!(g.shift, Vec2::new(0., 40.));
        assert_eq!(
            move_guides(
                r(0., 0., 100., 100.),
                &[r(5000., 5000., 5100., 5100.)],
                100.
            ),
            MoveGuides::default()
        );
    }

    #[test]
    fn equal_gaps_between_two_neighbours() {
        let g = move_guides(
            r(1450., 0., 2450., 500.),
            &[r(0., 0., 1000., 500.), r(3000., 0., 4000., 500.)],
            100.,
        );
        assert_eq!(g.shift.x, 50.);
        let equal: Vec<_> = g
            .lines
            .iter()
            .filter(|l| l.kind == GuideKind::Equal)
            .collect();
        assert_eq!(equal.len(), 2);
        assert_eq!(equal[0].label, equal[1].label);
    }

    #[test]
    fn distances_to_neighbours_and_the_plot_edge() {
        let plot = r(0., 0., 10_000., 6_000.);
        let lines = distance_guides(
            r(2000., 2000., 3000., 3000.),
            &[r(4200., 2500., 5000., 3500.)],
            Some(plot),
            UnitSystem::Metric,
        );
        let labels: Vec<_> = lines.iter().filter_map(|l| l.label.clone()).collect();
        assert!(labels.contains(&"1.20 m".to_string()));
        assert!(labels.contains(&"2.00 m".to_string()));
        assert!(labels.contains(&"3.00 m".to_string()));
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|l| l.kind == GuideKind::Distance));
    }

    #[test]
    fn overlapping_or_missing_neighbours_give_no_bad_lines() {
        assert!(
            distance_guides(
                r(0., 0., 100., 100.),
                &[r(50., 50., 150., 150.)],
                None,
                UnitSystem::Metric
            )
            .is_empty()
        );
        assert!(distance_guides(r(0., 0., 100., 100.), &[], None, UnitSystem::Metric).is_empty());
        assert_eq!(
            move_guides(r(0., 0., 100., 100.), &[], 100.),
            MoveGuides::default()
        );
        // outside the plot: no distance to its edge
        assert!(
            distance_guides(
                r(-500., 0., -100., 100.),
                &[],
                Some(r(0., 0., 1000., 1000.)),
                UnitSystem::Metric
            )
            .is_empty()
        );
    }

    #[test]
    fn drawing_points_align_to_edges_and_centres() {
        let (p, lines) = point_guides(
            Point::new(1030., 2980.),
            &[r(1000., 0., 3000., 1000.)],
            100.,
        );
        assert_eq!(p, Point::new(1000., 2980.));
        assert_eq!(lines.len(), 1);
        let (p, lines) = point_guides(
            Point::new(9000., 9000.),
            &[r(1000., 0., 3000., 1000.)],
            100.,
        );
        assert_eq!(p, Point::new(9000., 9000.));
        assert!(lines.is_empty());
    }
}
