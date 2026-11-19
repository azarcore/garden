//! Starter gardens a new design can begin from.
//!
//! Each template is built in code from catalog plants and plain shapes, the
//! same way the editor builds objects, so tests catch any that drift out of
//! step with the model. Plans are in millimetres with the plot at the origin.

use garden_core::command::Command;
use garden_core::geometry::{Point, Segment, Shape};
use garden_core::model::{
    AreaType, Document, LayerId, LinearKind, Object, ObjectKind, StructureKind,
};

use crate::catalog::builtin_plants;
use crate::objects::default_name;
use crate::strings::EN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateId {
    SmallBack,
    Front,
    Cottage,
    Vegetable,
    Courtyard,
}

/// A starter garden as the chooser shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Template {
    pub id: TemplateId,
    pub name: &'static str,
    pub blurb: &'static str,
    /// Plot width and depth (mm).
    pub size_mm: (f64, f64),
}

/// Every template, in the order the chooser shows them.
pub fn templates() -> Vec<Template> {
    let t = |id, name, blurb, w, h| Template {
        id,
        name,
        blurb,
        size_mm: (w, h),
    };
    vec![
        t(
            TemplateId::SmallBack,
            EN.tpl_small_back,
            EN.tpl_small_back_blurb,
            10_000.0,
            6_000.0,
        ),
        t(
            TemplateId::Front,
            EN.tpl_front,
            EN.tpl_front_blurb,
            8_000.0,
            5_000.0,
        ),
        t(
            TemplateId::Cottage,
            EN.tpl_cottage,
            EN.tpl_cottage_blurb,
            12_000.0,
            8_000.0,
        ),
        t(
            TemplateId::Vegetable,
            EN.tpl_vegetable,
            EN.tpl_vegetable_blurb,
            10_000.0,
            5_000.0,
        ),
        t(
            TemplateId::Courtyard,
            EN.tpl_courtyard,
            EN.tpl_courtyard_blurb,
            6_000.0,
            6_000.0,
        ),
    ]
}

/// A new document built from the template, with fresh ids, named `name`.
pub fn instantiate(id: TemplateId, name: &str, now_ms: i64) -> Document {
    let mut b = Builder::new(name, now_ms);
    match id {
        TemplateId::SmallBack => small_back(&mut b),
        TemplateId::Front => front(&mut b),
        TemplateId::Cottage => cottage(&mut b),
        TemplateId::Vegetable => vegetable(&mut b),
        TemplateId::Courtyard => courtyard(&mut b),
    }
    b.doc
}

/// Adds objects to a document through commands, as the editor does.
struct Builder {
    doc: Document,
    layer: LayerId,
    now_ms: i64,
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Shape {
    Shape::rect(Point::new(x, y), w, h)
}

fn pts(points: &[(f64, f64)]) -> Vec<Point> {
    points.iter().map(|&(x, y)| Point::new(x, y)).collect()
}

impl Builder {
    fn new(name: &str, now_ms: i64) -> Builder {
        let doc = Document::new(name, now_ms);
        let layer = doc.layers[0].id;
        Builder { doc, layer, now_ms }
    }

    fn add(&mut self, kind: ObjectKind) {
        let object = Object::new(default_name(&kind), kind, self.layer);
        self.insert(object);
    }

    fn insert(&mut self, object: Object) {
        let index = self.doc.objects.len();
        self.doc
            .apply(Command::Insert { object, index }, self.now_ms)
            .expect("template objects are valid");
    }

    fn plot(&mut self, w: f64, h: f64) {
        self.add(ObjectKind::PlotBoundary {
            shape: rect(0.0, 0.0, w, h),
        });
    }

    fn area(&mut self, area_type: AreaType, shape: Shape) {
        self.add(ObjectKind::Area { area_type, shape });
    }

    fn path(&mut self, shape: Shape, width: f64) {
        self.add(ObjectKind::Path { shape, width });
    }

    fn linear(&mut self, kind: LinearKind, points: &[(f64, f64)], thickness: f64, height: f64) {
        self.add(ObjectKind::Linear {
            kind,
            shape: Shape::polyline(&pts(points), false),
            thickness,
            height,
        });
    }

    fn structure(&mut self, kind: StructureKind, footprint: Shape, height: f64) {
        self.add(ObjectKind::Structure {
            kind,
            footprint,
            height,
        });
    }

    /// A catalog plant at (x, y).
    fn plant(&mut self, catalog_id: &str, x: f64, y: f64) {
        let def = builtin_plants()
            .into_iter()
            .find(|p| p.id == catalog_id)
            .unwrap_or_else(|| panic!("no catalog plant {catalog_id}"));
        let object = def.place(Point::new(x, y), self.layer);
        self.insert(object);
    }
}

/// 10 × 6 m: patio by the house (bottom), oval lawn, borders along the back
/// and left, a shed in the far corner reached by a path, an apple tree.
fn small_back(b: &mut Builder) {
    b.plot(10_000.0, 6_000.0);
    b.area(AreaType::Paving, rect(0.0, 4_200.0, 4_000.0, 1_800.0));
    b.area(
        AreaType::Lawn,
        Shape::ellipse(Point::new(5_500.0, 2_700.0), 2_600.0, 1_300.0),
    );
    b.area(AreaType::Bed, rect(300.0, 300.0, 7_000.0, 900.0));
    b.area(AreaType::Bed, rect(300.0, 1_400.0, 1_000.0, 2_600.0));
    b.path(
        Shape::polyline(
            &pts(&[(4_100.0, 5_100.0), (8_800.0, 5_100.0), (8_800.0, 1_900.0)]),
            false,
        ),
        800.0,
    );
    b.structure(
        StructureKind::Shed,
        rect(8_000.0, 300.0, 1_700.0, 1_500.0),
        2_200.0,
    );
    // back border
    b.plant("rose", 1_000.0, 750.0);
    for x in [2_200.0, 2_900.0, 3_600.0] {
        b.plant("lavender", x, 750.0);
    }
    b.plant("hydrangea", 4_800.0, 750.0);
    b.plant("salvia", 6_000.0, 750.0);
    b.plant("salvia", 6_600.0, 750.0);
    // side border
    b.plant("hosta", 800.0, 1_900.0);
    b.plant("hosta", 800.0, 2_900.0);
    b.plant("geranium", 800.0, 3_700.0);
    b.plant("apple", 6_800.0, 2_700.0);
}

/// 8 × 5 m: house along the top, gate at the bottom; a straight path to the
/// door between two lawns, beds under the windows, a hedge with a gap, a maple.
fn front(b: &mut Builder) {
    b.plot(8_000.0, 5_000.0);
    b.path(
        Shape::polyline(&pts(&[(4_000.0, 600.0), (4_000.0, 4_400.0)]), false),
        1_200.0,
    );
    b.area(AreaType::Bed, rect(300.0, 300.0, 3_000.0, 1_300.0));
    b.area(AreaType::Bed, rect(4_700.0, 300.0, 3_000.0, 1_300.0));
    b.area(AreaType::Lawn, rect(300.0, 1_900.0, 3_000.0, 2_200.0));
    b.area(AreaType::Lawn, rect(4_700.0, 1_900.0, 3_000.0, 2_200.0));
    b.linear(
        LinearKind::Hedge,
        &[(300.0, 4_700.0), (3_300.0, 4_700.0)],
        600.0,
        1_200.0,
    );
    b.linear(
        LinearKind::Hedge,
        &[(4_700.0, 4_700.0), (7_700.0, 4_700.0)],
        600.0,
        1_200.0,
    );
    for x in [800.0, 1_500.0, 2_200.0] {
        b.plant("lavender", x, 950.0);
    }
    b.plant("box", 2_900.0, 950.0);
    b.plant("rose", 5_400.0, 950.0);
    b.plant("hydrangea", 6_900.0, 950.0);
    b.plant("acer", 6_200.0, 3_000.0);
}

/// 12 × 8 m: a curving path from the gate (bottom) to a bench (top), with
/// two big flowing borders packed with perennials either side.
fn cottage(b: &mut Builder) {
    b.plot(12_000.0, 8_000.0);
    b.area(
        AreaType::Bed,
        Shape::ellipse(Point::new(2_800.0, 4_000.0), 2_300.0, 3_000.0),
    );
    b.area(
        AreaType::Bed,
        Shape::ellipse(Point::new(9_500.0, 4_300.0), 2_000.0, 2_800.0),
    );
    b.path(
        Shape {
            start: Point::new(6_000.0, 7_400.0),
            segments: vec![Segment::Cubic {
                c1: Point::new(3_500.0, 5_500.0),
                c2: Point::new(8_500.0, 3_500.0),
                to: Point::new(6_000.0, 1_700.0),
            }],
            closed: false,
            hint: None,
        },
        900.0,
    );
    b.structure(
        StructureKind::Furniture,
        rect(5_300.0, 600.0, 1_400.0, 700.0),
        900.0,
    );
    let left = [
        ["rose", "salvia", "geranium"],
        ["lavender", "hosta", "salvia"],
        ["grass", "rose", "fern"],
        ["salvia", "geranium", "lavender"],
        ["hosta", "grass", "salvia"],
    ];
    for (row, names) in left.iter().enumerate() {
        for (col, name) in names.iter().enumerate() {
            b.plant(
                name,
                1_500.0 + 1_000.0 * col as f64,
                2_000.0 + 1_000.0 * row as f64,
            );
        }
    }
    let right = [
        ["geranium", "rose", "salvia"],
        ["salvia", "lavender", "hosta"],
        ["fern", "salvia", "geranium"],
        ["lavender", "grass", "salvia"],
        ["hosta", "geranium", "fern"],
    ];
    for (row, names) in right.iter().enumerate() {
        for (col, name) in names.iter().enumerate() {
            b.plant(
                name,
                8_500.0 + 1_000.0 * col as f64,
                2_500.0 + 1_000.0 * row as f64,
            );
        }
    }
}

/// 10 × 5 m: four long beds with gravel paths between, a cross path, compost
/// bays and a greenhouse.
fn vegetable(b: &mut Builder) {
    b.plot(10_000.0, 5_000.0);
    let beds = [500.0, 2_300.0, 4_100.0, 5_900.0];
    for x in beds {
        b.area(AreaType::Bed, rect(x, 700.0, 1_200.0, 3_600.0));
    }
    for x in [1_700.0, 3_500.0, 5_300.0] {
        b.area(AreaType::Gravel, rect(x, 700.0, 600.0, 3_600.0));
    }
    b.area(AreaType::Gravel, rect(500.0, 4_300.0, 6_600.0, 500.0));
    b.structure(
        StructureKind::Other,
        rect(7_700.0, 700.0, 850.0, 1_100.0),
        900.0,
    );
    b.structure(
        StructureKind::Other,
        rect(8_650.0, 700.0, 850.0, 1_100.0),
        900.0,
    );
    b.structure(
        StructureKind::Greenhouse,
        rect(7_700.0, 2_300.0, 1_800.0, 2_000.0),
        2_200.0,
    );
    for y in [1_100.0, 1_900.0, 2_700.0, 3_500.0] {
        b.plant("tomato", 1_100.0, y);
    }
    for y in [1_000.0, 1_600.0, 2_200.0, 2_800.0, 3_400.0, 4_000.0] {
        b.plant("lettuce", 2_600.0, y);
        b.plant("lettuce", 3_200.0, y);
    }
    for y in [1_000.0, 1_600.0, 2_200.0, 2_800.0, 3_400.0, 4_000.0] {
        b.plant("basil", 4_400.0, y);
        b.plant("basil", 5_000.0, y);
    }
    for y in [1_200.0, 2_200.0, 3_200.0] {
        b.plant("rosemary", 6_500.0, y);
    }
}

/// 6 × 6 m: walled on three sides, house at the bottom; paving with a round
/// pool, planting along the walls and a maple in a corner bed.
fn courtyard(b: &mut Builder) {
    b.plot(6_000.0, 6_000.0);
    b.linear(
        LinearKind::Wall,
        &[
            (100.0, 5_900.0),
            (100.0, 100.0),
            (5_900.0, 100.0),
            (5_900.0, 5_900.0),
        ],
        200.0,
        1_800.0,
    );
    b.area(AreaType::Bed, rect(200.0, 200.0, 5_600.0, 1_000.0));
    b.area(AreaType::Bed, rect(200.0, 1_200.0, 1_000.0, 4_600.0));
    b.area(AreaType::Bed, rect(4_800.0, 1_200.0, 1_000.0, 4_600.0));
    b.area(AreaType::Bed, rect(1_200.0, 1_200.0, 1_600.0, 1_600.0));
    b.area(
        AreaType::Paving,
        Shape::polyline(
            &pts(&[
                (2_800.0, 1_200.0),
                (4_800.0, 1_200.0),
                (4_800.0, 6_000.0),
                (1_200.0, 6_000.0),
                (1_200.0, 2_800.0),
                (2_800.0, 2_800.0),
            ]),
            true,
        ),
    );
    b.area(
        AreaType::Water,
        Shape::circle(Point::new(3_300.0, 4_200.0), 600.0),
    );
    b.plant("acer", 1_700.0, 1_700.0);
    for x in [3_400.0, 4_200.0, 5_000.0] {
        b.plant("box", x, 700.0);
    }
    for (i, y) in [2_000.0, 3_000.0, 4_000.0, 5_000.0].into_iter().enumerate() {
        b.plant(if i % 2 == 0 { "hosta" } else { "fern" }, 5_300.0, y);
    }
    for (i, y) in [3_300.0, 4_300.0, 5_300.0].into_iter().enumerate() {
        b.plant(if i % 2 == 0 { "fern" } else { "hosta" }, 700.0, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::builtin_plants;
    use crate::objects::object_bounds;
    use garden_core::geometry::Point;
    use garden_core::model::{AreaType, Document, ObjectKind};

    fn plot_rect(d: &Document) -> kurbo::Rect {
        let plots: Vec<_> = d
            .objects
            .iter()
            .filter_map(|o| match &o.kind {
                ObjectKind::PlotBoundary { shape } => Some(shape.bounding_box()),
                _ => None,
            })
            .collect();
        assert_eq!(plots.len(), 1, "{}: one plot", d.name);
        plots[0]
    }

    #[test]
    fn there_are_five_templates_in_order() {
        let ids: Vec<_> = templates().iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            [
                TemplateId::SmallBack,
                TemplateId::Front,
                TemplateId::Cottage,
                TemplateId::Vegetable,
                TemplateId::Courtyard
            ]
        );
    }

    #[test]
    fn each_template_has_its_plot_size_and_stays_inside_it() {
        for t in templates() {
            let d = instantiate(t.id, t.name, 5);
            let r = plot_rect(&d);
            assert_eq!((r.x0, r.y0), (0.0, 0.0), "{}", t.name);
            assert_eq!((r.width(), r.height()), t.size_mm, "{}", t.name);
            for o in &d.objects {
                assert!(
                    r.inflate(1.0, 1.0).contains_rect(object_bounds(&o.kind)),
                    "{}: {} sticks out ({:?})",
                    t.name,
                    o.name,
                    object_bounds(&o.kind)
                );
            }
        }
    }

    #[test]
    fn templates_survive_saving_and_loading() {
        for t in templates() {
            let d = instantiate(t.id, t.name, 5);
            let back = garden_core::io::from_json(&garden_core::io::to_json(&d).unwrap()).unwrap();
            assert_eq!(back.objects.len(), d.objects.len());
            assert_eq!(back.name, t.name);
            assert!(d.objects.len() >= 8, "{} is too sparse", t.name);
        }
    }

    #[test]
    fn plants_come_from_the_catalog_and_dont_sit_on_hard_ground_or_each_other() {
        let catalog: Vec<String> = builtin_plants().into_iter().map(|p| p.id).collect();
        for t in templates() {
            let d = instantiate(t.id, t.name, 5);
            let plants: Vec<(Point, f64)> = d
                .objects
                .iter()
                .filter_map(|o| match &o.kind {
                    ObjectKind::Plant {
                        position,
                        spread,
                        catalog_id,
                        ..
                    } => {
                        let id = catalog_id.as_ref().expect("catalog plant");
                        assert!(catalog.contains(id), "{}: unknown plant {id}", t.name);
                        Some((*position, *spread))
                    }
                    _ => None,
                })
                .collect();
            assert!(plants.len() >= 4, "{}: too few plants", t.name);
            for (p, _) in &plants {
                for o in &d.objects {
                    match &o.kind {
                        ObjectKind::Area {
                            area_type: AreaType::Paving | AreaType::Water | AreaType::Gravel,
                            shape,
                        } => assert!(
                            !shape.contains(*p),
                            "{}: plant on {} at {p:?}",
                            t.name,
                            o.name
                        ),
                        ObjectKind::Path { shape, width } => assert!(
                            shape.distance_to(*p) > width / 2.0,
                            "{}: plant on the path at {p:?}",
                            t.name
                        ),
                        ObjectKind::Structure { footprint, .. } => {
                            assert!(
                                !footprint.contains(*p),
                                "{}: plant in {} at {p:?}",
                                t.name,
                                o.name
                            )
                        }
                        _ => {}
                    }
                }
            }
            for (i, (a, ra)) in plants.iter().enumerate() {
                for (b, rb) in &plants[i + 1..] {
                    assert!(
                        a.distance(*b) >= 0.4 * (ra + rb),
                        "{}: plants piled up at {a:?} and {b:?}",
                        t.name
                    );
                }
            }
        }
    }

    #[test]
    fn every_instantiation_is_a_new_design() {
        let a = instantiate(TemplateId::Courtyard, "Courtyard", 1);
        let b = instantiate(TemplateId::Courtyard, "Courtyard 2", 2);
        assert_ne!(a.id, b.id);
        assert_ne!(a.objects[0].id, b.objects[0].id);
        assert_eq!(b.name, "Courtyard 2");
        assert!(a.objects.iter().all(|o| o.layer == a.layers[0].id));
    }
}
