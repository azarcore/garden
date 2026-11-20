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

/// Text height of new labels (mm).
const DEFAULT_LABEL_SIZE: f64 = 300.0;
/// How far a new dimension line sits from what it measures (mm).
const DEFAULT_DIMENSION_OFFSET: f64 = 600.0;

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
                let object = def.place(at, s.active_layer_id());
                let id = object.id;
                let index = s.doc().objects.len();
                if s.execute(Command::Insert { object, index }, now_ms) {
                    s.selection = vec![id];
                }
            }
        }
        ToolKind::SetScale => set_scale(s, input),
        ToolKind::Text => {
            if let Input::Move(p, m) = input {
                snapped(s, p, m);
            }
            if let Input::Up(p, m) = input {
                let position = snapped(s, p, m);
                let label = ObjectKind::Label {
                    position,
                    text: crate::strings::EN.new_label.to_string(),
                    size: DEFAULT_LABEL_SIZE,
                };
                insert_new(s, label, now_ms, true);
            }
        }
        ToolKind::Dimension => two_points(s, input, now_ms, true),
        ToolKind::Measure => two_points(s, input, now_ms, false),
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
            height: crate::catalog::structure_height(*kind),
        },
    }
}

/// Add a new object on top, select it and go back to the select tool.
pub(crate) fn insert_new(s: &mut EditorState, kind: ObjectKind, now_ms: i64, back_to_select: bool) {
    let object = Object::new(default_name(&kind), kind, s.active_layer_id());
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
    s.preview.guides.clear();
    let mut point = r.point;
    // line up with other objects' edges and centres, unless already on a corner/edge
    if s.guides_on(m) && matches!(r.kind, SnapKind::None | SnapKind::Grid) {
        let (others, _) = s.guide_context(&[]);
        let tol = s.viewport.px_to_mm(s.snap_radius_px);
        let (aligned, lines) = crate::guides::point_guides(p, &others, tol);
        for l in &lines {
            if l.from.x == l.to.x {
                point.x = aligned.x; // a vertical guide: x lined up
            } else {
                point.y = aligned.y;
            }
        }
        s.preview.guides = lines;
    }
    point
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
                raw_start: p,
                current: start,
                shift: m.shift,
            };
        }
        Input::Move(p, m) | Input::Up(p, m) => {
            let current = snapped(s, p, m);
            let Gesture::DragShape {
                start, raw_start, ..
            } = s.gesture
            else {
                return;
            };
            s.gesture = Gesture::DragShape {
                start,
                raw_start,
                current,
                shift: m.shift,
            };
            let b = drag_box(start, current, m.shift);
            let min = s.viewport.px_to_mm(1.0);
            // A press that barely moved is a click, even if snapping moved the ends apart.
            let dragged = raw_start.distance(p) > s.drag_threshold_mm();
            let big_enough = dragged && b.width() >= min && b.height() >= min;
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
            let guides = std::mem::take(&mut s.preview.guides);
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
                guides,
            };
        }
        Input::Key(Key::Escape, _) => s.cancel_gesture(),
        Input::DoubleClick(..) | Input::Key(..) => {}
    }
}

/// Set-scale tool: click two points on the photo (no snapping — you click on
/// what the photo shows), then type the real distance in the hint bar.
fn set_scale(s: &mut EditorState, input: Input) {
    if s.doc().background.is_none() {
        return;
    }
    let mut points = match &s.gesture {
        Gesture::Scale { points } => points.clone(),
        _ => Vec::new(),
    };
    match input {
        Input::Up(p, _) if points.len() < 2 => {
            let min = s.viewport.px_to_mm(1.0);
            if points.last().is_none_or(|last| last.distance(p) > min) {
                points.push(p);
            }
        }
        Input::Move(p, _) if points.len() == 1 => {
            let a = points[0];
            let units = s.doc().units;
            s.preview = Preview {
                shapes: vec![crate::render::RenderShape::Line { from: a, to: p }],
                dims: vec![(a.midpoint(p), format_length(a.distance(p), units))],
                ..Preview::default()
            };
            return;
        }
        Input::Key(Key::Escape, _) => {
            s.cancel_gesture();
            return;
        }
        _ => return,
    }
    if points.len() == 2 {
        let (a, b) = (points[0], points[1]);
        s.preview = Preview {
            shapes: vec![crate::render::RenderShape::Line { from: a, to: b }],
            dims: vec![(a.midpoint(b), format_length(a.distance(b), s.doc().units))],
            ..Preview::default()
        };
    }
    s.gesture = Gesture::Scale { points };
}

/// Dimension tool (`store`) and measure tool: two snapped clicks. A dimension
/// is added after the second; a measurement stays on screen until the next click.
fn two_points(s: &mut EditorState, input: Input, now_ms: i64, store: bool) {
    let mut points = match &s.gesture {
        Gesture::TwoPoints { points } => points.clone(),
        _ => Vec::new(),
    };
    let line_preview = |s: &mut EditorState, a: Point, b: Point| {
        let snap = s.preview.snap;
        let guides = std::mem::take(&mut s.preview.guides);
        s.preview = Preview {
            shapes: vec![crate::render::RenderShape::Line { from: a, to: b }],
            dims: vec![(a.midpoint(b), format_length(a.distance(b), s.doc().units))],
            snap,
            marquee: None,
            guides,
        };
    };
    match input {
        Input::Move(p, m) => {
            if points.len() == 2 {
                return; // keep showing the finished measurement
            }
            let at = snapped(s, p, m);
            if let [a] = points[..] {
                line_preview(s, a, at);
            }
        }
        Input::Up(p, m) => {
            if points.len() == 2 {
                points.clear();
                s.preview = Preview::default();
            }
            let at = snapped(s, p, m);
            let min = s.viewport.px_to_mm(1.0);
            if points.last().is_some_and(|last| last.distance(at) <= min) {
                return;
            }
            points.push(at);
            if let [a, b] = points[..] {
                if store {
                    let dim = ObjectKind::Dimension {
                        from: a,
                        to: b,
                        offset: DEFAULT_DIMENSION_OFFSET,
                    };
                    return insert_new(s, dim, now_ms, true);
                }
                line_preview(s, a, b);
            }
            s.gesture = Gesture::TwoPoints { points };
        }
        Input::Key(Key::Escape, _) => s.cancel_gesture(),
        _ => {}
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
            s.preview.guides.clear(); // the angle decides the point, not alignment
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
    let guides = std::mem::take(&mut s.preview.guides);
    s.preview = Preview {
        shapes,
        dims,
        snap,
        marquee: None,
        guides,
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
                // the click itself counts, even if alignment pulled the cursor away
                && (points[0].distance(p) <= close_radius
                    || points[0].distance(cursor) <= close_radius)
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

    #[test]
    fn undo_while_drawing_removes_last_vertex_only() {
        let mut s = state_with(vec![plant_at(9000., 9000., 600.)]);
        s.set_tool(ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)));
        for p in [(0., 0.), (3000., 0.), (3000., 3000.), (6000., 6000.)] {
            click(&mut s, Point::new(p.0, p.1), NO);
        }
        assert!(s.undo(1));
        assert_eq!(s.doc().objects.len(), 1); // the plant is still there
        s.handle(Input::Key(Key::Enter, NO), 1);
        let ObjectKind::Area { shape, .. } = &s.doc().objects[1].kind else {
            panic!()
        };
        assert_eq!(shape.vertices().len(), 3);
    }

    #[test]
    fn jittery_click_with_rect_tool_creates_nothing() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Rect(DrawTarget::Area(AreaType::Bed)));
        // ~1.4 px of travel that crosses a grid midpoint
        s.handle(Input::Down(Point::new(1040., 1040.), NO), 1);
        s.handle(Input::Up(Point::new(1060., 1060.), NO), 1);
        assert!(s.doc().objects.is_empty());
    }

    #[test]
    fn switching_tool_finishes_a_line_in_progress() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Polyline(LineTarget::Path { width: 900. }));
        click(&mut s, Point::new(0., 0.), NO);
        click(&mut s, Point::new(5000., 0.), NO);
        assert_eq!(s.points_placed(), 2);
        s.switch_tool(ToolKind::Select, 1);
        assert_eq!(s.doc().objects.len(), 1);
        assert_eq!(s.tool(), &ToolKind::Select);
        assert_eq!(s.points_placed(), 0);
    }

    #[test]
    fn switching_tool_drops_a_shape_that_cannot_be_finished() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)));
        click(&mut s, Point::new(0., 0.), NO);
        click(&mut s, Point::new(3000., 0.), NO);
        s.switch_tool(ToolKind::Select, 1);
        assert!(s.doc().objects.is_empty());
        assert_eq!(s.tool(), &ToolKind::Select);
    }

    fn photo() -> garden_core::model::BackgroundImage {
        garden_core::model::BackgroundImage {
            image_ref: "k".into(),
            mime: "image/png".into(),
            width_px: 1000,
            height_px: 800,
            origin: Point::ZERO,
            mm_per_px: 10.,
            rotation: 0.,
            opacity: 0.5,
            locked: true,
        }
    }

    #[test]
    fn set_scale_tool_measures_two_points_and_applies() {
        let mut s = state_with(vec![]);
        s.set_background(Some(photo()), 1);
        s.set_tool(ToolKind::SetScale);
        click(&mut s, Point::new(1000., 1000.), NO);
        click(&mut s, Point::new(4000., 5000.), NO); // 5000 mm on screen
        let (a, b, len) = s.pending_scale().unwrap();
        assert_eq!((a, b), (Point::new(100., 100.), Point::new(400., 500.)));
        assert!((len - 5000.).abs() < 1e-9);
        assert_eq!(
            s.apply_scale("abc", 2),
            Err(crate::state::ScaleError::NotALength)
        );
        s.apply_scale("10 m", 3).unwrap();
        let bg = s.doc().background.clone().unwrap();
        assert!((bg.mm_per_px - 20.).abs() < 1e-9);
        assert!((bg.px_to_world(Point::new(100., 100.)) - Point::new(1000., 1000.)).hypot() < 1e-6);
        assert_eq!(s.tool(), &ToolKind::Select);
        assert!(s.undo(4));
        assert!((s.doc().background.clone().unwrap().mm_per_px - 10.).abs() < 1e-9);
    }

    #[test]
    fn set_scale_needs_a_photo_and_two_distinct_points() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::SetScale);
        click(&mut s, Point::new(0., 0.), NO);
        click(&mut s, Point::new(1000., 0.), NO);
        assert!(s.pending_scale().is_none());
        assert_eq!(
            s.apply_scale("1 m", 1),
            Err(crate::state::ScaleError::NoBackground)
        );
        s.set_background(Some(photo()), 2);
        s.set_tool(ToolKind::SetScale);
        click(&mut s, Point::new(500., 500.), NO);
        click(&mut s, Point::new(500., 500.), NO); // same spot is ignored
        assert!(s.pending_scale().is_none());
        assert_eq!(s.points_placed(), 1);
    }

    #[test]
    fn text_tool_places_a_label() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Text);
        click(&mut s, Point::new(1000., 1000.), NO);
        assert!(
            matches!(&s.doc().objects[0].kind, ObjectKind::Label { text, size, .. } if text == crate::strings::EN.new_label && *size == 300.)
        );
        assert_eq!(s.tool(), &ToolKind::Select);
        assert_eq!(s.selection.len(), 1);
    }

    #[test]
    fn dimension_tool_needs_two_points() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Dimension);
        click(&mut s, Point::new(0., 0.), NO);
        assert!(s.doc().objects.is_empty());
        assert_eq!(s.points_placed(), 1);
        s.handle(Input::Move(Point::new(4000., 0.), NO), 1);
        assert_eq!(s.preview().dims[0].1, "4.00 m");
        click(&mut s, Point::new(0., 0.), NO); // same point again is ignored
        assert!(s.doc().objects.is_empty());
        click(&mut s, Point::new(4000., 0.), NO);
        assert!(
            matches!(s.doc().objects[0].kind, ObjectKind::Dimension { to, offset, .. } if to == Point::new(4000., 0.) && offset == 600.)
        );
        assert_eq!(s.tool(), &ToolKind::Select);
        assert!(s.pending_scale().is_none());
    }

    #[test]
    fn undo_mid_dimension_drops_the_point_not_the_last_change() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Plant(builtin_plants()[7].clone()));
        click(&mut s, Point::new(5000., 5000.), NO);
        s.set_tool(ToolKind::Dimension);
        click(&mut s, Point::new(0., 0.), NO);
        assert!(s.undo(1));
        assert_eq!(s.doc().objects.len(), 1); // the plant is still there
        assert_eq!(s.points_placed(), 0);
    }

    #[test]
    fn measure_tool_stores_nothing() {
        let mut s = state_with(vec![]);
        s.set_background(Some(photo()), 0);
        s.set_tool(ToolKind::Measure);
        let c0 = s.change_count();
        click(&mut s, Point::new(0., 0.), NO);
        click(&mut s, Point::new(3000., 4000.), NO);
        assert_eq!(s.preview().dims[0].1, "5.00 m");
        assert!(s.doc().objects.is_empty());
        assert_eq!(s.change_count(), c0);
        assert!(s.pending_scale().is_none()); // not mistaken for setting the scale
        assert!(!s.is_busy()); // a finished measurement doesn't hold up saving
        click(&mut s, Point::new(1000., 0.), NO); // starts a new measurement
        assert_eq!(s.points_placed(), 1);
        s.handle(Input::Key(Key::Escape, NO), 1);
        assert!(s.preview().dims.is_empty());
        assert_eq!(s.tool(), &ToolKind::Measure);
    }

    #[test]
    fn structures_get_their_own_height() {
        use crate::catalog::{FeatureId, feature_tool};
        use garden_core::model::StructureKind;
        let mut s = state_with(vec![]);
        s.set_tool(feature_tool(FeatureId::Greenhouse));
        s.handle(Input::Down(Point::new(0., 0.), NO), 1);
        s.handle(Input::Move(Point::new(2000., 3000.), NO), 1);
        s.handle(Input::Up(Point::new(2000., 3000.), NO), 1);
        assert!(matches!(
            s.doc().objects[0].kind,
            ObjectKind::Structure { kind: StructureKind::Greenhouse, height, .. } if height == 2400.
        ));
    }
}
