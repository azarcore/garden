use kurbo::{Line, ParamCurve, ParamCurveNearest, Vec2};

use super::hit::object_shape;
use crate::geometry::Point;
use crate::model::{Document, ObjectId, ObjectKind};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapSettings {
    /// Grid spacing in mm, or `None` for no grid snapping.
    pub grid: Option<f64>,
    /// Snap to object corners and edges.
    pub objects: bool,
    /// How close (mm) the cursor must be to snap to an object.
    pub radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    None,
    Grid,
    Vertex,
    Edge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapResult {
    pub point: Point,
    pub kind: SnapKind,
}

fn closest(candidates: impl Iterator<Item = Point>, p: Point, radius: f64) -> Option<Point> {
    candidates
        .map(|c| (c.distance(p), c))
        .filter(|(d, _)| d.is_finite() && *d <= radius)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

/// Snap `p` to (in order of priority) a nearby vertex, a nearby edge, or the grid.
/// Objects in `exclude` (e.g. the ones being dragged) and hidden layers are ignored.
pub fn snap(doc: &Document, p: Point, settings: &SnapSettings, exclude: &[ObjectId]) -> SnapResult {
    if settings.objects {
        let objects: Vec<_> = doc
            .objects
            .iter()
            .filter(|o| !exclude.contains(&o.id) && doc.layer(o.layer).is_some_and(|l| l.visible))
            .collect();
        let vertices = objects.iter().flat_map(|o| match &o.kind {
            ObjectKind::Plant { position, .. } => vec![*position],
            ObjectKind::Dimension { from, to, .. } => vec![*from, *to],
            kind => object_shape(kind).map(|s| s.vertices()).unwrap_or_default(),
        });
        if let Some(v) = closest(vertices, p, settings.radius) {
            return SnapResult {
                point: v,
                kind: SnapKind::Vertex,
            };
        }
        let edges = objects.iter().filter_map(|o| match &o.kind {
            ObjectKind::Dimension { from, to, .. } => {
                let line = Line::new(*from, *to);
                Some(line.eval(line.nearest(p, 1e-6).t))
            }
            kind => object_shape(kind).map(|s| s.nearest_point(p)),
        });
        if let Some(e) = closest(edges, p, settings.radius) {
            return SnapResult {
                point: e,
                kind: SnapKind::Edge,
            };
        }
    }
    if let Some(g) = settings.grid.filter(|g| g.is_finite() && *g > 0.0) {
        let point = Point::new((p.x / g).round() * g, (p.y / g).round() * g);
        return SnapResult {
            point,
            kind: SnapKind::Grid,
        };
    }
    SnapResult {
        point: p,
        kind: SnapKind::None,
    }
}

/// Keep the distance from `origin` but round the direction to a multiple of `step` radians.
pub fn snap_angle(origin: Point, p: Point, step: f64) -> Point {
    if !step.is_finite() || step <= 0.0 {
        return p;
    }
    let d = p - origin;
    let angle = (d.y.atan2(d.x) / step).round() * step;
    origin + Vec2::new(angle.cos(), angle.sin()) * d.hypot()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Shape;
    use crate::model::{AreaType, ObjectKind};
    use crate::query::test_util::doc_with;

    #[test]
    fn snap_priority_vertex_edge_grid() {
        let d = doc_with(vec![ObjectKind::Area {
            area_type: AreaType::Bed,
            shape: Shape::rect(Point::new(1000., 1000.), 2000., 1000.),
        }]);
        let s = SnapSettings {
            grid: Some(500.),
            objects: true,
            radius: 100.,
        };
        assert_eq!(
            snap(&d, Point::new(1040., 1030.), &s, &[]),
            SnapResult {
                point: Point::new(1000., 1000.),
                kind: SnapKind::Vertex
            }
        );
        let e = snap(&d, Point::new(2000., 1060.), &s, &[]);
        assert_eq!(e.kind, SnapKind::Edge);
        assert!((e.point.y - 1000.).abs() < 1e-6);
        assert_eq!(
            snap(&d, Point::new(5240., 5260.), &s, &[]),
            SnapResult {
                point: Point::new(5000., 5500.),
                kind: SnapKind::Grid
            }
        );
        assert_eq!(
            snap(&d, Point::new(1040., 1030.), &s, &[d.objects[0].id]).kind,
            SnapKind::Grid
        );
    }

    #[test]
    fn no_snap_returns_point_unchanged() {
        let d = doc_with(vec![]);
        let s = SnapSettings {
            grid: None,
            objects: true,
            radius: 100.,
        };
        assert_eq!(
            snap(&d, Point::new(7., 9.), &s, &[]),
            SnapResult {
                point: Point::new(7., 9.),
                kind: SnapKind::None
            }
        );
    }

    #[test]
    fn snap_angle_to_15_degrees() {
        let p = snap_angle(Point::ZERO, Point::new(1000., 30.), 15f64.to_radians());
        assert!(p.y.abs() < 1e-9 && (p.x - 1000f64.hypot(30.)).abs() < 1e-9);
    }
}
