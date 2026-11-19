//! Handles on the selected object: resize corners, rotate, and (in node mode)
//! points and curve controls.

use garden_core::command::Command;
use garden_core::geometry::{Point, Shape};
use garden_core::model::{Object, ObjectKind};
use garden_core::query::{SnapKind, SnapResult};
use kurbo::Rect;

use crate::objects::{kind_shape, with_shape};
use crate::render::outline;
use crate::state::{EditorState, Gesture};
use crate::tools::{Modifiers, Preview, ToolKind};

/// Distance of the rotate handle above the shape, in screen pixels.
const ROTATE_OFFSET_PX: f64 = 28.0;
/// Smallest size (screen pixels) a resize can shrink a shape to.
const MIN_SIZE_PX: f64 = 1.0;
/// Rotation step with Shift held.
const ROTATE_STEP: f64 = std::f64::consts::PI / 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    NorthWest,
    NorthEast,
    SouthEast,
    SouthWest,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::NorthWest,
        Corner::NorthEast,
        Corner::SouthEast,
        Corner::SouthWest,
    ];

    pub fn of(self, r: Rect) -> Point {
        match self {
            Corner::NorthWest => Point::new(r.x0, r.y0),
            Corner::NorthEast => Point::new(r.x1, r.y0),
            Corner::SouthEast => Point::new(r.x1, r.y1),
            Corner::SouthWest => Point::new(r.x0, r.y1),
        }
    }

    pub fn opposite(self) -> Corner {
        match self {
            Corner::NorthWest => Corner::SouthEast,
            Corner::NorthEast => Corner::SouthWest,
            Corner::SouthEast => Corner::NorthWest,
            Corner::SouthWest => Corner::NorthEast,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    Resize(Corner),
    Rotate,
    /// A point of the outline (index as in `Shape::vertices`), or a dimension's
    /// start (0) / end (1).
    Vertex(usize),
    Control {
        segment: usize,
        second: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub kind: HandleKind,
    pub at: Point,
}

/// Scale factor along one axis that takes `corner` to `target`, never
/// shrinking the size below `min` (dragging past the anchor flips).
fn axis_factor(target: f64, corner: f64, anchor: f64, min: f64) -> f64 {
    let size = (corner - anchor).abs();
    if size < 1e-9 {
        return 1.0;
    }
    let f = (target - anchor) / (corner - anchor);
    if !f.is_finite() {
        return 1.0;
    }
    let smallest = min / size;
    if f.abs() < smallest {
        if f < 0.0 { -smallest } else { smallest }
    } else {
        f
    }
}

impl EditorState {
    /// The one selected object handles apply to, if it may be reshaped.
    pub(crate) fn handle_target(&self) -> Option<&Object> {
        if self.tool != ToolKind::Select || self.selection.len() != 1 {
            return None;
        }
        let doc = self.doc();
        let o = doc.object(self.selection[0])?;
        let layer_ok = doc.layer(o.layer).is_some_and(|l| l.visible && !l.locked);
        (!o.locked && layer_ok).then_some(o)
    }

    /// Handles for the single selected, unlocked object.
    pub fn handles(&self) -> Vec<Handle> {
        let Some(o) = self.handle_target() else {
            return Vec::new();
        };
        if let ObjectKind::Dimension { from, to, .. } = o.kind {
            return vec![
                Handle {
                    kind: HandleKind::Vertex(0),
                    at: from,
                },
                Handle {
                    kind: HandleKind::Vertex(1),
                    at: to,
                },
            ];
        }
        let Some(shape) = kind_shape(&o.kind) else {
            return Vec::new();
        };
        let b = shape.bounding_box();
        let mut h: Vec<Handle> = Corner::ALL
            .into_iter()
            .map(|c| Handle {
                kind: HandleKind::Resize(c),
                at: c.of(b),
            })
            .collect();
        h.push(Handle {
            kind: HandleKind::Rotate,
            at: Point::new(
                b.center().x,
                b.y0 - self.viewport.px_to_mm(ROTATE_OFFSET_PX),
            ),
        });
        h
    }

    /// The handle under `p` (nearest one within the hit radius).
    pub(crate) fn handle_at(&self, p: Point) -> Option<Handle> {
        let tol = self.viewport.px_to_mm(self.hit_radius_px);
        self.handles()
            .into_iter()
            .map(|h| (h.at.distance(p), h))
            .filter(|(d, _)| *d <= tol)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, h)| h)
    }

    /// The object as it would be with handle `kind` (which was at `at`)
    /// dragged by the pointer from `press` to `p`.
    fn reshaped(
        &self,
        o: &Object,
        kind: HandleKind,
        at: Point,
        press: Point,
        p: Point,
        m: Modifiers,
    ) -> Option<(ObjectKind, Option<SnapResult>)> {
        let target = at + (p - press);
        let snapped = || self.snap_point(target, m, &[o.id]);
        if let ObjectKind::Dimension { from, to, offset } = o.kind {
            let s = snapped();
            let (from, to) = match kind {
                HandleKind::Vertex(0) => (s.point, to),
                HandleKind::Vertex(1) => (from, s.point),
                _ => return None,
            };
            return Some((ObjectKind::Dimension { from, to, offset }, Some(s)));
        }
        let shape: &Shape = kind_shape(&o.kind)?;
        let (new, snap) = match kind {
            HandleKind::Resize(c) => {
                let s = snapped();
                let b = shape.bounding_box();
                let (corner, anchor) = (c.of(b), c.opposite().of(b));
                let min = self.viewport.px_to_mm(MIN_SIZE_PX);
                let mut sx = axis_factor(s.point.x, corner.x, anchor.x, min);
                let mut sy = axis_factor(s.point.y, corner.y, anchor.y, min);
                if m.shift {
                    let sized = |f: f64, size: f64| if size < 1e-9 { 0.0 } else { f.abs() };
                    let k = sized(sx, b.width()).max(sized(sy, b.height()));
                    if k > 0.0 {
                        sx = k.copysign(sx);
                        sy = k.copysign(sy);
                    }
                }
                (shape.scaled(anchor, sx, sy), Some(s))
            }
            HandleKind::Rotate => {
                let c = shape.bounding_box().center();
                let a0 = (at - c).atan2();
                let mut angle = (target - c).atan2() - a0;
                if m.shift {
                    angle = (angle / ROTATE_STEP).round() * ROTATE_STEP;
                }
                (shape.rotated(c, angle), None)
            }
            HandleKind::Vertex(i) => {
                let s = snapped();
                (shape.move_vertex(i, s.point)?, Some(s))
            }
            HandleKind::Control { segment, second } => {
                (shape.move_control(segment, second, target)?, None)
            }
        };
        new.is_finite().then(|| (with_shape(&o.kind, new), snap))
    }

    /// Pointer moved while dragging a handle: update the live preview.
    pub(crate) fn drag_handle(&mut self, p: Point, m: Modifiers) {
        let threshold = self.drag_threshold_mm();
        let Gesture::Handle {
            kind,
            at,
            press,
            moved,
            ..
        } = self.gesture.clone()
        else {
            return;
        };
        if !moved && press.distance(p) <= threshold {
            return;
        }
        let Some(o) = self.handle_target().cloned() else {
            return self.cancel_gesture();
        };
        let result = self.reshaped(&o, kind, at, press, p, m);
        self.preview = Preview {
            shapes: result.iter().map(|(k, _)| outline(k)).collect(),
            snap: result
                .as_ref()
                .and_then(|(_, s)| *s)
                .filter(|s| s.kind != SnapKind::None),
            ..Preview::default()
        };
        self.gesture = Gesture::Handle {
            kind,
            at,
            press,
            moved: true,
            result: result.map(|(k, _)| k),
        };
    }

    /// Pointer released: commit the reshaped object as one undo step.
    pub(crate) fn finish_handle(&mut self, now_ms: i64) {
        let gesture = std::mem::replace(&mut self.gesture, Gesture::Idle);
        self.preview = Preview::default();
        if let Gesture::Handle {
            result: Some(kind),
            moved: true,
            ..
        } = gesture
            && let Some(o) = self.handle_target().cloned()
            && o.kind != kind
        {
            self.execute(
                Command::Replace {
                    object: Object { kind, ..o },
                },
                now_ms,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EditorState;
    use crate::test_util::{NO, SHIFT, plant_at, state_with};
    use crate::tools::{Input, Modifiers};
    use garden_core::geometry::{Point, Shape};
    use garden_core::model::{AreaType, ObjectKind};

    fn bed() -> ObjectKind {
        ObjectKind::Area {
            area_type: AreaType::Bed,
            shape: Shape::rect(Point::ZERO, 4000., 2000.),
        }
    }

    pub(crate) fn drag(s: &mut EditorState, from: Point, to: Point, m: Modifiers) {
        s.handle(Input::Down(from, m), 1);
        s.handle(Input::Move(to, m), 1);
        s.handle(Input::Up(to, m), 2);
    }

    fn shape(s: &EditorState) -> Shape {
        match &s.doc().objects[0].kind {
            ObjectKind::Area { shape, .. } => shape.clone(),
            _ => panic!(),
        }
    }

    const ALT: Modifiers = Modifiers {
        alt: true,
        shift: false,
        ctrl: false,
    };

    #[test]
    fn selected_shape_shows_resize_and_rotate_handles() {
        let mut s = state_with(vec![bed()]);
        assert!(s.handles().is_empty());
        s.selection = vec![s.doc().objects[0].id];
        let h = s.handles();
        assert_eq!(h.len(), 5);
        assert!(h.contains(&Handle {
            kind: HandleKind::Resize(Corner::SouthEast),
            at: Point::new(4000., 2000.)
        }));
        assert!(
            h.iter().any(|h| h.kind == HandleKind::Rotate
                && (h.at - Point::new(2000., -560.)).hypot() < 1e-9)
        );
    }

    #[test]
    fn dragging_a_corner_resizes_as_one_undo_step() {
        let mut s = state_with(vec![bed()]);
        s.selection = vec![s.doc().objects[0].id];
        drag(
            &mut s,
            Point::new(4000., 2000.),
            Point::new(6000., 3000.),
            NO,
        );
        assert_eq!(
            shape(&s).bounding_box(),
            kurbo::Rect::new(0., 0., 6000., 3000.)
        );
        assert!(s.undo(3));
        assert_eq!(
            shape(&s).bounding_box(),
            kurbo::Rect::new(0., 0., 4000., 2000.)
        );
        assert!(!s.can_undo());
    }

    #[test]
    fn shift_keeps_proportions() {
        let mut s = state_with(vec![bed()]);
        s.selection = vec![s.doc().objects[0].id];
        drag(
            &mut s,
            Point::new(4000., 2000.),
            Point::new(8000., 2500.),
            SHIFT,
        );
        assert_eq!(
            shape(&s).bounding_box(),
            kurbo::Rect::new(0., 0., 8000., 4000.)
        );
    }

    #[test]
    fn resizing_past_the_anchor_flips_and_never_collapses() {
        let mut s = state_with(vec![bed()]);
        s.selection = vec![s.doc().objects[0].id];
        drag(&mut s, Point::new(4000., 2000.), Point::new(0., 0.), ALT);
        let b = shape(&s).bounding_box();
        assert!(b.width() >= 20. - 1e-9 && b.height() >= 20. - 1e-9);
        assert!(b.x0.is_finite() && b.y1.is_finite());
        drag(
            &mut s,
            Point::new(b.x1, b.y1),
            Point::new(-3000., -1000.),
            ALT,
        );
        let b = shape(&s).bounding_box();
        assert!((b.width() - 3000.).abs() < 1.0 && (b.height() - 1000.).abs() < 1.0);
    }

    #[test]
    fn rotate_handle_turns_the_shape_with_15_degree_snap() {
        let mut s = state_with(vec![bed()]);
        s.selection = vec![s.doc().objects[0].id];
        // centre (2000,1000); handle straight up; drag to ≈ 92° → snaps to 90°
        drag(
            &mut s,
            Point::new(2000., -560.),
            Point::new(4000., 1070.),
            SHIFT,
        );
        let b = shape(&s).bounding_box();
        assert!((b.width() - 2000.).abs() < 1.0 && (b.height() - 4000.).abs() < 1.0);
    }

    #[test]
    fn escape_cancels_a_handle_drag() {
        let mut s = state_with(vec![bed()]);
        s.selection = vec![s.doc().objects[0].id];
        s.handle(Input::Down(Point::new(4000., 2000.), NO), 1);
        s.handle(Input::Move(Point::new(6000., 3000.), NO), 1);
        assert!(!s.preview().shapes.is_empty());
        s.handle(Input::Key(crate::tools::Key::Escape, NO), 1);
        s.handle(Input::Up(Point::new(6000., 3000.), NO), 1);
        assert_eq!(
            shape(&s).bounding_box(),
            kurbo::Rect::new(0., 0., 4000., 2000.)
        );
        assert_eq!(s.selection.len(), 1);
    }

    #[test]
    fn dimension_endpoints_can_be_dragged() {
        let dim = ObjectKind::Dimension {
            from: Point::ZERO,
            to: Point::new(3000., 0.),
            offset: 600.,
        };
        let mut s = state_with(vec![dim]);
        s.selection = vec![s.doc().objects[0].id];
        assert_eq!(s.handles().len(), 2);
        drag(&mut s, Point::new(3000., 0.), Point::new(5000., 0.), NO);
        assert!(
            matches!(s.doc().objects[0].kind, ObjectKind::Dimension { to, .. } if to == Point::new(5000., 0.))
        );
    }

    #[test]
    fn locked_objects_and_plants_have_no_handles() {
        let mut s = state_with(vec![bed(), plant_at(9000., 9000., 600.)]);
        s.selection = vec![s.doc().objects[1].id];
        assert!(s.handles().is_empty());
        let mut o = s.doc().objects[0].clone();
        o.locked = true;
        s.replace_object(o, 1).unwrap();
        s.selection = vec![s.doc().objects[0].id];
        assert!(s.handles().is_empty());
    }
}
