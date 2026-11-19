//! Which properties the inspector shows for an object, and how typed text is applied.

use garden_core::model::{Object, ObjectKind};
use garden_core::units::{UnitSystem, format_length, parse_length};

use crate::strings::Strings;

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
}

impl FieldError {
    pub fn message(&self, s: &Strings) -> &'static str {
        match self {
            FieldError::Empty => s.err_empty,
            FieldError::NotALength => s.err_not_a_length,
            FieldError::MustBePositive => s.err_must_be_positive,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::plant_at;
    use garden_core::geometry::{Point, Shape};
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
}
