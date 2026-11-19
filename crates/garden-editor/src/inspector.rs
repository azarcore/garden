//! Which properties the inspector shows for an object, and how typed text is applied.

use garden_core::geometry::{EdgeEditError, Segment, SegmentKind, Shape};
use garden_core::model::{Document, Object, ObjectKind};
use garden_core::units::{UnitSystem, format_area, format_length, parse_length};

use crate::objects::{kind_shape, with_shape};
use crate::strings::{EN, Strings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Width,
    Thickness,
    Height,
    Spread,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldError {
    Empty,
    NotALength,
    MustBePositive,
    NotStraight,
    NotAnAngle,
    AngleNotPossible,
}

impl FieldError {
    pub fn message(&self, s: &Strings) -> &'static str {
        match self {
            FieldError::Empty => s.err_empty,
            FieldError::NotALength => s.err_not_a_length,
            FieldError::MustBePositive => s.err_must_be_positive,
            FieldError::NotStraight => s.err_not_straight,
            FieldError::NotAnAngle => s.err_not_an_angle,
            FieldError::AngleNotPossible => s.err_angle_not_possible,
        }
    }
}

pub fn inspector_fields(kind: &ObjectKind) -> Vec<Field> {
    use Field::*;
    match kind {
        ObjectKind::Path { .. } => vec![Name, Width],
        ObjectKind::Linear { .. } => vec![Name, Thickness, Height],
        ObjectKind::Structure { .. } => vec![Name, Height],
        ObjectKind::Plant { .. } => vec![Name, Spread, Height],
        ObjectKind::Label { .. } => vec![Name, Text],
        ObjectKind::PlotBoundary { .. }
        | ObjectKind::Area { .. }
        | ObjectKind::Dimension { .. } => vec![Name],
    }
}

/// The length a field edits, if it is a length field on this kind.
fn length_slot(kind: &mut ObjectKind, field: Field) -> Option<&mut f64> {
    match (kind, field) {
        (ObjectKind::Path { width, .. }, Field::Width) => Some(width),
        (ObjectKind::Linear { thickness, .. }, Field::Thickness) => Some(thickness),
        (ObjectKind::Linear { height, .. }, Field::Height)
        | (ObjectKind::Structure { height, .. }, Field::Height)
        | (ObjectKind::Plant { height, .. }, Field::Height) => Some(height),
        (ObjectKind::Plant { spread, .. }, Field::Spread) => Some(spread),
        _ => None,
    }
}

/// Read-only quantities for an object: area of surfaces, length of lines,
/// and how many of a plant the plan contains (label, value).
pub fn measures(doc: &Document, object: &Object, units: UnitSystem) -> Vec<(&'static str, String)> {
    match &object.kind {
        ObjectKind::PlotBoundary { shape } | ObjectKind::Area { shape, .. } => {
            vec![(EN.measure_area, format_area(shape.area(), units))]
        }
        ObjectKind::Structure { footprint, .. } => {
            vec![(EN.measure_area, format_area(footprint.area(), units))]
        }
        ObjectKind::Path { shape, .. } | ObjectKind::Linear { shape, .. } => {
            vec![(EN.measure_length, format_length(shape.length(), units))]
        }
        ObjectKind::Plant { catalog_id, .. } => {
            let count = match catalog_id {
                Some(id) => doc
                    .objects
                    .iter()
                    .filter(|o| matches!(&o.kind, ObjectKind::Plant { catalog_id: Some(c), .. } if c == id))
                    .count(),
                None => 1,
            };
            vec![(EN.measure_in_plan, count.to_string())]
        }
        ObjectKind::Label { .. } | ObjectKind::Dimension { .. } => Vec::new(),
    }
}

/// Current value of a field as display text.
pub fn field_value(object: &Object, field: Field, units: UnitSystem) -> String {
    match (field, &object.kind) {
        (Field::Name, _) => object.name.clone(),
        (Field::Text, ObjectKind::Label { text, .. }) => text.clone(),
        _ => {
            let mut kind = object.kind.clone();
            length_slot(&mut kind, field)
                .map(|v| format_length(*v, units))
                .unwrap_or_default()
        }
    }
}

/// A copy of `object` with `input` applied to `field`, or why it can't be.
pub fn apply_field(
    object: &Object,
    field: Field,
    input: &str,
    units: UnitSystem,
) -> Result<Object, FieldError> {
    let mut out = object.clone();
    match field {
        Field::Name | Field::Text => {
            let text = input.trim();
            if text.is_empty() {
                return Err(FieldError::Empty);
            }
            match (&mut out.kind, field) {
                (ObjectKind::Label { text: t, .. }, Field::Text) => *t = text.to_string(),
                _ => out.name = text.to_string(),
            }
        }
        _ => {
            let value = parse_length(input, units).map_err(|_| FieldError::NotALength)?;
            if value <= 0.0 {
                return Err(FieldError::MustBePositive);
            }
            if let Some(slot) = length_slot(&mut out.kind, field) {
                *slot = value;
            }
        }
    }
    Ok(out)
}

/// One edge of an outline as the inspector lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeRow {
    pub index: usize,
    pub length: String,
    pub kind: SegmentKind,
    /// The corner at the edge's end, e.g. "90°" (None when an edge there is curved,
    /// or after the last edge of an open line).
    pub angle: Option<String>,
}

/// Degrees with at most one decimal, e.g. "90°", "89.5°".
fn format_angle(radians: f64) -> String {
    let text = format!("{:.1}", radians.to_degrees());
    format!("{}°", text.strip_suffix(".0").unwrap_or(&text))
}

/// Every edge of the object's outline (empty for objects without one).
pub fn edge_rows(object: &Object, units: UnitSystem) -> Vec<EdgeRow> {
    let Some(shape) = kind_shape(&object.kind) else {
        return Vec::new();
    };
    let vertex_count = shape.vertices().len();
    (0..shape.edge_count())
        .map(|i| {
            let kind = match shape.segments.get(i) {
                Some(Segment::Arc { .. }) => SegmentKind::Arc,
                Some(Segment::Cubic { .. }) => SegmentKind::Cubic,
                _ => SegmentKind::Line,
            };
            let corner = if shape.closed {
                (i + 1) % vertex_count
            } else {
                i + 1
            };
            EdgeRow {
                index: i,
                length: format_length(shape.edge_length(i).unwrap_or(0.0), units),
                kind,
                angle: shape.corner_angle(corner).map(format_angle),
            }
        })
        .collect()
}

fn reshape(
    object: &Object,
    edit: impl FnOnce(&Shape) -> Result<Shape, FieldError>,
) -> Result<Object, FieldError> {
    let shape = kind_shape(&object.kind).ok_or(FieldError::NotStraight)?;
    let new = edit(shape)?;
    Ok(Object {
        kind: with_shape(&object.kind, new),
        ..object.clone()
    })
}

/// Make edge `edge` exactly the typed length (moves its end point).
pub fn apply_edge_length(
    object: &Object,
    edge: usize,
    input: &str,
    units: UnitSystem,
) -> Result<Object, FieldError> {
    let value = parse_length(input, units).map_err(|_| FieldError::NotALength)?;
    if value <= 0.0 {
        return Err(FieldError::MustBePositive);
    }
    reshape(object, |s| {
        s.set_edge_length(edge, value).map_err(|e| match e {
            EdgeEditError::NotStraight => FieldError::NotStraight,
            EdgeEditError::InvalidValue => FieldError::MustBePositive,
            EdgeEditError::OutOfRange => FieldError::NotALength,
        })
    })
}

/// Set the corner at `vertex` to the typed angle in degrees ("90", "90°", "89.5").
pub fn apply_corner_angle(
    object: &Object,
    vertex: usize,
    input: &str,
) -> Result<Object, FieldError> {
    let text = input.trim();
    let text = text.strip_suffix('°').unwrap_or(text).trim();
    let degrees: f64 = text.parse().map_err(|_| FieldError::NotAnAngle)?;
    if !degrees.is_finite() || degrees <= 0.0 || degrees >= 360.0 {
        return Err(FieldError::NotAnAngle);
    }
    reshape(object, |s| {
        s.set_corner_angle(vertex, degrees.to_radians())
            .map_err(|e| match e {
                EdgeEditError::NotStraight => FieldError::NotStraight,
                EdgeEditError::InvalidValue | EdgeEditError::OutOfRange => {
                    FieldError::AngleNotPossible
                }
            })
    })
}

/// Make edge `edge` straight, an arc or a curve.
pub fn apply_segment_kind(
    object: &Object,
    edge: usize,
    kind: SegmentKind,
) -> Result<Object, FieldError> {
    reshape(object, |s| {
        s.convert_segment(edge, kind)
            .map_err(|_| FieldError::NotStraight)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::EN;
    use crate::test_util::plant_at;
    use garden_core::geometry::{Point, SegmentKind, Shape};
    use garden_core::model::LayerId;

    #[test]
    fn fields_per_kind() {
        assert_eq!(
            inspector_fields(&plant_at(0., 0., 600.)),
            vec![Field::Name, Field::Spread, Field::Height]
        );
        let path = ObjectKind::Path {
            shape: Shape::polyline(&[Point::ZERO], false),
            width: 1.,
        };
        assert_eq!(inspector_fields(&path), vec![Field::Name, Field::Width]);
    }

    #[test]
    fn edits_plant_spread_with_units() {
        let o = Object::new("Lavender", plant_at(0., 0., 600.), LayerId::new());
        assert_eq!(field_value(&o, Field::Spread, UnitSystem::Metric), "60 cm");
        let o2 = apply_field(&o, Field::Spread, "1,2 m", UnitSystem::Metric).unwrap();
        assert!(matches!(o2.kind, ObjectKind::Plant { spread, .. } if spread == 1200.));
    }

    #[test]
    fn rejects_bad_input_and_leaves_object_unchanged() {
        let o = Object::new("Lavender", plant_at(0., 0., 600.), LayerId::new());
        assert_eq!(
            apply_field(&o, Field::Spread, "abc", UnitSystem::Metric),
            Err(FieldError::NotALength)
        );
        assert_eq!(
            apply_field(&o, Field::Spread, "-2 m", UnitSystem::Metric),
            Err(FieldError::NotALength)
        );
        assert_eq!(
            apply_field(&o, Field::Spread, "0", UnitSystem::Metric),
            Err(FieldError::MustBePositive)
        );
        assert_eq!(
            apply_field(&o, Field::Name, "   ", UnitSystem::Metric),
            Err(FieldError::Empty)
        );
        assert!(matches!(o.kind, ObjectKind::Plant { spread, .. } if spread == 600.));
    }

    #[test]
    fn measures_show_area_length_and_plant_count() {
        let d = crate::test_util::doc_with(vec![
            ObjectKind::Area {
                area_type: garden_core::model::AreaType::Bed,
                shape: Shape::rect(Point::ZERO, 4000., 2100.),
            },
            ObjectKind::Path {
                shape: Shape::polyline(&[Point::ZERO, Point::new(3500., 0.)], false),
                width: 900.,
            },
        ]);
        let bed = &d.objects[0];
        assert_eq!(
            measures(&d, bed, UnitSystem::Metric),
            vec![(EN.measure_area, "8.4 m²".to_string())]
        );
        let path = &d.objects[1];
        assert_eq!(
            measures(&d, path, UnitSystem::Metric),
            vec![(EN.measure_length, "3.50 m".to_string())]
        );

        let def = crate::catalog::builtin_plants()
            .into_iter()
            .find(|p| p.id == "salvia")
            .unwrap();
        let mut d2 = d.clone();
        let l = d2.layers[0].id;
        for (i, x) in [0., 600., 1200.].into_iter().enumerate() {
            d2.apply(
                garden_core::command::Command::Insert {
                    object: def.place(Point::new(x, 0.), l),
                    index: 2 + i,
                },
                1,
            )
            .unwrap();
        }
        let salvia = &d2.objects[2];
        assert_eq!(
            measures(&d2, salvia, UnitSystem::Metric),
            vec![(EN.measure_in_plan, "3".to_string())]
        );
    }

    fn rect_bed() -> Object {
        Object::new(
            "Bed",
            ObjectKind::Area {
                area_type: garden_core::model::AreaType::Bed,
                shape: Shape::rect(Point::ZERO, 4000., 3000.),
            },
            LayerId::new(),
        )
    }

    #[test]
    fn edge_rows_show_lengths_and_corners() {
        let rows = edge_rows(&rect_bed(), UnitSystem::Metric);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].length, "4.00 m");
        assert_eq!(rows[1].length, "3.00 m");
        assert_eq!(rows[0].angle.as_deref(), Some("90°"));
        assert_eq!(rows[0].kind, SegmentKind::Line);
        assert!(
            edge_rows(
                &Object::new("P", plant_at(0., 0., 1.), LayerId::new()),
                UnitSystem::Metric
            )
            .is_empty()
        );
    }

    #[test]
    fn open_lines_have_no_corner_after_the_last_edge() {
        let path = Object::new(
            "Path",
            ObjectKind::Path {
                shape: Shape::polyline(
                    &[Point::ZERO, Point::new(1000., 0.), Point::new(1000., 1000.)],
                    false,
                ),
                width: 900.,
            },
            LayerId::new(),
        );
        let rows = edge_rows(&path, UnitSystem::Metric);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].angle.as_deref(), Some("90°"));
        assert_eq!(rows[1].angle, None);
    }

    #[test]
    fn typed_lengths_and_angles_apply_exactly() {
        let o = apply_edge_length(&rect_bed(), 0, "5 m", UnitSystem::Metric).unwrap();
        assert_eq!(edge_rows(&o, UnitSystem::Metric)[0].length, "5.00 m");
        let o = apply_corner_angle(&o, 1, "100°").unwrap();
        assert_eq!(
            edge_rows(&o, UnitSystem::Metric)[0].angle.as_deref(),
            Some("100°")
        );
        let o = apply_corner_angle(&o, 1, " 89.5 ").unwrap();
        assert_eq!(
            edge_rows(&o, UnitSystem::Metric)[0].angle.as_deref(),
            Some("89.5°")
        );
        assert_eq!(
            apply_edge_length(&o, 0, "abc", UnitSystem::Metric),
            Err(FieldError::NotALength)
        );
        assert_eq!(
            apply_edge_length(&o, 0, "0", UnitSystem::Metric),
            Err(FieldError::MustBePositive)
        );
    }

    #[test]
    fn bad_angles_change_nothing() {
        let tri = Object::new(
            "T",
            ObjectKind::Area {
                area_type: garden_core::model::AreaType::Bed,
                shape: Shape::polyline(
                    &[Point::ZERO, Point::new(1000., 0.), Point::new(0., 1000.)],
                    true,
                ),
            },
            LayerId::new(),
        );
        assert_eq!(
            apply_corner_angle(&tri, 1, "abc"),
            Err(FieldError::NotAnAngle)
        );
        assert_eq!(
            apply_corner_angle(&tri, 1, "0"),
            Err(FieldError::NotAnAngle)
        );
        assert_eq!(
            apply_corner_angle(&tri, 1, "360"),
            Err(FieldError::NotAnAngle)
        );
        assert_eq!(
            apply_corner_angle(&tri, 1, "270"),
            Err(FieldError::AngleNotPossible)
        );
        assert_eq!(
            FieldError::AngleNotPossible.message(&EN),
            EN.err_angle_not_possible
        );
    }

    #[test]
    fn curved_edges_report_and_convert() {
        let o = apply_segment_kind(&rect_bed(), 0, SegmentKind::Arc).unwrap();
        let rows = edge_rows(&o, UnitSystem::Metric);
        assert_eq!(rows[0].kind, SegmentKind::Arc);
        assert_eq!(rows[0].angle, None);
        assert_eq!(
            apply_edge_length(&o, 0, "2 m", UnitSystem::Metric),
            Err(FieldError::NotStraight)
        );
        assert_eq!(
            apply_corner_angle(&o, 1, "90"),
            Err(FieldError::NotStraight)
        );
        let back = apply_segment_kind(&o, 0, SegmentKind::Line).unwrap();
        assert_eq!(
            edge_rows(&back, UnitSystem::Metric)[0].kind,
            SegmentKind::Line
        );
    }
}
