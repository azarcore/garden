//! Drawing tools (rectangle, ellipse, polygon, polyline, plant).

use garden_core::command::Command;
use garden_core::geometry::{Point, Shape};
use garden_core::model::{Object, ObjectKind};
use garden_core::query::{SnapKind, snap_angle};
use garden_core::units::format_length;
use kurbo::{Rect, Vec2};

use crate::objects::default_name;
use crate::render::outline;
use crate::state::{EditorState, Gesture};
use crate::tools::{DrawTarget, Input, Key, LineTarget, Modifiers, Preview, ToolKind};

/// Height given to new structures (mm).
const DEFAULT_STRUCTURE_HEIGHT: f64 = 2200.0;

pub(crate) fn handle(s: &mut EditorState, input: Input, now_ms: i64) {
    match s.tool.clone() {
        ToolKind::Rect(target) => drag_shape(s, input, now_ms, &target, false),
        ToolKind::Ellipse(target) => drag_shape(s, input, now_ms, &target, true),
        ToolKind::Polygon(target) => poly(s, input, now_ms, PolyTarget::Closed(target)),
        ToolKind::Polyline(target) => poly(s, input, now_ms, PolyTarget::Open(target)),
        ToolKind::Plant(def) => {
            if let Input::Move(p, m) = input {
                snapped(s, p, m);
            }
            if let Input::Up(p, m) = input {
                let at = snapped(s, p, m);
                let object = def.place(at, s.active_layer());
                let id = object.id;
                let index = s.doc().objects.len();
                if s.execute(Command::Insert { object, index }, now_ms) {
                    s.selection = vec![id];
                }
            }
        }
        ToolKind::Select => {}
    }
}

fn closed_kind(target: &DrawTarget, shape: Shape) -> ObjectKind {
    match target {
        DrawTarget::Plot => ObjectKind::PlotBoundary { shape },
        DrawTarget::Area(area_type) => ObjectKind::Area {
            area_type: *area_type,
            shape,
        },
        DrawTarget::Structure(kind) => ObjectKind::Structure {
            kind: *kind,
            footprint: shape,
            height: DEFAULT_STRUCTURE_HEIGHT,
        },
    }
}

/// Add a new object on top, select it and go back to the select tool.
pub(crate) fn insert_new(s: &mut EditorState, kind: ObjectKind, now_ms: i64, back_to_select: bool) {
    let object = Object::new(default_name(&kind), kind, s.active_layer());
    let id = object.id;
    let index = s.doc().objects.len();
    if s.execute(Command::Insert { object, index }, now_ms) {
        s.selection = vec![id];
    }
    s.cancel_gesture();
    if back_to_select {
        s.tool = ToolKind::Select;
    }
}

pub(crate) fn snapped(s: &mut EditorState, p: Point, m: Modifiers) -> Point {
    let r = s.snap_point(p, m, &[]);
    s.preview.snap = Some(r).filter(|r| r.kind != SnapKind::None);
    r.point
}

/// The rectangle dragged from `start` to `current`; Shift makes it square.
fn drag_box(start: Point, current: Point, square: bool) -> Rect {
    let mut d = current - start;
    if square {
        let side = d.x.abs().max(d.y.abs());
        let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
        d = Vec2::new(sign(d.x) * side, sign(d.y) * side);
    }
    Rect::from_points(start, start + d)
}

fn box_shape(b: Rect, ellipse: bool, circle: bool) -> Shape {
    if !ellipse {
        Shape::rect(Point::new(b.x0, b.y0), b.width(), b.height())
    } else if circle {
        Shape::circle(b.center(), b.width() / 2.0)
    } else {
        Shape::ellipse(b.center(), b.width() / 2.0, b.height() / 2.0)
    }
}

fn drag_shape(s: &mut EditorState, input: Input, now_ms: i64, target: &DrawTarget, ellipse: bool) {
    match input {
        Input::Down(p, m) => {
            let start = snapped(s, p, m);
            s.gesture = Gesture::DragShape {
                start,
                current: start,
                shift: m.shift,
            };
        }
        Input::Move(p, m) | Input::Up(p, m) => {
            let current = snapped(s, p, m);
            let Gesture::DragShape { start, .. } = s.gesture else {
                return;
            };
            s.gesture = Gesture::DragShape {
                start,
                current,
                shift: m.shift,
            };
            let b = drag_box(start, current, m.shift);
            let min = s.viewport.px_to_mm(1.0);
            let big_enough = b.width() >= min && b.height() >= min;
            if let Input::Up(..) = input {
                if big_enough {
                    let shape = box_shape(b, ellipse, m.shift);
                    insert_new(s, closed_kind(target, shape), now_ms, true);
                } else {
                    s.cancel_gesture();
                }
                return;
            }
            let units = s.doc().units;
            let snap = s.preview.snap;
            s.preview = Preview {
                shapes: if big_enough {
                    vec![outline(&closed_kind(
                        target,
                        box_shape(b, ellipse, m.shift),
                    ))]
                } else {
                    vec![]
                },
                dims: vec![(
                    b.center(),
                    format!(
                        "{} × {}",
                        format_length(b.width(), units),
                        format_length(b.height(), units)
                    ),
                )],
                snap,
                marquee: None,
            };
        }
        Input::Key(Key::Escape, _) => s.cancel_gesture(),
        Input::DoubleClick(..) | Input::Key(..) => {}
    }
}

enum PolyTarget {
    Closed(DrawTarget),
    Open(LineTarget),
}

/// Angle step used when Shift is held while drawing lines.
const ANGLE_STEP: f64 = std::f64::consts::PI / 12.0;

fn poly_cursor(s: &mut EditorState, p: Point, m: Modifiers, last: Option<Point>) -> Point {
    match last {
        Some(last) if m.shift => {
            s.preview.snap = None;
            snap_angle(last, p, ANGLE_STEP)
        }
        _ => snapped(s, p, m),
    }
}

fn poly_points(s: &EditorState) -> Vec<Point> {
    match &s.gesture {
        Gesture::Poly { points, .. } => points.clone(),
        _ => Vec::new(),
    }
}

fn poly_preview(s: &mut EditorState, points: &[Point], cursor: Point) {
    let mut all = points.to_vec();
    all.push(cursor);
    let shapes = if all.len() >= 2 {
        let shape = Shape::polyline(&all, false);
        vec![crate::render::RenderShape::Path {
            d: shape.to_bez_path().to_svg(),
            closed: false,
        }]
    } else {
        Vec::new()
    };
    let dims = points
        .last()
        .map(|last| {
            (
                last.midpoint(cursor),
                format_length(last.distance(cursor), s.doc().units),
            )
        })
        .into_iter()
        .collect();
    let snap = s.preview.snap;
    s.preview = Preview {
        shapes,
        dims,
        snap,
        marquee: None,
    };
}

fn finish_poly(s: &mut EditorState, target: &PolyTarget, now_ms: i64) {
    let merge = s.viewport.px_to_mm(1.0);
    let mut points: Vec<Point> = Vec::new();
    for p in poly_points(s) {
        if points.last().is_none_or(|last| last.distance(p) > merge) {
            points.push(p);
        }
    }
    let closed = matches!(target, PolyTarget::Closed(_));
    if closed && points.len() > 1 && points[0].distance(points[points.len() - 1]) <= merge {
        points.pop();
    }
    let needed = if closed { 3 } else { 2 };
    if points.len() < needed {
        s.cancel_gesture();
        return;
    }
    let kind = match target {
        PolyTarget::Closed(t) => closed_kind(t, Shape::polyline(&points, true)),
        PolyTarget::Open(LineTarget::Path { width }) => ObjectKind::Path {
            shape: Shape::polyline(&points, false),
            width: *width,
        },
        PolyTarget::Open(LineTarget::Linear {
            kind,
            thickness,
            height,
        }) => ObjectKind::Linear {
            kind: *kind,
            shape: Shape::polyline(&points, false),
            thickness: *thickness,
            height: *height,
        },
    };
    insert_new(s, kind, now_ms, true);
}

fn poly(s: &mut EditorState, input: Input, now_ms: i64, target: PolyTarget) {
    match input {
        Input::Down(p, _) => match &mut s.gesture {
            Gesture::Poly { pressed, .. } => *pressed = Some(p),
            _ => {
                s.gesture = Gesture::Poly {
                    points: Vec::new(),
                    pressed: Some(p),
                }
            }
        },
        Input::Move(p, m) => {
            let points = poly_points(s);
            if matches!(s.gesture, Gesture::Poly { .. }) {
                let cursor = poly_cursor(s, p, m, points.last().copied());
                poly_preview(s, &points, cursor);
            } else {
                snapped(s, p, m); // show the snap indicator before the first click
            }
        }
        Input::Up(p, m) => {
            let threshold = s.drag_threshold_mm();
            let close_radius = s.viewport.px_to_mm(s.snap_radius_px);
            let Gesture::Poly { points, pressed } = s.gesture.clone() else {
                return;
            };
            let Some(down) = pressed else {
                return;
            };
            if down.distance(p) > threshold {
                // a drag, not a click: ignore it
                s.gesture = Gesture::Poly {
                    points,
                    pressed: None,
                };
                return;
            }
            let cursor = poly_cursor(s, p, m, points.last().copied());
            if matches!(target, PolyTarget::Closed(_))
                && points.len() >= 3
                && points[0].distance(cursor) <= close_radius
            {
                finish_poly(s, &target, now_ms);
                return;
            }
            let mut points = points;
            points.push(cursor);
            poly_preview(s, &points, cursor);
            s.gesture = Gesture::Poly {
                points,
                pressed: None,
            };
        }
        Input::DoubleClick(..) | Input::Key(Key::Enter, _) => finish_poly(s, &target, now_ms),
        Input::Key(Key::Backspace | Key::Delete, _) => {
            if let Gesture::Poly { points, .. } = &mut s.gesture {
                points.pop();
                let points = points.clone();
                if let Some(&last) = points.last() {
                    poly_preview(s, &points[..points.len() - 1], last);
                } else {
                    s.preview = Preview::default();
                }
            }
        }
        Input::Key(Key::Escape, _) => s.cancel_gesture(),
    }
}

#[cfg(test)]
mod tests {
    use crate::catalog::builtin_plants;
    use crate::test_util::{NO, SHIFT, click, plant_at, state_with};
    use crate::tools::{DrawTarget, Input, Key, LineTarget, ToolKind};
    use garden_core::geometry::{Point, PrimitiveHint};
    use garden_core::model::{AreaType, LinearKind, ObjectKind};

    #[test]
    fn rect_tool_draws_bed_with_live_dims() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Rect(DrawTarget::Area(AreaType::Bed)));
        s.handle(Input::Down(Point::new(1000., 1000.), NO), 1);
        s.handle(Input::Move(Point::new(5000., 4000.), NO), 1);
        assert_eq!(s.preview().dims[0].1, "4.00 m × 3.00 m");
        s.handle(Input::Up(Point::new(5000., 4000.), NO), 2);
        let o = &s.doc().objects[0];
        assert!(
            matches!(&o.kind, ObjectKind::Area { area_type: AreaType::Bed, shape } if (shape.area() - 12e6).abs() < 1.0)
        );
        assert_eq!(s.selection, vec![o.id]);
        assert_eq!(s.tool(), &ToolKind::Select);
        assert_eq!(o.name, "Bed");
    }

    #[test]
    fn rect_tool_normalises_and_squares_with_shift() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Rect(DrawTarget::Plot));
        s.handle(Input::Down(Point::new(5000., 4000.), SHIFT), 1);
        s.handle(Input::Up(Point::new(1000., 3000.), SHIFT), 1);
        let ObjectKind::PlotBoundary { shape } = &s.doc().objects[0].kind else {
            panic!()
        };
        assert_eq!(
            shape.bounding_box(),
            kurbo::Rect::new(1000., 0., 5000., 4000.)
        );
    }

    #[test]
    fn click_without_drag_creates_nothing() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Rect(DrawTarget::Area(AreaType::Lawn)));
        click(&mut s, Point::new(1000., 1000.), NO);
        assert!(s.doc().objects.is_empty());
        s.set_tool(ToolKind::Ellipse(DrawTarget::Area(AreaType::Lawn)));
        click(&mut s, Point::new(1000., 1000.), NO);
        assert!(s.doc().objects.is_empty());
    }

    #[test]
    fn escape_cancels_drag() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Rect(DrawTarget::Area(AreaType::Lawn)));
        s.handle(Input::Down(Point::new(0., 0.), NO), 1);
        s.handle(Input::Key(Key::Escape, NO), 1);
        s.handle(Input::Up(Point::new(3000., 3000.), NO), 1);
        assert!(s.doc().objects.is_empty());
        assert!(s.preview().shapes.is_empty());
    }

    #[test]
    fn ellipse_and_circle() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Ellipse(DrawTarget::Area(AreaType::Water)));
        s.handle(Input::Down(Point::ZERO, NO), 1);
        s.handle(Input::Up(Point::new(4000., 2000.), NO), 1);
        assert!(
            matches!(&s.doc().objects[0].kind, ObjectKind::Area { shape, .. }
            if shape.hint == Some(PrimitiveHint::Ellipse { rx: 2000., ry: 1000., rotation: 0. }))
        );
        s.set_tool(ToolKind::Ellipse(DrawTarget::Area(AreaType::Lawn)));
        s.handle(Input::Down(Point::ZERO, SHIFT), 2);
        s.handle(Input::Up(Point::new(4000., 2000.), SHIFT), 2);
        let ObjectKind::Area { shape, .. } = &s.doc().objects[1].kind else {
            panic!()
        };
        assert_eq!(shape.hint, Some(PrimitiveHint::Circle { radius: 2000. }));
        assert_eq!(shape.bounding_box().center(), Point::new(2000., 2000.));
    }

    #[test]
    fn polygon_closes_on_first_vertex() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Polygon(DrawTarget::Plot));
        for p in [(0., 0.), (10_000., 0.), (10_000., 8000.)] {
            click(&mut s, Point::new(p.0, p.1), NO);
        }
        click(&mut s, Point::new(40., 30.), NO);
        let ObjectKind::PlotBoundary { shape } = &s.doc().objects[0].kind else {
            panic!()
        };
        assert!(shape.closed);
        assert_eq!(shape.vertices().len(), 3);
        assert_eq!(s.tool(), &ToolKind::Select);
    }

    #[test]
    fn too_few_points_cancels() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)));
        click(&mut s, Point::new(0., 0.), NO);
        click(&mut s, Point::new(3000., 0.), NO);
        s.handle(Input::Key(Key::Enter, NO), 1);
        assert!(s.doc().objects.is_empty());
    }

    #[test]
    fn double_click_finishes_path_without_duplicate_vertex() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Polyline(LineTarget::Path { width: 900. }));
        click(&mut s, Point::new(0., 0.), NO);
        click(&mut s, Point::new(5000., 0.), NO);
        click(&mut s, Point::new(5000., 0.), NO);
        s.handle(Input::DoubleClick(Point::new(5000., 0.), NO), 1);
        let ObjectKind::Path { shape, width } = &s.doc().objects[0].kind else {
            panic!()
        };
        assert_eq!(*width, 900.);
        assert_eq!(
            shape.vertices(),
            vec![Point::new(0., 0.), Point::new(5000., 0.)]
        );
        assert!(!shape.closed);
    }

    #[test]
    fn rubber_band_shows_length_and_shift_snaps_angle() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Polyline(LineTarget::Linear {
            kind: LinearKind::Fence,
            thickness: 50.,
            height: 1800.,
        }));
        click(&mut s, Point::new(0., 0.), NO);
        s.handle(Input::Move(Point::new(3000., 0.), NO), 1);
        assert_eq!(s.preview().dims[0].1, "3.00 m");
        s.handle(Input::Move(Point::new(5000., 300.), SHIFT), 1);
        assert_eq!(s.preview().dims[0].1, "5.01 m");
        click(&mut s, Point::new(5000., 300.), SHIFT);
        s.handle(Input::Key(Key::Enter, NO), 1);
        let ObjectKind::Linear { shape, .. } = &s.doc().objects[0].kind else {
            panic!()
        };
        assert!(shape.vertices()[1].y.abs() < 1e-9);
    }

    #[test]
    fn backspace_removes_last_vertex_and_escape_cancels() {
        let mut s = state_with(vec![plant_at(9000., 9000., 600.)]);
        s.selection = vec![s.doc().objects[0].id];
        s.set_tool(ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)));
        for p in [(0., 0.), (3000., 0.), (3000., 3000.), (6000., 6000.)] {
            click(&mut s, Point::new(p.0, p.1), NO);
        }
        s.handle(Input::Key(Key::Backspace, NO), 1);
        s.handle(Input::Key(Key::Enter, NO), 1);
        assert_eq!(s.doc().objects.len(), 2); // plant not deleted; bed has 3 vertices
        let ObjectKind::Area { shape, .. } = &s.doc().objects[1].kind else {
            panic!()
        };
        assert_eq!(shape.vertices().len(), 3);
        s.set_tool(ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)));
        click(&mut s, Point::new(0., 0.), NO);
        s.handle(Input::Key(Key::Escape, NO), 1);
        assert!(s.preview().shapes.is_empty());
        assert_eq!(s.doc().objects.len(), 2);
    }

    #[test]
    fn plant_tool_places_copies_and_stays_active() {
        let mut s = state_with(vec![]);
        let def = builtin_plants()
            .into_iter()
            .find(|p| p.id == "lavender")
            .unwrap();
        s.set_tool(ToolKind::Plant(def));
        click(&mut s, Point::new(1210., 1290.), NO);
        click(&mut s, Point::new(3000., 3000.), NO);
        assert_eq!(s.doc().objects.len(), 2);
        assert!(matches!(s.tool(), ToolKind::Plant(_)));
        assert!(
            matches!(&s.doc().objects[0].kind, ObjectKind::Plant { position, spread, catalog_id, .. }
            if *position == Point::new(1200., 1300.) && *spread == 600. && catalog_id.as_deref() == Some("lavender"))
        );
    }
}
