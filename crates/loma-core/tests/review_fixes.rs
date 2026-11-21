//! Regression tests for issues found in the final review of loma-core.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use loma_core::command::{Command, CommandError};
use loma_core::geometry::{Point, PrimitiveHint, Shape};
use loma_core::io;
use loma_core::model::{AreaType, Document, Object, ObjectKind};
use loma_core::query::hit_test;
use loma_core::units::{ParseLengthError, UnitSystem, format_length, parse_length};

fn nan_path(layer: loma_core::model::LayerId) -> Object {
    Object::new(
        "p",
        ObjectKind::Path {
            shape: Shape::polyline(&[Point::ZERO, Point::new(1., 0.)], false),
            width: f64::NAN,
        },
        layer,
    )
}

#[test]
fn commands_with_non_finite_numbers_are_rejected() {
    let mut d = Document::new("g", 0);
    let l = d.layers[0].id;
    assert_eq!(
        d.apply(
            Command::Insert {
                object: nan_path(l),
                index: 0
            },
            1
        ),
        Err(CommandError::InvalidValue)
    );
    let bad_shape = Shape::polyline(
        &[
            Point::new(f64::INFINITY, 0.),
            Point::ZERO,
            Point::new(1., 1.),
        ],
        true,
    );
    let o = Object::new(
        "a",
        ObjectKind::Area {
            area_type: AreaType::Bed,
            shape: bad_shape,
        },
        l,
    );
    assert_eq!(
        d.apply(
            Command::Insert {
                object: o,
                index: 0
            },
            1
        ),
        Err(CommandError::InvalidValue)
    );
    assert!(d.objects.is_empty());
}

#[test]
fn scaling_by_a_non_finite_factor_leaves_shape_unchanged() {
    let r = Shape::rect(Point::ZERO, 10., 10.);
    assert_eq!(r.scaled(Point::ZERO, f64::INFINITY, 1.), r);
    assert_eq!(r.scaled(Point::ZERO, 1., f64::NAN), r);
}

#[test]
fn saving_non_finite_data_is_refused() {
    let mut d = Document::new("g", 0);
    let l = d.layers[0].id;
    d.objects.push(nan_path(l));
    assert!(io::to_json(&d).is_err());
    assert!(io::to_json(&Document::new("g", 0)).is_ok());
}

#[test]
fn non_uniform_scale_of_rotated_rect_keeps_hint_honest() {
    let r = Shape::rect(Point::ZERO, 1000., 1000.)
        .rotated(Point::ZERO, FRAC_PI_4)
        .scaled(Point::ZERO, 2., 1.);
    assert_eq!(r.hint, None);
    // a quarter turn swaps which scale factor applies to width and height
    let r = Shape::rect(Point::ZERO, 1000., 400.)
        .rotated(Point::ZERO, FRAC_PI_2)
        .scaled(Point::ZERO, 3., 1.);
    assert!(
        matches!(r.hint, Some(PrimitiveHint::Rect { width, height, .. }) if (width - 1000.).abs() < 1e-9 && (height - 1200.).abs() < 1e-9),
        "{:?}",
        r.hint
    );
}

#[test]
fn imperial_display_text_parses_back() {
    for mm in [1606.55, 82.55, 1524.0, 38.1, 6.35, 3048.0] {
        let s = format_length(mm, UnitSystem::Imperial);
        let back = parse_length(&s, UnitSystem::Imperial).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert!((back - mm).abs() < 3.2, "{s}"); // within 1/8 inch
    }
    assert!((parse_length("1 1/2\"", UnitSystem::Imperial).unwrap() - 38.1).abs() < 1e-9);
    assert!((parse_length("5'-3\"", UnitSystem::Imperial).unwrap() - 1600.2).abs() < 1e-6);
    assert!((parse_length("3/4\"", UnitSystem::Imperial).unwrap() - 19.05).abs() < 1e-9);
    assert!(parse_length("1/0\"", UnitSystem::Imperial).is_err());
}

#[test]
fn ambiguous_thousands_comma_is_rejected() {
    assert!(matches!(
        parse_length("1,500 mm", UnitSystem::Metric),
        Err(ParseLengthError::Invalid(_))
    ));
    assert!((parse_length("1,50 m", UnitSystem::Metric).unwrap() - 1500.).abs() < 1e-9);
}

#[test]
fn dimension_is_hit_on_its_drawn_offset_line() {
    let mut d = Document::new("g", 0);
    let l = d.layers[0].id;
    let dim = Object::new(
        "d",
        ObjectKind::Dimension {
            from: Point::ZERO,
            to: Point::new(1000., 0.),
            offset: 500.,
        },
        l,
    );
    d.apply(
        Command::Insert {
            object: dim,
            index: 0,
        },
        1,
    )
    .unwrap();
    assert!(hit_test(&d, Point::new(500., 500.), 10.).is_some());
    assert!(hit_test(&d, Point::new(500., 0.), 10.).is_none());
}

#[test]
fn maximum_revision_does_not_overflow() {
    let mut d = Document::new("g", 0);
    let l = d.layers[0].id;
    let mut o = Object::new(
        "x",
        ObjectKind::Label {
            position: Point::ZERO,
            text: "x".into(),
            size: 1.,
        },
        l,
    );
    d.apply(
        Command::Insert {
            object: o.clone(),
            index: 0,
        },
        1,
    )
    .unwrap();
    o.revision = u64::MAX;
    d.apply(Command::Replace { object: o.clone() }, 2).unwrap();
    assert_eq!(d.objects[0].revision, u64::MAX);
    d.apply(Command::Delete { id: o.id }, 3).unwrap();
    assert_eq!(d.tombstones[0].revision, u64::MAX);
}

#[test]
fn impossible_corner_angle_is_rejected() {
    let tri = Shape::polyline(
        &[Point::ZERO, Point::new(1000., 0.), Point::new(0., 1000.)],
        true,
    );
    assert!(tri.set_corner_angle(1, 4.0).is_err());
    let zero_edge = Shape::polyline(
        &[
            Point::ZERO,
            Point::new(1000., 0.),
            Point::new(1000., 0.),
            Point::new(0., 1000.),
        ],
        true,
    );
    assert!(zero_edge.set_corner_angle(1, 1.0).is_err());
    assert_eq!(zero_edge.corner_angle(1), None);
}
