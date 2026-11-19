//! Small helpers over object kinds used by tools.

use garden_core::geometry::Point;
use garden_core::model::ObjectKind;
use garden_core::query::dimension_line;
use kurbo::{Rect, Vec2};

use crate::strings::{EN, kind_label};

/// A copy of `kind` with every point moved by `d`.
pub fn translate_kind(kind: &ObjectKind, d: Vec2) -> ObjectKind {
    let mut k = kind.clone();
    match &mut k {
        ObjectKind::PlotBoundary { shape }
        | ObjectKind::Area { shape, .. }
        | ObjectKind::Path { shape, .. }
        | ObjectKind::Linear { shape, .. }
        | ObjectKind::Structure {
            footprint: shape, ..
        } => *shape = shape.translated(d),
        ObjectKind::Plant { position, .. } | ObjectKind::Label { position, .. } => *position += d,
        ObjectKind::Dimension { from, to, .. } => {
            *from += d;
            *to += d;
        }
    }
    k
}

/// Axis-aligned bounds including stroke widths and plant spread.
pub fn object_bounds(kind: &ObjectKind) -> Rect {
    let around = |p: Point, r: f64| Rect::new(p.x - r, p.y - r, p.x + r, p.y + r);
    match kind {
        ObjectKind::PlotBoundary { shape } | ObjectKind::Area { shape, .. } => shape.bounding_box(),
        ObjectKind::Structure { footprint, .. } => footprint.bounding_box(),
        ObjectKind::Path { shape, width } => shape.bounding_box().inflate(width / 2.0, width / 2.0),
        ObjectKind::Linear {
            shape, thickness, ..
        } => shape
            .bounding_box()
            .inflate(thickness / 2.0, thickness / 2.0),
        ObjectKind::Plant {
            position, spread, ..
        } => around(*position, spread / 2.0),
        ObjectKind::Label { position, size, .. } => around(*position, *size),
        ObjectKind::Dimension { from, to, offset } => {
            let (a, b) = dimension_line(*from, *to, *offset);
            Rect::from_points(*from, *to).union_pt(a).union_pt(b)
        }
    }
}

/// Corner points of an object (what snapping and anchoring use).
pub fn object_vertices(kind: &ObjectKind) -> Vec<Point> {
    match kind {
        ObjectKind::PlotBoundary { shape }
        | ObjectKind::Area { shape, .. }
        | ObjectKind::Path { shape, .. }
        | ObjectKind::Linear { shape, .. }
        | ObjectKind::Structure {
            footprint: shape, ..
        } => shape.vertices(),
        ObjectKind::Plant { position, .. } | ObjectKind::Label { position, .. } => vec![*position],
        ObjectKind::Dimension { from, to, .. } => vec![*from, *to],
    }
}

/// Name given to newly created objects.
pub fn default_name(kind: &ObjectKind) -> &'static str {
    kind_label(&EN, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::all_kinds_sample;
    use garden_core::geometry::Shape;

    #[test]
    fn translate_moves_every_kind() {
        for k in all_kinds_sample() {
            let b0 = object_bounds(&k);
            let b1 = object_bounds(&translate_kind(&k, Vec2::new(100., -50.)));
            assert!(
                (b1.x0 - b0.x0 - 100.).abs() < 1e-6 && (b1.y0 - b0.y0 + 50.).abs() < 1e-6,
                "{k:?}"
            );
        }
    }

    #[test]
    fn bounds_include_stroke_width() {
        let k = ObjectKind::Path {
            shape: Shape::polyline(&[Point::ZERO, Point::new(1000., 0.)], false),
            width: 900.,
        };
        assert_eq!(object_bounds(&k), Rect::new(-450., -450., 1450., 450.));
    }
}
