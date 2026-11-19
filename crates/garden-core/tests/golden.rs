//! The v1 sample file is committed once and never regenerated: every future
//! version of the app must still load it.

use garden_core::command::Command;
use garden_core::geometry::{Point, Segment, Shape};
use garden_core::io;
use garden_core::model::*;

#[test]
fn v1_sample_always_loads() {
    let doc =
        io::from_json(include_str!("fixtures/v1_sample.garden.json")).expect("v1 file must load");
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(doc.tombstones.len(), 1);
    assert!(doc.background.is_some());
    assert_eq!(doc.custom_plants.len(), 1);
    assert_eq!(doc.objects.len(), 9);
    let distinct: std::collections::HashSet<_> = doc
        .objects
        .iter()
        .map(|o| std::mem::discriminant(&o.kind))
        .collect();
    assert_eq!(distinct.len(), 8, "every ObjectKind appears");
}

/// A file with an empty `layers` list (hand-written or from a buggy build)
/// gets one usable layer, and objects on missing layers move onto it.
#[test]
fn file_without_layers_gets_one() {
    let doc = io::from_json(include_str!("fixtures/no_layers.garden.json"))
        .expect("file without layers must load");
    assert_eq!(doc.layers.len(), 1);
    let layer = &doc.layers[0];
    assert!(layer.visible && !layer.locked);
    assert_eq!(doc.objects.len(), 1);
    assert_eq!(doc.objects[0].layer, layer.id);
}

/// Objects pointing at a layer that isn't in the file move to the first layer.
#[test]
fn objects_on_missing_layers_move_to_the_first() {
    let mut v: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/v1_sample.garden.json")).unwrap();
    v["objects"][0]["layer"] = serde_json::json!(LayerId::new());
    let doc = io::from_json(&v.to_string()).unwrap();
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(doc.objects[0].layer, doc.layers[0].id);
    assert_eq!(doc.objects[6].layer, doc.layers[1].id); // untouched
}

/// Writes the fixture. Run once: `cargo test -p garden-core --test golden -- --ignored`.
#[test]
#[ignore]
fn generate_v1_sample() {
    let mut d = Document::new("Sample garden", 1_763_500_000_000);
    let l = d.layers[0].id;
    let plants = LayerId::new();
    let t = 1_763_500_000_100;
    let mut cmds = vec![Command::UpsertLayer {
        layer: Layer {
            id: plants,
            name: "Plants".into(),
            visible: true,
            locked: false,
        },
    }];
    let curvy = Shape {
        start: Point::new(1000., 1000.),
        segments: vec![
            Segment::Line {
                to: Point::new(4000., 1000.),
            },
            Segment::Arc {
                to: Point::new(4000., 3000.),
                sweep: std::f64::consts::PI,
            },
            Segment::Cubic {
                c1: Point::new(3000., 3500.),
                c2: Point::new(2000., 2500.),
                to: Point::new(1000., 3000.),
            },
        ],
        closed: true,
        hint: None,
    };
    let lavender = PlantDef {
        id: "custom-lavender".into(),
        name: "Hidcote lavender".into(),
        category: PlantCategory::Shrub,
        spread: 600.,
        height: 500.,
        symbol: PlantSymbol::Spiky,
        custom: true,
    };
    let objects = vec![
        Object::new(
            "Plot",
            ObjectKind::PlotBoundary {
                shape: Shape::rect(Point::ZERO, 12_000., 20_000.),
            },
            l,
        ),
        Object::new(
            "Lawn",
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: Shape::ellipse(Point::new(6000., 12_000.), 4000., 3000.),
            },
            l,
        ),
        Object::new(
            "Border",
            ObjectKind::Area {
                area_type: AreaType::Bed,
                shape: curvy,
            },
            l,
        ),
        Object::new(
            "Path",
            ObjectKind::Path {
                shape: Shape::polyline(&[Point::new(6000., 0.), Point::new(6000., 9000.)], false),
                width: 900.,
            },
            l,
        ),
        Object::new(
            "Fence",
            ObjectKind::Linear {
                kind: LinearKind::Fence,
                shape: Shape::polyline(&[Point::ZERO, Point::new(12_000., 0.)], false),
                thickness: 50.,
                height: 1800.,
            },
            l,
        ),
        Object::new(
            "Shed",
            ObjectKind::Structure {
                kind: StructureKind::Shed,
                footprint: Shape::rect(Point::new(9000., 17_000.), 2400., 1800.),
                height: 2200.,
            },
            l,
        ),
        lavender.place(Point::new(2000., 1500.), plants),
        Object::new(
            "Dim",
            ObjectKind::Dimension {
                from: Point::ZERO,
                to: Point::new(12_000., 0.),
                offset: -500.,
            },
            l,
        ),
        Object::new(
            "Note",
            ObjectKind::Label {
                position: Point::new(100., 100.),
                text: "Sunny corner".into(),
                size: 300.,
            },
            l,
        ),
    ];
    let label = Object::new(
        "Old note",
        ObjectKind::Label {
            position: Point::new(100., 100.),
            text: "Removed".into(),
            size: 300.,
        },
        l,
    );
    let label_id = label.id;
    for (i, o) in objects.into_iter().chain([label]).enumerate() {
        cmds.push(Command::Insert {
            object: o,
            index: i,
        });
    }
    cmds.push(Command::Delete { id: label_id }); // leaves a tombstone; 9 objects remain covering all 8 kinds
    cmds.push(Command::UpsertCustomPlant { def: lavender });
    cmds.push(Command::SetBackground {
        background: Some(BackgroundImage {
            image_ref: "bg-0001".into(),
            mime: "image/jpeg".into(),
            width_px: 1200,
            height_px: 2000,
            origin: Point::ZERO,
            mm_per_px: 10.,
            rotation: 0.,
            opacity: 0.5,
            locked: true,
        }),
    });
    d.apply(Command::Batch { commands: cmds }, t).unwrap();
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/v1_sample.garden.json"
        ),
        io::to_json(&d).unwrap(),
    )
    .unwrap();
}
