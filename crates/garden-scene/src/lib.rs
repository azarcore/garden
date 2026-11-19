#![forbid(unsafe_code)]
//! A garden design as plain 3D geometry: triangle meshes in metres (Y up),
//! plant instances and a starting camera. No graphics library here, so it is
//! tested natively; `garden-3d` draws it.

mod surfaces;

use garden_core::geometry::Point;
use garden_core::model::{
    AreaType, Color, Document, Object, ObjectKind, PlantCategory, PlantSymbol,
};
use garden_editor::objects::object_bounds;
use garden_editor::render::default_style;
use kurbo::Rect;

pub use surfaces::flat_polygon;

/// sRGB colour, 0..1 per channel.
pub type Rgba = [f32; 4];

/// What a mesh is, so the viewer can pick a material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Ground,
    Plot,
    Lawn,
    Bed,
    Paving,
    Gravel,
    Water,
    Area,
    Path,
    Fence,
    Wall,
    Hedge,
    Structure,
}

/// Triangles with one normal per vertex (flat shaded), in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub color: Rgba,
    pub kind: SurfaceKind,
}

/// A plant standing on the ground at its mature size (metres).
#[derive(Clone, Debug, PartialEq)]
pub struct PlantInstance {
    pub position: [f32; 3],
    pub spread: f32,
    pub height: f32,
    pub category: PlantCategory,
    pub symbol: PlantSymbol,
    pub color: Rgba,
}

/// Where the orbit camera starts: looking at `target` from `radius` metres away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraStart {
    pub target: [f32; 3],
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Scene {
    pub meshes: Vec<MeshData>,
    pub plants: Vec<PlantInstance>,
    pub camera: Option<CameraStart>,
    /// Objects that couldn't be turned into geometry (e.g. self-crossing outlines).
    pub skipped: usize,
}

/// Plan millimetres (x, y-down) to 3D metres (x, `height_m`, z).
pub fn mm_to_world(p: Point, height_m: f64) -> [f32; 3] {
    [
        (p.x / 1000.0) as f32,
        height_m as f32,
        (p.y / 1000.0) as f32,
    ]
}

/// Collects flat-shaded triangles; each triangle is wound so it faces `normal`.
#[derive(Default)]
pub(crate) struct MeshBuilder {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn f32s(p: [f64; 3]) -> [f32; 3] {
    p.map(|v| v as f32)
}

impl MeshBuilder {
    pub fn triangle(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], normal: [f64; 3]) {
        let n = cross(sub(b, a), sub(c, a));
        let facing = n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2];
        let (b, c) = if facing < 0.0 { (c, b) } else { (b, c) };
        let base = self.positions.len() as u32;
        for p in [a, b, c] {
            self.positions.push(f32s(p));
            self.normals.push(f32s(normal));
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    pub fn quad(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3], normal: [f64; 3]) {
        self.triangle(a, b, c, normal);
        self.triangle(a, c, d, normal);
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn finish(self, kind: SurfaceKind, color: Rgba) -> MeshData {
        MeshData {
            positions: self.positions,
            normals: self.normals,
            indices: self.indices,
            color,
            kind,
        }
    }
}

pub(crate) fn rgba(c: Color) -> Rgba {
    [c.0, c.1, c.2, c.3].map(|v| f32::from(v) / 255.0)
}

/// Ground around everything, a little below the plot.
const GROUND_Y: f64 = -0.01;
/// How far the ground reaches beyond the design (metres).
const GROUND_MARGIN_M: f64 = 10.0;
/// Each later object sits this much higher, so overlapping surfaces don't flicker.
const LAYER_STEP_M: f64 = 0.0002;
/// Raised beds stand this high.
const BED_HEIGHT_M: f64 = 0.12;
const GROUND_COLOR: Color = Color(196, 207, 184, 255);
const PLOT_COLOR: Color = Color(236, 238, 226, 255);

/// Height of a flat surface of this kind (before the per-object step).
fn surface_height(kind: SurfaceKind) -> f64 {
    match kind {
        SurfaceKind::Plot => 0.0,
        SurfaceKind::Water => 0.002,
        SurfaceKind::Lawn => 0.004,
        SurfaceKind::Area => 0.005,
        SurfaceKind::Gravel => 0.006,
        SurfaceKind::Paving => 0.008,
        SurfaceKind::Path => 0.012,
        _ => 0.0,
    }
}

fn area_kind(t: AreaType) -> SurfaceKind {
    match t {
        AreaType::Bed => SurfaceKind::Bed,
        AreaType::Lawn => SurfaceKind::Lawn,
        AreaType::Paving => SurfaceKind::Paving,
        AreaType::Gravel => SurfaceKind::Gravel,
        AreaType::Water => SurfaceKind::Water,
        AreaType::Other => SurfaceKind::Area,
    }
}

/// The fill colour used in 2D (the object's own, else the palette's).
pub(crate) fn fill_color(o: &Object, fallback: Color) -> Rgba {
    let (fill, stroke) = default_style(&o.kind);
    rgba(
        o.style
            .fill
            .or(fill)
            .or(o.style.stroke)
            .or(stroke)
            .unwrap_or(fallback),
    )
}

/// Turn a design into 3D geometry. Hidden layers, labels and dimensions are left out.
pub fn build(doc: &Document) -> Scene {
    let mut scene = Scene::default();
    let mut bounds: Option<Rect> = None;
    for (index, o) in doc.objects.iter().enumerate() {
        if !doc.layer(o.layer).is_some_and(|l| l.visible) {
            continue;
        }
        if matches!(
            o.kind,
            ObjectKind::Label { .. } | ObjectKind::Dimension { .. }
        ) {
            continue;
        }
        let b = object_bounds(&o.kind);
        bounds = Some(bounds.map_or(b, |a| a.union(b)));
        let lift = LAYER_STEP_M * index as f64;
        let mut mb = MeshBuilder::default();
        let built = match &o.kind {
            ObjectKind::PlotBoundary { shape } => surfaces::add_top(&mut mb, shape, lift)
                .map(|_| (SurfaceKind::Plot, rgba(PLOT_COLOR))),
            ObjectKind::Area { area_type, shape } => {
                let kind = area_kind(*area_type);
                let color = fill_color(o, PLOT_COLOR);
                if kind == SurfaceKind::Bed {
                    surfaces::add_prism(&mut mb, shape, 0.0, BED_HEIGHT_M + lift)
                        .map(|_| (kind, color))
                } else {
                    surfaces::add_top(&mut mb, shape, surface_height(kind) + lift)
                        .map(|_| (kind, color))
                }
            }
            _ => continue,
        };
        match built {
            Some((kind, color)) if !mb.is_empty() => scene.meshes.push(mb.finish(kind, color)),
            _ => scene.skipped += 1,
        }
    }
    scene.meshes.insert(0, ground(bounds));
    scene
}

/// A square of ground reaching `GROUND_MARGIN_M` beyond the design (20 × 20 m when empty).
fn ground(bounds: Option<Rect>) -> MeshData {
    let r = bounds
        .map(|b| Rect::new(b.x0 / 1000.0, b.y0 / 1000.0, b.x1 / 1000.0, b.y1 / 1000.0))
        .filter(|r| [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()))
        .map(|r| r.inflate(GROUND_MARGIN_M, GROUND_MARGIN_M))
        .unwrap_or(Rect::new(-10.0, -10.0, 10.0, 10.0));
    let mut b = MeshBuilder::default();
    b.quad(
        [r.x0, GROUND_Y, r.y0],
        [r.x1, GROUND_Y, r.y0],
        [r.x1, GROUND_Y, r.y1],
        [r.x0, GROUND_Y, r.y1],
        [0.0, 1.0, 0.0],
    );
    b.finish(SurfaceKind::Ground, rgba(GROUND_COLOR))
}

#[cfg(test)]
pub(crate) mod test_util {
    use garden_core::command::Command;
    use garden_core::geometry::{Point, Shape};
    use garden_core::model::*;

    pub fn doc_with(kinds: Vec<ObjectKind>) -> Document {
        let mut d = Document::new("g", 0);
        let l = d.layers[0].id;
        for (i, k) in kinds.into_iter().enumerate() {
            d.apply(
                Command::Insert {
                    object: Object::new(format!("{i}"), k, l),
                    index: i,
                },
                0,
            )
            .unwrap();
        }
        d
    }

    #[allow(dead_code)] // used from Task 2 on
    pub fn plant_at(x: f64, y: f64, spread: f64) -> ObjectKind {
        ObjectKind::Plant {
            position: Point::new(x, y),
            spread,
            height: 500.,
            symbol: PlantSymbol::Round,
            category: PlantCategory::Shrub,
            catalog_id: None,
        }
    }

    pub fn lawn_rect() -> ObjectKind {
        ObjectKind::Area {
            area_type: AreaType::Lawn,
            shape: Shape::rect(Point::ZERO, 4000., 3000.),
        }
    }

    /// One of every object kind (copy of garden_editor's test sample).
    #[allow(dead_code)] // used from Task 2 on
    pub fn all_kinds_sample() -> Vec<ObjectKind> {
        let line = Shape::polyline(&[Point::ZERO, Point::new(1000., 0.)], false);
        vec![
            ObjectKind::PlotBoundary {
                shape: Shape::rect(Point::ZERO, 10_000., 10_000.),
            },
            lawn_rect(),
            ObjectKind::Path {
                shape: line.clone(),
                width: 900.,
            },
            ObjectKind::Linear {
                kind: LinearKind::Hedge,
                shape: line,
                thickness: 600.,
                height: 1500.,
            },
            ObjectKind::Structure {
                kind: StructureKind::Shed,
                footprint: Shape::rect(Point::ZERO, 2000., 1500.),
                height: 2200.,
            },
            plant_at(100., 100., 600.),
            ObjectKind::Label {
                position: Point::ZERO,
                text: "Hi".into(),
                size: 300.,
            },
            ObjectKind::Dimension {
                from: Point::ZERO,
                to: Point::new(1000., 0.),
                offset: 500.,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;
    use garden_core::geometry::{Point, Shape};
    use garden_core::model::{AreaType, ObjectKind};

    pub(crate) fn tri_area(m: &MeshData) -> f64 {
        m.indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
                (((b[0] - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (b[2] - a[2])) as f64 / 2.0).abs()
            })
            .sum()
    }

    #[test]
    fn plan_maps_to_metres_with_y_up() {
        assert_eq!(mm_to_world(Point::new(4000., 3000.), 0.5), [4.0, 0.5, 3.0]);
    }

    #[test]
    fn lawn_becomes_a_flat_surface_of_the_same_area() {
        let s = build(&doc_with(vec![lawn_rect()]));
        let lawn = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Lawn)
            .unwrap();
        assert!((tri_area(lawn) - 12.0).abs() < 1e-3);
        assert!(lawn.positions.iter().all(|p| (p[1] - 0.004).abs() < 1e-3));
        assert!(lawn.normals.iter().all(|n| *n == [0.0, 1.0, 0.0]));
        assert!(s.meshes.iter().any(|m| m.kind == SurfaceKind::Ground));
    }

    #[test]
    fn upward_triangles_wind_the_same_way() {
        // front faces must point up whichever way the outline was drawn
        let cw = Shape::polyline(
            &[
                Point::ZERO,
                Point::new(1000., 0.),
                Point::new(1000., 1000.),
                Point::new(0., 1000.),
            ],
            true,
        );
        let ccw = Shape::polyline(
            &[
                Point::ZERO,
                Point::new(0., 1000.),
                Point::new(1000., 1000.),
                Point::new(1000., 0.),
            ],
            true,
        );
        for shape in [cw, ccw] {
            let (pos, idx) = flat_polygon(&shape, 0.0).unwrap();
            for t in idx.chunks(3) {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| pos[i as usize]);
                // y component of (b - a) × (c - a) is positive for counter-clockwise seen from above
                let ny = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
                assert!(ny > 0.0);
            }
        }
    }

    #[test]
    fn round_lawn_keeps_its_area() {
        let s = build(&doc_with(vec![ObjectKind::Area {
            area_type: AreaType::Lawn,
            shape: Shape::circle(Point::ZERO, 2000.),
        }]));
        let lawn = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Lawn)
            .unwrap();
        // flattening to 20 mm cuts a sliver off every curve: ~1% of a 2 m circle
        assert!((tri_area(lawn) - std::f64::consts::PI * 4.0).abs() < 0.2);
    }

    #[test]
    fn beds_are_raised() {
        let s = build(&doc_with(vec![ObjectKind::Area {
            area_type: AreaType::Bed,
            shape: Shape::rect(Point::ZERO, 2000., 1000.),
        }]));
        let bed = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Bed)
            .unwrap();
        let top = bed.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((top - 0.12).abs() < 0.01);
    }

    #[test]
    fn shapes_that_cant_be_filled_are_skipped_not_fatal() {
        let bowtie = Shape::polyline(
            &[
                Point::ZERO,
                Point::new(1000., 1000.),
                Point::new(1000., 0.),
                Point::new(0., 1000.),
            ],
            true,
        );
        let sliver = Shape::polyline(
            &[Point::ZERO, Point::new(1000., 0.), Point::new(2000., 0.)],
            true,
        );
        let two = Shape::polyline(&[Point::ZERO, Point::new(1000., 0.)], true);
        let s = build(&doc_with(vec![
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: bowtie,
            },
            ObjectKind::Area {
                area_type: AreaType::Paving,
                shape: sliver,
            },
            ObjectKind::Area {
                area_type: AreaType::Water,
                shape: two,
            },
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: Shape::rect(Point::new(5000., 0.), 1000., 1000.),
            },
        ]));
        assert_eq!(s.skipped, 3);
        assert_eq!(
            s.meshes
                .iter()
                .filter(|m| m.kind == SurfaceKind::Lawn)
                .count(),
            1
        );
    }

    #[test]
    fn hidden_layers_and_annotations_are_left_out() {
        let mut d = doc_with(vec![
            lawn_rect(),
            ObjectKind::Label {
                position: Point::ZERO,
                text: "x".into(),
                size: 300.,
            },
        ]);
        d.layers[0].visible = false;
        assert!(
            build(&d)
                .meshes
                .iter()
                .all(|m| m.kind == SurfaceKind::Ground)
        );
    }

    #[test]
    fn plot_sits_on_the_ground_and_later_objects_draw_on_top() {
        let s = build(&doc_with(vec![
            ObjectKind::PlotBoundary {
                shape: Shape::rect(Point::ZERO, 10_000., 10_000.),
            },
            lawn_rect(),
            lawn_rect(),
        ]));
        let plot = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Plot)
            .unwrap();
        assert!(plot.positions.iter().all(|p| p[1].abs() < 1e-3));
        let lawns: Vec<f32> = s
            .meshes
            .iter()
            .filter(|m| m.kind == SurfaceKind::Lawn)
            .map(|m| m.positions[0][1])
            .collect();
        assert!(lawns[1] > lawns[0]);
        let ground = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Ground)
            .unwrap();
        let min_x = ground
            .positions
            .iter()
            .map(|p| p[0])
            .fold(f32::MAX, f32::min);
        assert!(min_x <= -10.0 + 1e-3); // 10 m beyond the plot
    }
}
