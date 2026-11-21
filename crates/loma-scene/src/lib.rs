#![forbid(unsafe_code)]
//! A garden design as plain 3D geometry: triangle meshes in metres (Y up),
//! plant instances and a starting camera. No graphics library here, so it is
//! tested natively; `loma-3d` draws it.

mod lines;
mod surfaces;

use kurbo::Rect;
use loma_core::geometry::Point;
use loma_core::model::{
    AreaType, Color, Document, LinearKind, Object, ObjectKind, PlantCategory, PlantSymbol,
};
use loma_editor::objects::object_bounds;
use loma_editor::render::default_style;

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
const LAYER_STEP_M: f64 = 0.001;
/// Flat surfaces (other than the plot) start this far above the ground.
const FLAT_BASE_M: f64 = 0.004;
/// Flat surfaces never rise more than this above their base height.
const MAX_LIFT_M: f64 = 0.03;
/// Raised beds stand this high.
const BED_HEIGHT_M: f64 = 0.12;
/// Plants are never flatter than this (metres).
const MIN_PLANT_HEIGHT_M: f64 = 0.05;
const MIN_CAMERA_RADIUS_M: f64 = 8.0;
const EMPTY_CAMERA_RADIUS_M: f64 = 20.0;
/// Camera tilt above the horizon (radians).
const CAMERA_PITCH: f32 = 0.6;
const GROUND_COLOR: Color = Color(196, 207, 184, 255);
const PLOT_COLOR: Color = Color(236, 238, 226, 255);

/// Height of a flat surface before the per-object step: the plot at ground
/// level, everything else just above it. What is drawn later sits higher, as in 2D.
fn surface_height(kind: SurfaceKind) -> f64 {
    match kind {
        SurfaceKind::Plot => 0.0,
        _ => FLAT_BASE_M,
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
    // How many flat surfaces came before: each sits a hair above the last.
    let mut surfaces_so_far = 0usize;
    // Raised beds (outline and top height), so plants in them stand on the soil.
    let mut beds: Vec<(&loma_core::geometry::Shape, f64)> = Vec::new();
    let mut plants: Vec<&Object> = Vec::new();
    for o in &doc.objects {
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
        // the plot is the ground itself: it stays at the bottom, unstacked
        let flat = matches!(o.kind, ObjectKind::Area { .. } | ObjectKind::Path { .. });
        let lift = if flat {
            surfaces_so_far += 1;
            (LAYER_STEP_M * (surfaces_so_far - 1) as f64).min(MAX_LIFT_M)
        } else {
            0.0
        };
        let mut mb = MeshBuilder::default();
        let built = match &o.kind {
            ObjectKind::PlotBoundary { shape } => surfaces::add_top(&mut mb, shape, lift)
                .map(|_| (SurfaceKind::Plot, rgba(PLOT_COLOR))),
            ObjectKind::Area { area_type, shape } => {
                let kind = area_kind(*area_type);
                let color = fill_color(o, PLOT_COLOR);
                if kind == SurfaceKind::Bed {
                    let top = BED_HEIGHT_M + lift;
                    let made = surfaces::add_prism(&mut mb, shape, 0.0, top).map(|_| (kind, color));
                    if made.is_some() {
                        beds.push((shape, top));
                    }
                    made
                } else {
                    surfaces::add_top(&mut mb, shape, surface_height(kind) + lift)
                        .map(|_| (kind, color))
                }
            }
            ObjectKind::Path { shape, width } => {
                let y = surface_height(SurfaceKind::Path) + lift;
                lines::add_strip(&mut mb, &lines::line_m(shape), width / 1000.0, y);
                Some((SurfaceKind::Path, fill_color(o, PLOT_COLOR)))
            }
            ObjectKind::Linear {
                kind,
                shape,
                thickness,
                height,
            } => {
                let surface = match kind {
                    LinearKind::Fence => SurfaceKind::Fence,
                    LinearKind::Wall => SurfaceKind::Wall,
                    LinearKind::Hedge => SurfaceKind::Hedge,
                };
                let joints = *kind != LinearKind::Fence;
                let pts = lines::line_m(shape);
                lines::add_boxes(
                    &mut mb,
                    &pts,
                    thickness / 1000.0,
                    height.max(1.0) / 1000.0,
                    joints,
                );
                if mb.is_empty() {
                    continue; // a line without length: nothing to stand up
                }
                Some((surface, fill_color(o, PLOT_COLOR)))
            }
            ObjectKind::Structure {
                footprint, height, ..
            } => surfaces::add_prism(&mut mb, footprint, 0.0, height.max(1.0) / 1000.0 + lift)
                .map(|_| (SurfaceKind::Structure, fill_color(o, PLOT_COLOR))),
            ObjectKind::Plant { .. } => {
                plants.push(o); // placed once every bed is known
                continue;
            }
            ObjectKind::Label { .. } | ObjectKind::Dimension { .. } => continue,
        };
        match built {
            Some((kind, color)) if !mb.is_empty() => scene.meshes.push(mb.finish(kind, color)),
            _ => scene.skipped += 1,
        }
    }
    for o in plants {
        let ObjectKind::Plant {
            position,
            spread,
            height,
            category,
            symbol,
            ..
        } = &o.kind
        else {
            continue;
        };
        // stand on the highest raised bed it is planted in, else on the ground
        let ground_y = beds
            .iter()
            .filter(|(shape, _)| shape.contains(*position))
            .map(|(_, top)| *top)
            .fold(0.0, f64::max);
        let mut color = fill_color(o, PLOT_COLOR);
        color[3] = 1.0;
        scene.plants.push(PlantInstance {
            position: mm_to_world(*position, ground_y),
            spread: (spread.max(1.0) / 1000.0) as f32,
            height: (height / 1000.0).max(MIN_PLANT_HEIGHT_M) as f32,
            category: *category,
            symbol: *symbol,
            color,
        });
    }
    scene.meshes.insert(0, ground(bounds));
    scene.camera = Some(camera(bounds));
    scene
}

/// Looking at the middle of the design from the bottom of the plan, far enough
/// back to see all of it.
fn camera(bounds: Option<Rect>) -> CameraStart {
    let framed = bounds.filter(|b| [b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite()));
    let (target, radius) = match framed {
        Some(b) => {
            let c = b.center();
            let diagonal = (b.width().hypot(b.height())) / 1000.0;
            (
                [(c.x / 1000.0) as f32, 0.0, (c.y / 1000.0) as f32],
                (1.2 * diagonal).max(MIN_CAMERA_RADIUS_M),
            )
        }
        None => ([0.0; 3], EMPTY_CAMERA_RADIUS_M),
    };
    CameraStart {
        target,
        radius: radius as f32,
        yaw: 0.0,
        pitch: CAMERA_PITCH,
    }
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
    use loma_core::command::Command;
    use loma_core::geometry::{Point, Shape};
    use loma_core::model::*;

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

    /// One of every object kind (copy of loma_editor's test sample).
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
    use loma_core::geometry::{Point, Shape};
    use loma_core::model::{AreaType, LinearKind, ObjectKind, StructureKind};

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

    #[test]
    fn paths_are_strips_of_their_width() {
        let s = build(&doc_with(vec![ObjectKind::Path {
            shape: Shape::polyline(&[Point::ZERO, Point::new(5000., 0.)], false),
            width: 1000.,
        }]));
        let p = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Path)
            .unwrap();
        let zs: Vec<f32> = p.positions.iter().map(|v| v[2]).collect();
        assert!((zs.iter().cloned().fold(f32::MIN, f32::max) - 0.5).abs() < 1e-3);
        assert!((zs.iter().cloned().fold(f32::MAX, f32::min) + 0.5).abs() < 1e-3);
        assert!(p.normals.iter().all(|n| *n == [0.0, 1.0, 0.0]));
    }

    #[test]
    fn walls_stand_to_their_height() {
        let s = build(&doc_with(vec![ObjectKind::Linear {
            kind: LinearKind::Wall,
            shape: Shape::polyline(
                &[Point::ZERO, Point::new(3000., 0.), Point::new(3000., 2000.)],
                false,
            ),
            thickness: 200.,
            height: 1800.,
        }]));
        let w = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Wall)
            .unwrap();
        let top = w.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((top - 1.8).abs() < 1e-4);
        assert!(w.positions.iter().all(|p| p[1] >= -1e-6));
        assert_eq!(w.indices.len() % 3, 0);
        // the wall is 0.2 m thick around the line
        let max_z_first_leg = w
            .positions
            .iter()
            .filter(|p| p[0] < 2.5)
            .map(|p| p[2])
            .fold(f32::MIN, f32::max);
        assert!((max_z_first_leg - 0.1).abs() < 1e-3);
    }

    #[test]
    fn sheds_are_solid_to_their_height() {
        let s = build(&doc_with(vec![ObjectKind::Structure {
            kind: StructureKind::Shed,
            footprint: Shape::rect(Point::ZERO, 2000., 1500.),
            height: 2200.,
        }]));
        let shed = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Structure)
            .unwrap();
        assert!(shed.positions.iter().any(|p| (p[1] - 2.2).abs() < 1e-4));
        assert!(shed.normals.iter().any(|n| n[0].abs() > 0.99)); // has side walls
    }

    #[test]
    fn side_walls_face_outwards() {
        let s = build(&doc_with(vec![ObjectKind::Structure {
            kind: StructureKind::Shed,
            footprint: Shape::rect(Point::ZERO, 2000., 2000.),
            height: 2000.,
        }]));
        let shed = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Structure)
            .unwrap();
        for (p, n) in shed.positions.iter().zip(&shed.normals) {
            if n[1] == 0.0 {
                // centre is (1, 1): an outward normal points away from it
                assert!((p[0] - 1.0) * n[0] + (p[2] - 1.0) * n[2] > 0.0);
            }
        }
    }

    #[test]
    fn plants_keep_their_mature_size() {
        let s = build(&doc_with(vec![plant_at(2000., 1000., 600.)]));
        assert_eq!(s.plants.len(), 1);
        assert_eq!(s.plants[0].position, [2.0, 0.0, 1.0]);
        assert!((s.plants[0].spread - 0.6).abs() < 1e-6);
        assert!((s.plants[0].height - 0.5).abs() < 1e-6);
        assert_eq!(s.plants[0].color[3], 1.0);
    }

    #[test]
    fn camera_frames_the_garden_and_an_empty_design() {
        let s = build(&doc_with(vec![lawn_rect()])); // 4 × 3 m at the origin
        let c = s.camera.unwrap();
        assert_eq!(c.target, [2.0, 0.0, 1.5]);
        assert!(c.radius >= 8.0 && c.radius.is_finite());
        let empty = build(&doc_with(vec![]));
        let c = empty.camera.unwrap();
        assert!(c.radius.is_finite() && c.target.iter().all(|v| v.is_finite()));
        assert!(empty.meshes.iter().any(|m| m.kind == SurfaceKind::Ground));
    }

    #[test]
    fn every_mesh_is_well_formed() {
        let s = build(&doc_with(all_kinds_sample()));
        assert_eq!(s.skipped, 0);
        for kind in [
            SurfaceKind::Plot,
            SurfaceKind::Lawn,
            SurfaceKind::Path,
            SurfaceKind::Hedge,
            SurfaceKind::Structure,
        ] {
            assert!(s.meshes.iter().any(|m| m.kind == kind), "{kind:?}");
        }
        for m in &s.meshes {
            assert_eq!(m.positions.len(), m.normals.len());
            assert!(m.indices.iter().all(|i| (*i as usize) < m.positions.len()));
            assert!(m.positions.iter().flatten().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn degenerate_lines_dont_break_anything() {
        let one_point = Shape::polyline(&[Point::new(1000., 1000.)], false);
        let s = build(&doc_with(vec![
            ObjectKind::Path {
                shape: one_point.clone(),
                width: 900.,
            },
            ObjectKind::Linear {
                kind: LinearKind::Fence,
                shape: one_point,
                thickness: 50.,
                height: 1800.,
            },
            ObjectKind::Linear {
                kind: LinearKind::Hedge,
                shape: Shape::polyline(&[Point::ZERO, Point::ZERO, Point::new(1000., 0.)], false),
                thickness: 600.,
                height: 1500.,
            },
        ]));
        for m in &s.meshes {
            assert!(m.positions.iter().flatten().all(|v| v.is_finite()));
        }
        assert!(s.meshes.iter().any(|m| m.kind == SurfaceKind::Hedge));
    }

    #[test]
    fn many_plants_dont_lift_later_surfaces() {
        let mut kinds: Vec<ObjectKind> = (0..400)
            .map(|i| plant_at(i as f64 * 10., 0., 300.))
            .collect();
        kinds.push(lawn_rect());
        let s = build(&doc_with(kinds));
        let lawn = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Lawn)
            .unwrap();
        assert!(
            lawn.positions[0][1] < 0.01,
            "lawn at {}",
            lawn.positions[0][1]
        );
    }

    #[test]
    fn surface_lift_is_capped() {
        let kinds: Vec<ObjectKind> = (0..1000).map(|_| lawn_rect()).collect();
        let s = build(&doc_with(kinds));
        let top = s
            .meshes
            .iter()
            .flat_map(|m| m.positions.iter().map(|p| p[1]))
            .fold(f32::MIN, f32::max);
        assert!(top < 0.04, "surfaces reach {top}");
    }

    #[test]
    fn plants_in_a_raised_bed_stand_on_it() {
        let s = build(&doc_with(vec![
            plant_at(1000., 500., 300.), // listed before the bed it sits in
            ObjectKind::Area {
                area_type: AreaType::Bed,
                shape: Shape::rect(Point::ZERO, 2000., 1000.),
            },
            plant_at(5000., 500., 300.), // outside it
        ]));
        let bed_top = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Bed)
            .unwrap()
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max);
        assert!((s.plants[0].position[1] - bed_top).abs() < 1e-6);
        assert_eq!(s.plants[1].position[1], 0.0);
    }

    #[test]
    fn open_or_broken_outlines_are_not_filled() {
        let open = Shape::polyline(
            &[Point::ZERO, Point::new(2000., 0.), Point::new(0., 2000.)],
            false,
        );
        let mut broken = Shape::rect(Point::ZERO, 2000., 2000.);
        broken.segments[1] = loma_core::geometry::Segment::Line {
            to: Point::new(f64::NAN, 2000.),
        };
        let mut d = doc_with(vec![ObjectKind::Area {
            area_type: AreaType::Lawn,
            shape: open,
        }]);
        // commands refuse non-finite shapes, but a hand-edited file could still carry one
        let layer = d.layers[0].id;
        d.objects.push(loma_core::model::Object::new(
            "broken",
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: broken,
            },
            layer,
        ));
        let s = build(&d);
        assert_eq!(s.skipped, 2);
        assert!(s.meshes.iter().all(|m| m.kind != SurfaceKind::Lawn));
    }

    #[test]
    fn later_surfaces_sit_on_top_like_in_2d() {
        // a pond drawn on paving, and paving laid over a lawn
        let s = build(&doc_with(vec![
            ObjectKind::Area {
                area_type: AreaType::Paving,
                shape: Shape::rect(Point::ZERO, 4000., 4000.),
            },
            ObjectKind::Area {
                area_type: AreaType::Water,
                shape: Shape::circle(Point::new(2000., 2000.), 600.),
            },
            lawn_rect(),
            ObjectKind::Area {
                area_type: AreaType::Paving,
                shape: Shape::rect(Point::new(1000., 1000.), 1000., 1000.),
            },
        ]));
        let y = |kind: SurfaceKind, nth: usize| {
            s.meshes
                .iter()
                .filter(|m| m.kind == kind)
                .nth(nth)
                .unwrap()
                .positions[0][1]
        };
        assert!(y(SurfaceKind::Water, 0) > y(SurfaceKind::Paving, 0));
        assert!(y(SurfaceKind::Paving, 1) > y(SurfaceKind::Lawn, 0));
        // far enough apart not to flicker when seen from a distance
        assert!(y(SurfaceKind::Water, 0) - y(SurfaceKind::Paving, 0) >= 0.001 - 1e-6);
    }

    #[test]
    fn the_plot_stays_underneath_whatever_order_it_was_drawn_in() {
        let mut kinds: Vec<ObjectKind> = (0..10).map(|_| lawn_rect()).collect();
        kinds.push(ObjectKind::PlotBoundary {
            shape: Shape::rect(Point::ZERO, 10_000., 10_000.),
        });
        let s = build(&doc_with(kinds));
        let plot = s
            .meshes
            .iter()
            .find(|m| m.kind == SurfaceKind::Plot)
            .unwrap()
            .positions[0][1];
        let lowest_lawn = s
            .meshes
            .iter()
            .filter(|m| m.kind == SurfaceKind::Lawn)
            .map(|m| m.positions[0][1])
            .fold(f32::MAX, f32::min);
        assert!(plot < lowest_lawn, "plot {plot} vs lawn {lowest_lawn}");
    }

    #[test]
    fn small_round_ponds_are_kept() {
        let s = build(&doc_with(vec![ObjectKind::Area {
            area_type: AreaType::Water,
            shape: Shape::circle(Point::ZERO, 600.),
        }]));
        assert_eq!(s.skipped, 0);
        assert!(s.meshes.iter().any(|m| m.kind == SurfaceKind::Water));
    }

    #[test]
    fn every_template_builds_in_3d() {
        use loma_editor::templates::{instantiate, templates};
        for t in templates() {
            let s = build(&instantiate(t.id, t.name, 0));
            assert_eq!(s.skipped, 0, "{}", t.name);
            assert!(!s.plants.is_empty(), "{}", t.name);
        }
    }
}
