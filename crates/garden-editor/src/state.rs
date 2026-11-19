//! The editor as the UI sees it: document + history, view, selection and the active tool.

use garden_core::command::{Command, CommandError};
use garden_core::editor::Editor;
use garden_core::geometry::Point;
use garden_core::model::{BackgroundImage, CalibrationError, Document, LayerId, Object, ObjectId};
use garden_core::query::{SnapKind, SnapResult, SnapSettings, hit_test_selectable, snap};
use garden_core::units::{UnitSystem, parse_length};
use kurbo::{Rect, Vec2};

use crate::objects::{object_bounds, object_vertices, translate_kind};
use crate::render::outline;
use crate::tools::{Input, Key, Modifiers, Preview, ToolKind};
use crate::viewport::Viewport;

/// Pointer travel (px) below which a press counts as a click.
const DRAG_THRESHOLD_PX: f64 = 3.0;
/// Offset of duplicated objects.
const DUPLICATE_OFFSET: Vec2 = Vec2::new(500.0, 500.0);

/// Why a typed scale distance couldn't be applied.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ScaleError {
    NoBackground,
    NotALength,
    MustBePositive,
    SamePoint,
}

impl ScaleError {
    pub fn message(&self, s: &crate::strings::Strings) -> &'static str {
        match self {
            ScaleError::NoBackground => s.hint_scale_need_photo,
            ScaleError::NotALength => s.err_not_a_length,
            ScaleError::MustBePositive => s.err_must_be_positive,
            ScaleError::SamePoint => s.scale_same_point,
        }
    }
}

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
    /// `anchor` is the selected corner nearest the press; it is what snaps.
    Moving {
        start: Point,
        anchor: Point,
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
        /// Unsnapped press position, to tell a click from a drag.
        raw_start: Point,
        current: Point,
        shift: bool,
    },
    /// Polygon / polyline tools: placed points and where the last press began.
    Poly {
        points: Vec<Point>,
        pressed: Option<Point>,
    },
    /// Set-scale tool: up to two points clicked on the traced photo (world mm).
    Scale {
        points: Vec<Point>,
    },
    /// Dragging a handle of the selected object. `at` is where the handle was,
    /// `press` where the pointer went down; `result` is the reshaped object so far.
    Handle {
        kind: crate::handles::HandleKind,
        at: Point,
        press: Point,
        moved: bool,
        result: Option<garden_core::model::ObjectKind>,
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
    /// Point editing of one object, if active.
    pub(crate) node: Option<crate::handles::NodeEdit>,
    /// The layer the person chose for new objects.
    pub(crate) active: Option<LayerId>,
    /// Successful document changes so far (for autosave).
    changes: u64,
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
            node: None,
            active: None,
            changes: 0,
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
        self.node = None;
        self.cancel_gesture();
    }

    /// Switch tools the way a person expects: a line or shape being drawn is
    /// finished first (if it has enough points), not thrown away.
    pub fn switch_tool(&mut self, tool: ToolKind, now_ms: i64) {
        if self.points_placed() > 0 {
            self.handle(Input::Key(Key::Enter, Modifiers::default()), now_ms);
        }
        self.set_tool(tool);
    }

    /// How many points the polygon/line being drawn has (0 when not drawing one).
    pub fn points_placed(&self) -> usize {
        match &self.gesture {
            Gesture::Poly { points, .. } | Gesture::Scale { points } => points.len(),
            _ => 0,
        }
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

    /// Undo. While drawing a polygon or line this removes the last placed point instead.
    /// Increases by one on every successful document change. Never decreases.
    pub fn change_count(&self) -> u64 {
        self.changes
    }

    /// True while a pointer gesture or a half-drawn polygon/line is in progress.
    pub fn is_busy(&self) -> bool {
        self.gesture != Gesture::Idle
    }

    /// Replace the document (opening another design). Clears history, selection
    /// and any gesture, keeps the viewport size and fits to the new content.
    /// Leaves `change_count` unchanged.
    pub fn open_document(&mut self, doc: Document) {
        self.editor = Editor::new(doc);
        self.selection.clear();
        self.node = None;
        self.active = None;
        self.cancel_gesture();
        self.fit_to_content();
    }

    pub fn undo(&mut self, now_ms: i64) -> bool {
        if let Gesture::Poly { points, .. } = &mut self.gesture
            && points.pop().is_some()
        {
            self.preview.shapes.clear();
            self.preview.dims.clear();
            return true;
        }
        let done = self.editor.undo(now_ms);
        if done {
            self.changes += 1;
        }
        self.after_history_change();
        done
    }

    pub fn redo(&mut self, now_ms: i64) -> bool {
        let done = self.editor.redo(now_ms);
        if done {
            self.changes += 1;
        }
        self.after_history_change();
        done
    }

    fn after_history_change(&mut self) {
        let doc = self.editor.doc();
        self.selection.retain(|id| doc.object(*id).is_some());
        if let Some(n) = &mut self.node {
            n.vertex = None;
        }
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
        self.try_execute(Command::Replace { object }, now_ms)
    }

    /// Add, change or remove the traced photo (undoable).
    pub fn set_background(&mut self, bg: Option<BackgroundImage>, now_ms: i64) {
        if self.doc().background != bg {
            self.execute(Command::SetBackground { background: bg }, now_ms);
        }
    }

    /// After two clicks with the set-scale tool: the two points in photo pixels
    /// and their current distance on the plan (mm).
    pub fn pending_scale(&self) -> Option<(Point, Point, f64)> {
        let bg = self.doc().background.as_ref()?;
        match &self.gesture {
            Gesture::Scale { points } if points.len() == 2 => Some((
                bg.world_to_px(points[0]),
                bg.world_to_px(points[1]),
                points[0].distance(points[1]),
            )),
            _ => None,
        }
    }

    /// Rescale the photo so the two clicked points are `input` apart (e.g. "6 m").
    pub fn apply_scale(&mut self, input: &str, now_ms: i64) -> Result<(), ScaleError> {
        let Some(mut bg) = self.doc().background.clone() else {
            return Err(ScaleError::NoBackground);
        };
        let Some((a, b, _)) = self.pending_scale() else {
            return Err(ScaleError::SamePoint);
        };
        let real = parse_length(input, self.doc().units).map_err(|_| ScaleError::NotALength)?;
        if real <= 0.0 {
            return Err(ScaleError::MustBePositive);
        }
        bg.calibrate_about(a, b, real).map_err(|e| match e {
            CalibrationError::SamePoint => ScaleError::SamePoint,
            CalibrationError::InvalidDistance => ScaleError::MustBePositive,
        })?;
        self.set_background(Some(bg), now_ms);
        self.set_tool(ToolKind::Select);
        Ok(())
    }

    /// Rename the design (undoable). Blank names are ignored.
    pub fn rename(&mut self, name: &str, now_ms: i64) {
        let name = name.trim();
        if !name.is_empty() && name != self.doc().name {
            self.execute(
                Command::Rename {
                    name: name.to_string(),
                },
                now_ms,
            );
        }
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
        self.try_execute(cmd, now_ms).is_ok()
    }

    /// The single path every document change goes through (keeps `changes` honest).
    pub(crate) fn try_execute(&mut self, cmd: Command, now_ms: i64) -> Result<(), CommandError> {
        self.editor.execute(cmd, now_ms)?;
        self.changes += 1;
        Ok(())
    }

    pub(crate) fn drag_threshold_mm(&self) -> f64 {
        self.viewport.px_to_mm(DRAG_THRESHOLD_PX)
    }

    fn handle_select(&mut self, input: Input, now_ms: i64) {
        if matches!(self.gesture, Gesture::Handle { .. }) {
            match input {
                Input::Move(p, m) => return self.drag_handle(p, m),
                Input::Up(p, m) => {
                    self.drag_handle(p, m);
                    return self.finish_handle(now_ms);
                }
                _ => {}
            }
        }
        match input {
            Input::Down(p, m) => {
                if let Some(h) = self.handle_at(p) {
                    self.gesture = Gesture::Handle {
                        kind: h.kind,
                        at: h.at,
                        press: p,
                        moved: false,
                        result: None,
                    };
                    return;
                }
                let tol = self.viewport.px_to_mm(self.hit_radius_px);
                match hit_test_selectable(self.doc(), p, tol) {
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
                        self.node = None;
                        self.gesture = Gesture::PressedOnEmpty {
                            start: p,
                            additive: m.shift,
                        };
                    }
                }
            }
            Input::Move(p, m) => self.select_move(p, m),
            Input::Up(p, m) => {
                if self.node.is_some_and(|n| self.selection != [n.id]) {
                    self.node = None;
                }
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
            Input::DoubleClick(p, _) => self.node_double_click(p, now_ms),
            Input::Key(Key::Escape, _) => {
                if self.gesture == Gesture::Idle {
                    if self.node_mode() {
                        self.node = None;
                    } else {
                        self.selection.clear();
                    }
                }
                self.cancel_gesture();
            }
            Input::Key(Key::Delete | Key::Backspace, _) => {
                self.cancel_gesture();
                if self.node_mode() {
                    self.delete_selected_vertex(now_ms);
                } else {
                    self.delete_selection(now_ms);
                }
            }
            Input::Key(Key::Enter, _) => {}
        }
    }

    fn select_move(&mut self, p: Point, m: Modifiers) {
        let threshold = self.drag_threshold_mm();
        self.gesture = match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::PressedOnObject { start } if start.distance(p) > threshold => {
                let anchor = self
                    .unlocked_selection()
                    .iter()
                    .flat_map(|o| object_vertices(&o.kind))
                    .min_by(|a, b| a.distance(start).total_cmp(&b.distance(start)))
                    .unwrap_or(start);
                Gesture::Moving {
                    start,
                    anchor,
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
            Gesture::Moving { start, anchor, .. } => {
                // Snap where the grabbed corner would land, not the cursor.
                let snapped = self.snap_point(anchor + (p - start), m, &self.selection);
                let delta = snapped.point - anchor;
                self.preview = Preview {
                    shapes: self
                        .unlocked_selection()
                        .iter()
                        .map(|o| outline(&translate_kind(&o.kind, delta)))
                        .collect(),
                    snap: Some(snapped).filter(|s| s.kind != SnapKind::None),
                    ..Preview::default()
                };
                self.gesture = Gesture::Moving {
                    start,
                    anchor,
                    delta,
                };
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
    use crate::layers::ZOrder;
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
    fn undo_drops_stale_selection() {
        let mut s = state_with(vec![]);
        s.set_tool(ToolKind::Plant(builtin_plants()[7].clone()));
        click(&mut s, Point::new(1000., 1000.), NO);
        s.set_tool(ToolKind::Select);
        s.selection = vec![s.doc().objects[0].id];
        s.undo(2);
        assert!(s.selection.is_empty());
    }

    #[test]
    fn locked_object_can_be_selected_but_not_moved() {
        let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
        let mut o = s.doc().objects[0].clone();
        o.locked = true;
        s.replace_object(o, 1).unwrap();
        click(&mut s, Point::new(1000., 1000.), NO);
        assert_eq!(s.selection, vec![s.doc().objects[0].id]); // so it can be unlocked
        s.handle(Input::Down(Point::new(1000., 1000.), NO), 2);
        s.handle(Input::Move(Point::new(3000., 1000.), NO), 2);
        s.handle(Input::Up(Point::new(3000., 1000.), NO), 2);
        assert_eq!(position(&s, 0), Point::new(1000., 1000.));
    }

    #[test]
    fn move_snaps_the_object_not_the_cursor() {
        let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
        // grab the plant off-centre, drag ~half a metre right
        s.handle(Input::Down(Point::new(1030., 1010.), NO), 1);
        s.handle(Input::Move(Point::new(1530., 1010.), NO), 1);
        s.handle(Input::Up(Point::new(1530., 1010.), NO), 1);
        assert_eq!(position(&s, 0), Point::new(1500., 1000.));
    }

    #[test]
    fn change_count_tracks_every_document_change() {
        let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
        let c0 = s.change_count();
        s.selection = vec![s.doc().objects[0].id];
        s.duplicate_selection(1);
        assert_eq!(s.change_count(), c0 + 1);
        let mut o = s.doc().objects[0].clone();
        o.name = "x".into();
        s.replace_object(o, 2).unwrap();
        assert_eq!(s.change_count(), c0 + 2);
        s.undo(3);
        assert_eq!(s.change_count(), c0 + 3);
        s.redo(4);
        assert_eq!(s.change_count(), c0 + 4);
        s.set_units(garden_core::units::UnitSystem::Imperial, 5);
        assert_eq!(s.change_count(), c0 + 5);
        s.set_units(garden_core::units::UnitSystem::Imperial, 6);
        assert_eq!(s.change_count(), c0 + 5); // no-op is not a change
        click(&mut s, Point::new(9000., 9000.), NO);
        assert_eq!(s.change_count(), c0 + 5); // selection only
    }

    #[test]
    fn busy_while_dragging_or_drawing() {
        let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
        assert!(!s.is_busy());
        s.handle(Input::Down(Point::new(1000., 1000.), NO), 1);
        assert!(s.is_busy());
        s.handle(Input::Up(Point::new(1000., 1000.), NO), 1);
        assert!(!s.is_busy());
        s.set_tool(ToolKind::Polyline(crate::tools::LineTarget::Path {
            width: 900.,
        }));
        click(&mut s, Point::new(0., 0.), NO);
        assert!(s.is_busy());
    }

    #[test]
    fn open_document_resets_history_and_selection() {
        let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
        s.selection = vec![s.doc().objects[0].id];
        s.duplicate_selection(1);
        let other = Document::new("Other", 5);
        s.open_document(other.clone());
        assert_eq!(s.doc().id, other.id);
        assert!(s.selection.is_empty());
        assert!(!s.can_undo());
        assert!(!s.is_busy());
    }

    #[test]
    fn rename_is_undoable() {
        let mut s = state_with(vec![]);
        let c0 = s.change_count();
        s.rename("Back garden", 1);
        assert_eq!(s.doc().name, "Back garden");
        assert_eq!(s.change_count(), c0 + 1);
        s.rename("   ", 2); // blank names are ignored
        assert_eq!(s.doc().name, "Back garden");
        assert!(s.undo(3));
        assert_eq!(s.doc().name, "g");
    }

    #[test]
    fn new_layers_become_active_and_receive_new_objects() {
        let mut s = state_with(vec![]);
        let first = s.active_layer_id();
        let l2 = s.add_layer(1);
        assert_eq!(s.active_layer_id(), l2);
        assert_eq!(s.doc().layers[1].name, "Layer 2");
        s.set_tool(ToolKind::Plant(builtin_plants()[7].clone()));
        click(&mut s, Point::new(0., 0.), NO);
        assert_eq!(s.doc().objects[0].layer, l2);
        s.set_active_layer(first);
        assert_eq!(s.active_layer_id(), first);
        s.set_active_layer(garden_core::model::LayerId::new()); // unknown: ignored
        assert_eq!(s.active_layer_id(), first);
    }

    #[test]
    fn hidden_or_locked_active_layer_falls_back() {
        let mut s = state_with(vec![]);
        let first = s.active_layer_id();
        let l2 = s.add_layer(1);
        let mut layer = s.doc().layers[1].clone();
        layer.locked = true;
        s.update_layer(layer, 2);
        assert!(s.doc().layers[1].locked);
        assert_eq!(s.active_layer_id(), first);
        assert!(s.undo(3));
        assert_eq!(s.active_layer_id(), l2);
    }

    #[test]
    fn layers_with_objects_cannot_be_deleted() {
        let mut s = state_with(vec![plant_at(0., 0., 600.)]);
        let first = s.doc().layers[0].id;
        assert_eq!(s.delete_layer(first, 1), Err(CommandError::LayerInUse));
        let l2 = s.add_layer(2);
        s.selection = vec![s.doc().objects[0].id];
        s.move_selection_to_layer(l2, 3);
        assert_eq!(s.doc().objects[0].layer, l2);
        s.set_active_layer(first);
        s.delete_layer(first, 4).unwrap();
        assert_eq!(s.active_layer_id(), l2);
        assert_eq!(s.delete_layer(l2, 5), Err(CommandError::LayerInUse));
        assert_eq!(s.doc().layers.len(), 1);
    }

    #[test]
    fn layer_names_stay_unique() {
        let mut s = state_with(vec![]);
        let l2 = s.add_layer(1);
        s.add_layer(2);
        s.delete_layer(l2, 3).unwrap();
        s.add_layer(4);
        let names: Vec<_> = s.doc().layers.iter().map(|l| l.name.clone()).collect();
        assert_eq!(names[1..], ["Layer 3".to_string(), "Layer 2".to_string()]);
    }

    #[test]
    fn reorder_keeps_relative_order_in_one_step() {
        let mut s = state_with(vec![
            plant_at(0., 0., 1.),
            plant_at(10., 0., 1.),
            plant_at(20., 0., 1.),
            plant_at(30., 0., 1.),
        ]);
        let ids: Vec<_> = s.doc().objects.iter().map(|o| o.id).collect();
        let order = |s: &EditorState| s.doc().objects.iter().map(|o| o.id).collect::<Vec<_>>();
        s.selection = vec![ids[0], ids[1]];
        s.reorder_selection(ZOrder::Front, 1);
        assert_eq!(order(&s), vec![ids[2], ids[3], ids[0], ids[1]]);
        s.selection = vec![ids[1]];
        s.reorder_selection(ZOrder::Backward, 2);
        assert_eq!(s.doc().objects[2].id, ids[1]);
        s.selection = vec![ids[2], ids[0]];
        s.reorder_selection(ZOrder::Back, 3);
        assert_eq!(order(&s)[..2], [ids[2], ids[0]]);
        s.reorder_selection(ZOrder::Back, 4); // already there: no change, no undo step
        assert!(s.undo(5));
        assert!(s.undo(5));
        assert!(s.undo(5));
        assert_eq!(order(&s), ids);
        assert!(!s.can_undo());
        s.selection = vec![ids[2]];
        s.reorder_selection(ZOrder::Forward, 6);
        assert_eq!(order(&s), vec![ids[0], ids[1], ids[3], ids[2]]);
    }
}
