//! The editor as the UI sees it: document + history, view, selection and the active tool.

use garden_core::command::{Command, CommandError};
use garden_core::editor::Editor;
use garden_core::geometry::Point;
use garden_core::model::{Document, LayerId, Object, ObjectId};
use garden_core::query::{SnapKind, SnapResult, SnapSettings, hit_test, snap};
use garden_core::units::UnitSystem;
use kurbo::{Rect, Vec2};

use crate::objects::{object_bounds, translate_kind};
use crate::render::outline;
use crate::tools::{Input, Key, Modifiers, Preview, ToolKind};
use crate::viewport::Viewport;

/// Pointer travel (px) below which a press counts as a click.
const DRAG_THRESHOLD_PX: f64 = 3.0;
/// Offset of duplicated objects.
const DUPLICATE_OFFSET: Vec2 = Vec2::new(500.0, 500.0);

/// What the pointer is doing right now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Gesture {
    Idle,
    /// Select tool: pressed on a selected object; becomes a move once dragged.
    PressedOnObject {
        start: Point,
    },
    /// Select tool: pressed on empty space; becomes a marquee once dragged.
    PressedOnEmpty {
        start: Point,
        additive: bool,
    },
    Moving {
        start: Point,
        delta: Vec2,
    },
    Marquee {
        start: Point,
        current: Point,
        additive: bool,
    },
    /// Rectangle / ellipse tools.
    DragShape {
        start: Point,
        current: Point,
        shift: bool,
    },
    /// Polygon / polyline tools: placed points and where the last press began.
    Poly {
        points: Vec<Point>,
        pressed: Option<Point>,
    },
}

pub struct EditorState {
    pub viewport: Viewport,
    pub selection: Vec<ObjectId>,
    /// Grid spacing in mm.
    pub grid: f64,
    pub snap_grid: bool,
    pub snap_objects: bool,
    pub snap_radius_px: f64,
    pub hit_radius_px: f64,
    pub(crate) editor: Editor,
    pub(crate) tool: ToolKind,
    pub(crate) gesture: Gesture,
    pub(crate) preview: Preview,
}

impl EditorState {
    pub fn new(doc: Document, width_px: f64, height_px: f64) -> EditorState {
        let mut s = EditorState {
            viewport: Viewport::new(width_px, height_px),
            selection: Vec::new(),
            grid: 100.0,
            snap_grid: true,
            snap_objects: true,
            snap_radius_px: 8.0,
            hit_radius_px: 6.0,
            editor: Editor::new(doc),
            tool: ToolKind::Select,
            gesture: Gesture::Idle,
            preview: Preview::default(),
        };
        s.fit_to_content();
        s
    }

    pub fn doc(&self) -> &Document {
        self.editor.doc()
    }

    pub fn tool(&self) -> &ToolKind {
        &self.tool
    }

    /// Switch tools, abandoning any gesture in progress.
    pub fn set_tool(&mut self, tool: ToolKind) {
        self.tool = tool;
        self.cancel_gesture();
    }

    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    pub(crate) fn cancel_gesture(&mut self) {
        self.gesture = Gesture::Idle;
        self.preview = Preview::default();
    }

    pub fn handle(&mut self, input: Input, now_ms: i64) {
        match self.tool {
            ToolKind::Select => self.handle_select(input, now_ms),
            _ => self.handle_draw(input, now_ms),
        }
    }

    pub fn undo(&mut self, now_ms: i64) -> bool {
        let done = self.editor.undo(now_ms);
        self.after_history_change();
        done
    }

    pub fn redo(&mut self, now_ms: i64) -> bool {
        let done = self.editor.redo(now_ms);
        self.after_history_change();
        done
    }

    fn after_history_change(&mut self) {
        let doc = self.editor.doc();
        self.selection.retain(|id| doc.object(*id).is_some());
        self.cancel_gesture();
    }

    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    fn unlocked_selection(&self) -> Vec<&Object> {
        self.doc()
            .objects
            .iter()
            .filter(|o| !o.locked && self.selection.contains(&o.id))
            .collect()
    }

    /// Delete selected (unlocked) objects as one undo step.
    pub fn delete_selection(&mut self, now_ms: i64) {
        let commands: Vec<_> = self
            .unlocked_selection()
            .iter()
            .map(|o| Command::Delete { id: o.id })
            .collect();
        if !commands.is_empty() {
            self.execute(Command::Batch { commands }, now_ms);
        }
        self.selection.clear();
    }

    /// Copy the selection, offset by half a metre, and select the copies.
    pub fn duplicate_selection(&mut self, now_ms: i64) {
        let doc = self.doc();
        let originals: Vec<Object> = doc
            .objects
            .iter()
            .filter(|o| self.selection.contains(&o.id))
            .cloned()
            .collect();
        let Some(top) = originals.iter().filter_map(|o| doc.index_of(o.id)).max() else {
            return;
        };
        let copies: Vec<Object> = originals
            .into_iter()
            .map(|o| Object {
                id: ObjectId::new(),
                kind: translate_kind(&o.kind, DUPLICATE_OFFSET),
                revision: 0,
                locked: false,
                ..o
            })
            .collect();
        let ids: Vec<ObjectId> = copies.iter().map(|o| o.id).collect();
        let commands = copies
            .into_iter()
            .enumerate()
            .map(|(i, object)| Command::Insert {
                object,
                index: top + 1 + i,
            })
            .collect();
        if self.execute(Command::Batch { commands }, now_ms) {
            self.selection = ids;
        }
    }

    /// Replace one object (inspector edits, lock toggles).
    pub fn replace_object(&mut self, object: Object, now_ms: i64) -> Result<(), CommandError> {
        self.editor.execute(Command::Replace { object }, now_ms)
    }

    pub fn set_units(&mut self, units: UnitSystem, now_ms: i64) {
        if self.doc().units != units {
            self.execute(Command::SetUnits { units }, now_ms);
        }
    }

    /// Zoom to show every visible object (or a 20 × 20 m area when empty).
    pub fn fit_to_content(&mut self) {
        let doc = self.editor.doc();
        let bounds = doc
            .objects
            .iter()
            .filter(|o| doc.layer(o.layer).is_some_and(|l| l.visible))
            .map(|o| object_bounds(&o.kind))
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::new(0.0, 0.0, 20_000.0, 20_000.0));
        self.viewport.fit(bounds, 40.0);
    }

    pub(crate) fn snap_point(&self, p: Point, m: Modifiers, exclude: &[ObjectId]) -> SnapResult {
        if m.alt {
            return SnapResult {
                point: p,
                kind: SnapKind::None,
            };
        }
        let settings = SnapSettings {
            grid: self.snap_grid.then_some(self.grid),
            objects: self.snap_objects,
            radius: self.viewport.px_to_mm(self.snap_radius_px),
        };
        snap(self.doc(), p, &settings, exclude)
    }

    pub(crate) fn execute(&mut self, cmd: Command, now_ms: i64) -> bool {
        self.editor.execute(cmd, now_ms).is_ok()
    }

    /// Where new objects go: the first visible, unlocked layer.
    pub(crate) fn active_layer(&self) -> LayerId {
        let layers = &self.doc().layers;
        layers
            .iter()
            .find(|l| l.visible && !l.locked)
            .unwrap_or(&layers[0])
            .id
    }

    pub(crate) fn drag_threshold_mm(&self) -> f64 {
        self.viewport.px_to_mm(DRAG_THRESHOLD_PX)
    }

    fn handle_select(&mut self, input: Input, now_ms: i64) {
        match input {
            Input::Down(p, m) => {
                let tol = self.viewport.px_to_mm(self.hit_radius_px);
                match hit_test(self.doc(), p, tol) {
                    Some(id) if m.shift => {
                        if let Some(i) = self.selection.iter().position(|s| *s == id) {
                            self.selection.remove(i);
                            self.gesture = Gesture::Idle;
                        } else {
                            self.selection.push(id);
                            self.gesture = Gesture::PressedOnObject { start: p };
                        }
                    }
                    Some(id) => {
                        if !self.selection.contains(&id) {
                            self.selection = vec![id];
                        }
                        self.gesture = Gesture::PressedOnObject { start: p };
                    }
                    None => {
                        if !m.shift {
                            self.selection.clear();
                        }
                        self.gesture = Gesture::PressedOnEmpty {
                            start: p,
                            additive: m.shift,
                        };
                    }
                }
            }
            Input::Move(p, m) => self.select_move(p, m),
            Input::Up(p, m) => {
                self.select_move(p, m);
                match std::mem::replace(&mut self.gesture, Gesture::Idle) {
                    Gesture::Moving { delta, .. } if delta.hypot() > 0.0 => {
                        let commands: Vec<_> = self
                            .unlocked_selection()
                            .into_iter()
                            .map(|o| Command::Replace {
                                object: Object {
                                    kind: translate_kind(&o.kind, delta),
                                    ..o.clone()
                                },
                            })
                            .collect();
                        if !commands.is_empty() {
                            self.execute(Command::Batch { commands }, now_ms);
                        }
                    }
                    Gesture::Marquee {
                        start,
                        current,
                        additive,
                    } => {
                        let rect = Rect::from_points(start, current);
                        let doc = self.doc();
                        let inside: Vec<ObjectId> = doc
                            .objects
                            .iter()
                            .filter(|o| {
                                !o.locked
                                    && doc.layer(o.layer).is_some_and(|l| l.visible && !l.locked)
                            })
                            .filter(|o| rect.contains_rect(object_bounds(&o.kind)))
                            .map(|o| o.id)
                            .collect();
                        if !additive {
                            self.selection.clear();
                        }
                        for id in inside {
                            if !self.selection.contains(&id) {
                                self.selection.push(id);
                            }
                        }
                    }
                    _ => {}
                }
                self.preview = Preview::default();
            }
            Input::DoubleClick(..) => {}
            Input::Key(Key::Escape, _) => {
                if self.gesture == Gesture::Idle {
                    self.selection.clear();
                }
                self.cancel_gesture();
            }
            Input::Key(Key::Delete | Key::Backspace, _) => {
                self.cancel_gesture();
                self.delete_selection(now_ms);
            }
            Input::Key(Key::Enter, _) => {}
        }
    }

    fn select_move(&mut self, p: Point, m: Modifiers) {
        let threshold = self.drag_threshold_mm();
        self.gesture = match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::PressedOnObject { start } if start.distance(p) > threshold => {
                Gesture::Moving {
                    start,
                    delta: Vec2::ZERO,
                }
            }
            Gesture::PressedOnEmpty { start, additive } if start.distance(p) > threshold => {
                Gesture::Marquee {
                    start,
                    current: p,
                    additive,
                }
            }
            Gesture::Marquee {
                start, additive, ..
            } => Gesture::Marquee {
                start,
                current: p,
                additive,
            },
            other => other,
        };
        match self.gesture.clone() {
            Gesture::Moving { start, .. } => {
                let snapped = self.snap_point(p, m, &self.selection);
                let delta = snapped.point - start;
                self.preview = Preview {
                    shapes: self
                        .unlocked_selection()
                        .iter()
                        .map(|o| outline(&translate_kind(&o.kind, delta)))
                        .collect(),
                    snap: Some(snapped).filter(|s| s.kind != SnapKind::None),
                    ..Preview::default()
                };
                self.gesture = Gesture::Moving { start, delta };
            }
            Gesture::Marquee { start, current, .. } => {
                self.preview = Preview {
                    marquee: Some(Rect::from_points(start, current)),
                    ..Preview::default()
                };
            }
            _ => {}
        }
    }

    /// Drawing tools; implemented in `draw.rs`.
    fn handle_draw(&mut self, input: Input, now_ms: i64) {
        crate::draw::handle(self, input, now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::builtin_plants;
    use crate::test_util::{NO, SHIFT, click, plant_at, state_with};
    use garden_core::model::ObjectKind;

    fn position(s: &EditorState, i: usize) -> Point {
        match s.doc().objects[i].kind {
            ObjectKind::Plant { position, .. } => position,
            _ => panic!("not a plant"),
        }
    }

    #[test]
    fn click_selects_shift_adds_empty_clears() {
        let mut s = state_with(vec![
            plant_at(1000., 1000., 600.),
            plant_at(5000., 1000., 600.),
        ]);
        let (a, b) = (s.doc().objects[0].id, s.doc().objects[1].id);
        click(&mut s, Point::new(1000., 1000.), NO);
        assert_eq!(s.selection, vec![a]);
        click(&mut s, Point::new(5000., 1000.), SHIFT);
        assert_eq!(s.selection, vec![a, b]);
        click(&mut s, Point::new(9000., 9000.), NO);
        assert!(s.selection.is_empty());
    }

    #[test]
    fn drag_moves_selection_as_one_undo_step() {
        let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
        s.handle(Input::Down(Point::new(1000., 1000.), NO), 1);
        s.handle(Input::Move(Point::new(1500., 1000.), NO), 2);
        assert!(!s.preview().shapes.is_empty());
        s.handle(Input::Up(Point::new(1500., 1000.), NO), 3);
        assert_eq!(position(&s, 0), Point::new(1500., 1000.));
        assert!(s.undo(4));
        assert_eq!(position(&s, 0), Point::new(1000., 1000.));
        assert!(!s.can_undo());
    }

    #[test]
    fn move_skips_locked_objects() {
        let mut s = state_with(vec![
            plant_at(1000., 1000., 600.),
            plant_at(1300., 1000., 600.),
        ]);
        let mut locked = s.doc().objects[1].clone();
        locked.locked = true;
        s.replace_object(locked, 1).unwrap();
        s.selection = s.doc().objects.iter().map(|o| o.id).collect();
        s.handle(Input::Down(Point::new(1000., 1000.), NO), 2);
        s.handle(Input::Move(Point::new(1000., 2000.), NO), 2);
        s.handle(Input::Up(Point::new(1000., 2000.), NO), 2);
        assert_eq!(position(&s, 1), Point::new(1300., 1000.));
        assert_eq!(position(&s, 0), Point::new(1000., 2000.));
        assert!(s.undo(3));
        assert!(s.undo(3));
        assert!(!s.can_undo()); // move = 1 step, lock = 1 step
    }

    #[test]
    fn marquee_selects_objects_fully_inside() {
        let mut s = state_with(vec![
            plant_at(1000., 1000., 600.),
            plant_at(5000., 5000., 600.),
        ]);
        s.handle(Input::Down(Point::new(0., 0.), NO), 1);
        s.handle(Input::Move(Point::new(2000., 2000.), NO), 1);
        assert!(s.preview().marquee.is_some());
        s.handle(Input::Up(Point::new(2000., 2000.), NO), 1);
        assert_eq!(s.selection, vec![s.doc().objects[0].id]);
    }

    #[test]
    fn delete_duplicate_undo() {
        let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
        let id = s.doc().objects[0].id;
        s.selection = vec![id];
        s.duplicate_selection(1);
        assert_eq!(s.doc().objects.len(), 2);
        assert_ne!(s.selection, vec![id]);
        assert_eq!(position(&s, 1), Point::new(1500., 1500.));
        s.delete_selection(2);
        assert_eq!(s.doc().objects.len(), 1);
        s.handle(Input::Key(Key::Delete, NO), 3);
        assert_eq!(s.doc().objects.len(), 1); // nothing selected now
        s.selection = vec![id];
        s.handle(Input::Key(Key::Backspace, NO), 4);
        assert!(s.doc().objects.is_empty());
        assert!(s.undo(5));
        assert_eq!(s.doc().objects.len(), 1);
    }

    #[test]
    fn alt_disables_snapping() {
        let s = state_with(vec![]);
        let alt = Modifiers { alt: true, ..NO };
        assert_eq!(
            s.snap_point(Point::new(1530., 1010.), alt, &[]).point,
            Point::new(1530., 1010.)
        );
        assert_eq!(
            s.snap_point(Point::new(1530., 1010.), NO, &[]).point,
            Point::new(1500., 1000.)
        );
    }

    #[test]
    #[ignore = "needs the plant tool (task 7)"]
    fn undo_drops_stale_selection() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Plant(builtin_plants()[7].clone()));
        click(&mut s, Point::new(1000., 1000.), NO);
        s.set_tool(ToolKind::Select);
        s.selection = vec![s.doc().objects[0].id];
        s.undo(2);
        assert!(s.selection.is_empty());
    }
}
