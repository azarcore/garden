//! Drawing tools (rectangle, ellipse, polygon, polyline, plant).

use garden_core::command::Command;
use garden_core::geometry::{Point, Shape};
use garden_core::model::{Object, ObjectKind};
use garden_core::query::SnapKind;
use garden_core::units::format_length;
use kurbo::{Rect, Vec2};

use crate::objects::default_name;
use crate::render::outline;
use crate::state::{EditorState, Gesture};
use crate::tools::{DrawTarget, Input, Key, Modifiers, Preview, ToolKind};

/// Height given to new structures (mm).
const DEFAULT_STRUCTURE_HEIGHT: f64 = 2200.0;

pub(crate) fn handle(s: &mut EditorState, input: Input, now_ms: i64) {
    match s.tool.clone() {
        ToolKind::Rect(target) => drag_shape(s, input, now_ms, &target, false),
        ToolKind::Ellipse(target) => drag_shape(s, input, now_ms, &target, true),
        ToolKind::Select | ToolKind::Polygon(_) | ToolKind::Polyline(_) | ToolKind::Plant(_) => {}
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

#[cfg(test)]
mod tests {
    use crate::test_util::{NO, SHIFT, click, state_with};
    use crate::tools::{DrawTarget, Input, Key, ToolKind};
    use garden_core::geometry::{Point, PrimitiveHint};
    use garden_core::model::{AreaType, ObjectKind};

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
}
