use kurbo::{Line, ParamCurveNearest};

use crate::geometry::{Point, Shape};
use crate::model::{Document, Object, ObjectId, ObjectKind};

pub(crate) fn segment_distance(a: Point, b: Point, p: Point) -> f64 {
    Line::new(a, b).nearest(p, 1e-6).distance_sq.sqrt()
}

/// Where a dimension line is drawn: the measured segment shifted by `offset` mm
/// along the unit normal `(-dy, dx) / len` of `from → to`.
pub fn dimension_line(from: Point, to: Point, offset: f64) -> (Point, Point) {
    let d = to - from;
    let len = d.hypot();
    if len < 1e-9 {
        return (from, to);
    }
    let normal = kurbo::Vec2::new(-d.y, d.x) / len;
    (from + normal * offset, to + normal * offset)
}

/// The shape an object is drawn from, if it has one.
pub(crate) fn object_shape(kind: &ObjectKind) -> Option<&Shape> {
    match kind {
        ObjectKind::PlotBoundary { shape }
        | ObjectKind::Area { shape, .. }
        | ObjectKind::Path { shape, .. }
        | ObjectKind::Linear { shape, .. } => Some(shape),
        ObjectKind::Structure { footprint, .. } => Some(footprint),
        _ => None,
    }
}

fn hits(object: &Object, p: Point, tol: f64) -> bool {
    match &object.kind {
        ObjectKind::PlotBoundary { shape } => shape.distance_to(p) <= tol,
        ObjectKind::Area { shape, .. }
        | ObjectKind::Structure {
            footprint: shape, ..
        } => shape.contains(p) || shape.distance_to(p) <= tol,
        ObjectKind::Path { shape, width } => shape.distance_to(p) <= width / 2.0 + tol,
        ObjectKind::Linear {
            shape, thickness, ..
        } => shape.distance_to(p) <= thickness / 2.0 + tol,
        ObjectKind::Plant {
            position, spread, ..
        } => position.distance(p) <= spread / 2.0 + tol,
        ObjectKind::Label { position, size, .. } => position.distance(p) <= size + tol,
        ObjectKind::Dimension { from, to, offset } => {
            let (a, b) = dimension_line(*from, *to, *offset);
            segment_distance(a, b, p) <= tol
        }
    }
}

/// The topmost selectable object at `p` (within `tolerance` mm).
/// Locked objects and objects on hidden or locked layers are never hit;
/// the plot boundary is hit only near its outline.
pub fn hit_test(doc: &Document, p: Point, tolerance: f64) -> Option<ObjectId> {
    hit(doc, p, tolerance, false)
}

/// Like [`hit_test`], but locked objects can be hit too, so they can be
/// selected (e.g. to unlock them). Hidden and locked layers are still skipped.
pub fn hit_test_selectable(doc: &Document, p: Point, tolerance: f64) -> Option<ObjectId> {
    hit(doc, p, tolerance, true)
}

fn hit(doc: &Document, p: Point, tolerance: f64, include_locked: bool) -> Option<ObjectId> {
    doc.objects
        .iter()
        .rev()
        .filter(|o| include_locked || !o.locked)
        .filter(|o| doc.layer(o.layer).is_some_and(|l| l.visible && !l.locked))
        .find(|o| hits(o, p, tolerance))
        .map(|o| o.id)
}

/// The topmost object at `p` (within `tolerance` mm) that passes `filter`,
/// on a visible layer. Locks are ignored: for picking things that can't move.
pub fn hit_test_where(
    doc: &Document,
    p: Point,
    tolerance: f64,
    filter: impl Fn(&Object) -> bool,
) -> Option<ObjectId> {
    doc.objects
        .iter()
        .rev()
        .filter(|o| filter(o))
        .filter(|o| doc.layer(o.layer).is_some_and(|l| l.visible))
        .find(|o| hits(o, p, tolerance))
        .map(|o| o.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::geometry::{Point, Shape};
    use crate::model::{AreaType, ObjectKind, PlantCategory, PlantSymbol};
    use crate::query::test_util::doc_with;

    #[test]
    fn clicking_inside_plot_does_not_select_boundary() {
        let d = doc_with(vec![ObjectKind::PlotBoundary {
            shape: Shape::rect(Point::ZERO, 10_000., 10_000.),
        }]);
        assert_eq!(hit_test(&d, Point::new(5000., 5000.), 50.), None);
        assert_eq!(
            hit_test(&d, Point::new(20., 5000.), 50.),
            Some(d.objects[0].id)
        );
    }

    #[test]
    fn topmost_wins_and_locked_is_skipped() {
        let mut d = doc_with(vec![
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: Shape::rect(Point::ZERO, 5000., 5000.),
            },
            ObjectKind::Plant {
                position: Point::new(1000., 1000.),
                spread: 600.,
                height: 500.,
                symbol: PlantSymbol::Round,
                category: PlantCategory::Shrub,
                catalog_id: None,
            },
        ]);
        let (lawn, plant) = (d.objects[0].id, d.objects[1].id);
        assert_eq!(hit_test(&d, Point::new(1100., 1000.), 10.), Some(plant));
        let mut p = d.objects[1].clone();
        p.locked = true;
        d.apply(Command::Replace { object: p }, 1).unwrap();
        assert_eq!(hit_test(&d, Point::new(1100., 1000.), 10.), Some(lawn));
        let mut layer = d.layers[0].clone();
        layer.visible = false;
        d.apply(Command::UpsertLayer { layer }, 2).unwrap();
        assert_eq!(hit_test(&d, Point::new(1100., 1000.), 10.), None);
    }

    #[test]
    fn path_hit_uses_width() {
        let d = doc_with(vec![ObjectKind::Path {
            shape: Shape::polyline(&[Point::ZERO, Point::new(5000., 0.)], false),
            width: 1000.,
        }]);
        assert!(hit_test(&d, Point::new(2500., 480.), 10.).is_some());
        assert!(hit_test(&d, Point::new(2500., 600.), 10.).is_none());
    }

    #[test]
    fn selectable_hit_includes_locked_objects() {
        let mut d = doc_with(vec![ObjectKind::Plant {
            position: Point::new(1000., 1000.),
            spread: 600.,
            height: 500.,
            symbol: PlantSymbol::Round,
            category: PlantCategory::Shrub,
            catalog_id: None,
        }]);
        let mut p = d.objects[0].clone();
        p.locked = true;
        d.apply(Command::Replace { object: p }, 1).unwrap();
        assert_eq!(hit_test(&d, Point::new(1000., 1000.), 10.), None);
        assert_eq!(
            hit_test_selectable(&d, Point::new(1000., 1000.), 10.),
            Some(d.objects[0].id)
        );
    }
}
