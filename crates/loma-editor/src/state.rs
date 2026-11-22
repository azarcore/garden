//! The editor as the UI sees it: document + history, view, selection and the active tool.

use kurbo::{Rect, Vec2};
use loma_core::command::{Command, CommandError};
use loma_core::editor::Editor;
use loma_core::geometry::Point;
use loma_core::model::{BackgroundImage, CalibrationError, Document, LayerId, Object, ObjectId};
use loma_core::query::{SnapKind, SnapResult, SnapSettings, hit_test_selectable, snap};
use loma_core::units::{UnitSystem, parse_length};

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
    /// Dimension and measure tools: the points clicked so far (at most two).
    TwoPoints {
        points: Vec<Point>,
    },
    /// Dragging a handle of the selected object. `at` is where the handle was,
    /// `press` where the pointer went down; `result` is the reshaped object so far.
    Handle {
        kind: crate::handles::HandleKind,
        at: Point,
        press: Point,
        moved: bool,
        result: Option<loma_core::model::ObjectKind>,
    },
}

/// Design changes the layout; Garden looks after the plants and never changes the layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Design,
    Garden,
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
    pub(crate) mode: Mode,
}

impl EditorState {
    pub fn new(mut doc: Document, width_px: f64, height_px: f64) -> EditorState {
        doc.repair();
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
            mode: Mode::Design,
        };
        s.fit_to_content();
        s
    }

    pub fn doc(&self) -> &Document {
        self.editor.doc()
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Switch between Design and Garden. Abandons any gesture or point editing.
    /// Entering Garden keeps only plants and hedges selected.
    pub fn set_mode(&mut self, mode: Mode) {
        self.tool = ToolKind::Select;
        self.node = None;
        self.cancel_gesture();
        self.mode = mode;
        if mode == Mode::Garden {
            let doc = self.editor.doc();
            self.selection
                .retain(|id| doc.object(*id).is_some_and(crate::care::is_gardenable));
        }
    }

    /// Garden mode: a tap at `p` selects the plant or hedge there (topmost on a
    /// visible layer, locks ignored). `additive` toggles it in the selection
    /// instead. A tap on nothing else clears the selection unless additive.
    pub fn garden_tap(&mut self, p: Point, additive: bool) {
        if self.mode != Mode::Garden {
            return;
        }
        let tolerance = self.viewport.px_to_mm(self.hit_radius_px);
        let hit = loma_core::query::hit_test_where(
            self.editor.doc(),
            p,
            tolerance,
            crate::care::is_gardenable,
        );
        match (hit, additive) {
            (Some(id), true) => {
                if let Some(i) = self.selection.iter().position(|s| *s == id) {
                    self.selection.remove(i);
                } else {
                    self.selection.push(id);
                }
            }
            (Some(id), false) => self.selection = vec![id],
            (None, true) => {}
            (None, false) => self.selection.clear(),
        }
    }

    pub fn tool(&self) -> &ToolKind {
        &self.tool
    }

    /// Switch tools, abandoning any gesture in progress.
    pub fn set_tool(&mut self, tool: ToolKind) {
        if self.mode == Mode::Garden {
            return;
        }
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
            Gesture::Poly { points, .. }
            | Gesture::Scale { points }
            | Gesture::TwoPoints { points } => points.len(),
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
        if self.mode == Mode::Garden {
            return;
        }
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
        match &self.gesture {
            Gesture::Idle => false,
            // a finished measurement just stays on screen
            Gesture::TwoPoints { points } => points.len() < 2,
            _ => true,
        }
    }

    /// Replace the document (opening another design). Clears history, selection
    /// and any gesture, keeps the viewport size and fits to the new content.
    /// Leaves `change_count` unchanged. A document without layers gets one.
    pub fn open_document(&mut self, mut doc: Document) {
        doc.repair();
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
        // half-placed dimension or measurement: undo drops that point
        if let Gesture::TwoPoints { points } = &self.gesture
            && points.len() == 1
        {
            self.cancel_gesture();
            return true;
        }
        if self.mode == Mode::Garden
            && !self.editor.next_undo().is_some_and(Command::is_layout_free)
        {
            return false;
        }
        let done = self.editor.undo(now_ms);
        if done {
            self.changes += 1;
        }
        self.after_history_change();
        done
    }

    pub fn redo(&mut self, now_ms: i64) -> bool {
        if self.mode == Mode::Garden
            && !self.editor.next_redo().is_some_and(Command::is_layout_free)
        {
            return false;
        }
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
        match self.mode {
            Mode::Design => self.editor.can_undo(),
            Mode::Garden => self.editor.next_undo().is_some_and(Command::is_layout_free),
        }
    }

    pub fn can_redo(&self) -> bool {
        match self.mode {
            Mode::Design => self.editor.can_redo(),
            Mode::Garden => self.editor.next_redo().is_some_and(Command::is_layout_free),
        }
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

    /// Set or clear a garden list price (per metric unit, undoable).
    /// Negative or non-finite prices are ignored.
    pub fn set_price(&mut self, key: &str, price: Option<f64>, now_ms: i64) {
        if price.is_some_and(|p| !p.is_finite() || p < 0.0)
            || self.doc().prices.get(key).copied() == price
        {
            return;
        }
        self.execute(
            Command::SetPrice {
                key: key.to_string(),
                price,
            },
            now_ms,
        );
    }

    /// Point true north at `degrees` clockwise from plan-up (undoable).
    /// Returns false when nothing changed: a bad or unchanged value, or Garden mode.
    pub fn set_north(&mut self, degrees: f64, now_ms: i64) -> bool {
        if !degrees.is_finite() {
            return false;
        }
        let degrees = crate::compass::normalize(degrees);
        if degrees == self.doc().north {
            return false;
        }
        self.execute(Command::SetNorth { degrees }, now_ms)
    }

    /// Set where the garden is (undoable); `None` forgets it. Returns false
    /// when nothing changed: a bad or unchanged place, or Garden mode.
    pub fn set_location(
        &mut self,
        location: Option<loma_core::model::GeoPoint>,
        now_ms: i64,
    ) -> bool {
        let location = match location {
            Some(p) => match loma_core::model::GeoPoint::new(p.lat, p.lon) {
                Some(p) => Some(p),
                None => return false,
            },
            None => None,
        };
        if location == self.doc().location {
            return false;
        }
        self.execute(Command::SetLocation { location }, now_ms)
    }

    /// Gravel depth for the garden list (mm, undoable); bad values are ignored.
    pub fn set_gravel_depth(&mut self, depth_mm: f64, now_ms: i64) {
        if depth_mm.is_finite() && depth_mm > 0.0 && self.doc().gravel_depth != Some(depth_mm) {
            self.execute(
                Command::SetGravelDepth {
                    depth: Some(depth_mm),
                },
                now_ms,
            );
        }
    }

    /// What the garden list's prices are in (undoable).
    pub fn set_currency(&mut self, currency: &str, now_ms: i64) {
        let currency = currency.trim();
        if currency != self.doc().currency {
            self.execute(
                Command::SetCurrency {
                    currency: currency.to_string(),
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

    /// Whether smart guides apply: they follow "snap to objects", and Alt turns them off.
    pub(crate) fn guides_on(&self, m: Modifiers) -> bool {
        self.snap_objects && !m.alt
    }

    /// Bounds of every object on a visible layer except `exclude`, and the plot's bounds.
    pub(crate) fn guide_context(&self, exclude: &[ObjectId]) -> (Vec<Rect>, Option<Rect>) {
        let doc = self.doc();
        let mut plot = None;
        let others = doc
            .objects
            .iter()
            .filter(|o| !exclude.contains(&o.id) && doc.layer(o.layer).is_some_and(|l| l.visible))
            .map(|o| {
                let b = object_bounds(&o.kind);
                if matches!(o.kind, loma_core::model::ObjectKind::PlotBoundary { .. }) {
                    plot = Some(b);
                }
                b
            })
            .collect();
        (others, plot)
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
        if self.mode == Mode::Garden && !cmd.is_layout_free() {
            return Err(CommandError::LayoutLocked);
        }
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
                let raw = anchor + (p - start);
                let snapped = self.snap_point(raw, m, &self.selection);
                let mut delta = snapped.point - anchor;
                let mut guides = Vec::new();
                let moving = self
                    .unlocked_selection()
                    .iter()
                    .map(|o| object_bounds(&o.kind))
                    .reduce(|a, b| a.union(b));
                if let Some(moving) = moving.filter(|_| self.guides_on(m)) {
                    let (others, plot) = self.guide_context(&self.selection);
                    // a snap to an object's corner or edge wins over alignment
                    if !matches!(snapped.kind, SnapKind::Vertex | SnapKind::Edge) {
                        let raw_delta = raw - anchor;
                        let tol = self.viewport.px_to_mm(self.snap_radius_px);
                        let g = crate::guides::move_guides(moving + raw_delta, &others, tol);
                        // alignment and equal spacing win over the grid on their axis
                        if g.aligned_x || g.spaced_x {
                            delta.x = raw_delta.x + g.shift.x;
                        }
                        if g.aligned_y || g.spaced_y {
                            delta.y = raw_delta.y + g.shift.y;
                        }
                        // draw the lines where the object actually lands
                        guides = crate::guides::exact_lines(moving + delta, &others, tol);
                    }
                    guides.extend(crate::guides::distance_guides(
                        moving + delta,
                        &others,
                        plot,
                        self.doc().units,
                    ));
                }
                self.preview = Preview {
                    guides,
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
    use loma_core::model::ObjectKind;

    fn position(s: &EditorState, i: usize) -> Point {
        match s.doc().objects[i].kind {
            ObjectKind::Plant { position, .. } => position,
            _ => panic!("not a plant"),
        }
    }

    #[test]
    fn set_location_undoes_and_is_locked_in_garden() {
        use loma_core::model::GeoPoint;
        let mut s = state_with(vec![]);
        let here = GeoPoint::new(51.4779, -0.0015);
        assert!(s.set_location(here, 1));
        assert!(
            !s.set_location(
                Some(GeoPoint {
                    lat: 51.481,
                    lon: 0.001
                }),
                2
            ),
            "same once rounded"
        );
        s.undo(3);
        assert_eq!(s.doc().location, None);
        s.set_mode(Mode::Garden);
        assert!(!s.set_location(here, 4));
        assert_eq!(s.doc().location, None);
    }

    #[test]
    fn set_north_is_one_undo_step_and_locked_in_garden() {
        let mut s = state_with(vec![]);
        assert!(s.set_north(30.0, 1));
        assert!(
            !s.set_north(30.0, 2),
            "same value again is not another step"
        );
        assert!(!s.set_north(f64::NAN, 3));
        assert!(!s.set_north(-330.0, 4), "-330 is 30 again");
        assert_eq!(s.doc().north, 30.0);
        s.undo(5);
        assert_eq!(s.doc().north, 0.0);
        s.set_mode(Mode::Garden);
        assert!(!s.set_north(45.0, 6));
        assert_eq!(s.doc().north, 0.0);
    }

    #[test]
    fn prices_and_currency_undo() {
        let mut s = state_with(vec![]);
        s.set_price("area:lawn", Some(3.0), 1);
        s.set_currency(" € ", 2);
        assert_eq!(s.doc().prices.get("area:lawn"), Some(&3.0));
        assert_eq!(s.doc().currency, "€");
        // the same value again is not another undo step
        s.set_price("area:lawn", Some(3.0), 3);
        s.undo(4);
        assert_eq!(s.doc().currency, "");
        s.undo(5);
        assert!(s.doc().prices.is_empty());
        s.redo(6);
        assert_eq!(s.doc().prices.get("area:lawn"), Some(&3.0));
        // bad values are ignored
        s.set_price("area:lawn", Some(f64::NAN), 7);
        s.set_price("area:lawn", Some(-2.0), 8);
        assert_eq!(s.doc().prices.get("area:lawn"), Some(&3.0));
        s.set_price("area:lawn", None, 9);
        assert!(s.doc().prices.is_empty());
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
        s.set_units(loma_core::units::UnitSystem::Imperial, 5);
        assert_eq!(s.change_count(), c0 + 5);
        s.set_units(loma_core::units::UnitSystem::Imperial, 6);
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
        s.set_active_layer(loma_core::model::LayerId::new()); // unknown: ignored
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

    mod smart_guides {
        use super::*;
        use crate::guides::GuideKind;
        use crate::tools::DrawTarget;
        use loma_core::geometry::Shape;
        use loma_core::model::AreaType;

        fn bed_at(x: f64, y: f64, w: f64, h: f64) -> ObjectKind {
            ObjectKind::Area {
                area_type: AreaType::Bed,
                shape: Shape::rect(Point::new(x, y), w, h),
            }
        }

        #[test]
        fn moving_aligns_and_shows_distances() {
            let mut s = state_with(vec![
                bed_at(1000., 0., 2000., 1000.),
                plant_at(1530., 3000., 600.),
            ]);
            s.handle(Input::Down(Point::new(1530., 3000.), NO), 1);
            s.handle(Input::Move(Point::new(1300., 3020.), NO), 1);
            assert!(
                s.preview()
                    .guides
                    .iter()
                    .any(|g| g.kind == GuideKind::Align)
            );
            assert!(
                s.preview()
                    .guides
                    .iter()
                    .any(|g| g.kind == GuideKind::Distance)
            );
            s.handle(Input::Up(Point::new(1300., 3020.), NO), 2);
            assert_eq!(object_bounds(&s.doc().objects[1].kind).x0, 1000.);
            assert!(s.undo(3));
            assert!(!s.can_undo());
        }

        #[test]
        fn alignment_beats_the_grid() {
            // plant left edge would land at 1040 on the grid; the bed's left edge (1000) is within reach
            let mut s = state_with(vec![
                bed_at(1000., 0., 2000., 1000.),
                plant_at(1530., 3000., 600.),
            ]);
            s.handle(Input::Down(Point::new(1530., 3000.), NO), 1);
            s.handle(Input::Move(Point::new(1340., 3000.), NO), 1);
            s.handle(Input::Up(Point::new(1340., 3000.), NO), 2);
            assert_eq!(object_bounds(&s.doc().objects[1].kind).x0, 1000.);
        }

        #[test]
        fn alt_or_the_toggle_turns_guides_off() {
            let mut s = state_with(vec![
                bed_at(1000., 0., 2000., 1000.),
                plant_at(1530., 3000., 600.),
            ]);
            let alt = Modifiers { alt: true, ..NO };
            s.handle(Input::Down(Point::new(1530., 3000.), alt), 1);
            s.handle(Input::Move(Point::new(1340., 3000.), alt), 1);
            assert!(s.preview().guides.is_empty());
            s.handle(Input::Key(Key::Escape, NO), 1);
            s.snap_objects = false;
            s.handle(Input::Down(Point::new(1530., 3000.), NO), 1);
            s.handle(Input::Move(Point::new(1340., 3000.), NO), 1);
            assert!(s.preview().guides.is_empty());
        }

        #[test]
        fn a_vertex_snap_beats_alignment_and_stays_put() {
            let a = bed_at(0., 0., 2000., 1000.);
            let b = bed_at(5000., 3000., 1000., 1000.);
            // a third bed whose left edge (2080) would pull B off A's corner if alignment won
            let c = bed_at(2080., 6000., 500., 500.);
            let mut s = state_with(vec![a, b, c]);
            s.handle(Input::Down(Point::new(5000., 3000.), NO), 1);
            s.handle(Input::Move(Point::new(2030., 1020.), NO), 1);
            let first = s.preview().shapes.clone();
            s.handle(Input::Move(Point::new(2040., 1010.), NO), 1);
            assert_eq!(s.preview().shapes, first);
            s.handle(Input::Up(Point::new(2040., 1010.), NO), 2);
            let bb = object_bounds(&s.doc().objects[1].kind);
            assert_eq!((bb.x0, bb.y0), (2000., 1000.));
        }

        #[test]
        fn guide_lines_follow_where_the_object_lands() {
            // equal spacing on x; on y the grid moves the plant from 523 to 500
            let mut s = state_with(vec![
                bed_at(0., 0., 1000., 1400.),
                bed_at(3000., 0., 1000., 1400.),
                plant_at(5000., 3000., 600.),
            ]);
            s.snap_radius_px = 1.0; // 20 mm: only equal spacing (and the grid) apply
            s.handle(Input::Down(Point::new(5000., 3000.), NO), 1);
            s.handle(Input::Move(Point::new(2010., 523.), NO), 1);
            let equal: Vec<_> = s
                .preview()
                .guides
                .iter()
                .filter(|g| g.kind == GuideKind::Equal)
                .collect();
            assert_eq!(equal.len(), 2);
            assert!(
                equal.iter().all(|g| g.from.y == 500. && g.to.y == 500.),
                "{equal:?}"
            );
        }

        #[test]
        fn polygon_closes_on_the_first_point_even_when_alignment_pulls() {
            let mut s = state_with(vec![bed_at(200., 5000., 1000., 1000.)]); // left edge x = 200
            s.snap_grid = false;
            s.set_tool(ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)));
            for p in [
                Point::new(0., 0.),
                Point::new(3000., 0.),
                Point::new(3000., 3000.),
            ] {
                click(&mut s, p, NO);
            }
            click(&mut s, Point::new(40., 30.), NO); // on the first point
            assert_eq!(s.doc().objects.len(), 2);
            assert!(
                matches!(&s.doc().objects[1].kind, ObjectKind::Area { shape, .. } if shape.vertices().len() == 3)
            );
        }

        #[test]
        fn shift_drawing_drops_stale_guides() {
            let mut s = state_with(vec![bed_at(1000., 0., 2000., 1000.)]);
            s.snap_grid = false;
            s.set_tool(ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)));
            click(&mut s, Point::new(0., 3000.), NO);
            s.handle(Input::Move(Point::new(1050., 3500.), NO), 1);
            assert!(!s.preview().guides.is_empty());
            s.handle(Input::Move(Point::new(1050., 3500.), SHIFT), 1);
            assert!(s.preview().guides.is_empty());
        }

        #[test]
        fn drawing_aligns_the_cursor() {
            let mut s = state_with(vec![bed_at(1000., 0., 2000., 1000.)]);
            s.set_tool(ToolKind::Rect(DrawTarget::Area(AreaType::Bed)));
            s.snap_grid = false;
            s.handle(Input::Move(Point::new(1050., 3000.), NO), 1);
            assert!(!s.preview().guides.is_empty());
            s.handle(Input::Down(Point::new(1050., 3000.), NO), 1);
            s.handle(Input::Move(Point::new(2500., 4000.), NO), 1);
            s.handle(Input::Up(Point::new(2500., 4000.), NO), 1);
            assert_eq!(object_bounds(&s.doc().objects[1].kind).x0, 1000.);
        }
    }

    mod garden {
        use super::*;
        use crate::test_util::lawn_rect;
        use loma_core::geometry::Shape;
        use loma_core::model::{Health, LinearKind, PlantCare};

        fn hedge() -> ObjectKind {
            ObjectKind::Linear {
                kind: LinearKind::Hedge,
                shape: Shape::polyline(&[Point::new(0., 8000.), Point::new(6000., 8000.)], false),
                thickness: 600.,
                height: 1500.,
            }
        }

        fn fence() -> ObjectKind {
            ObjectKind::Linear {
                kind: LinearKind::Fence,
                shape: Shape::polyline(&[Point::new(0., 9500.), Point::new(6000., 9500.)], false),
                thickness: 100.,
                height: 1500.,
            }
        }

        fn care_change(s: &mut EditorState, id: ObjectId, now: i64) {
            let cmd = Command::SetCare {
                id,
                care: Some(PlantCare {
                    health: Health::Sick,
                    ..PlantCare::default()
                }),
            };
            assert!(s.try_execute(cmd, now).is_ok());
        }

        #[test]
        fn garden_mode_never_changes_layout() {
            let mut s = state_with(vec![plant_at(1000., 1000., 600.), lawn_rect()]);
            let plant = s.doc().objects[0].id;
            s.set_mode(Mode::Garden);
            s.garden_tap(Point::new(1000., 1000.), false);
            assert_eq!(s.selection, vec![plant]);
            assert!(s.handles().is_empty(), "no resize or rotate handles");
            let before = s.doc().clone();
            let changes = s.change_count();
            s.handle(Input::Down(Point::new(1000., 1000.), NO), 1);
            s.handle(Input::Move(Point::new(3000., 1000.), NO), 2);
            s.handle(Input::Up(Point::new(3000., 1000.), NO), 3);
            s.handle(Input::Key(Key::Delete, NO), 4);
            s.delete_selection(5);
            s.duplicate_selection(6);
            s.set_tool(ToolKind::Text);
            click(&mut s, Point::new(5000., 5000.), NO);
            s.set_units(UnitSystem::Imperial, 7);
            let mut moved = s.doc().objects[0].clone();
            moved.name = "renamed".into();
            assert!(s.replace_object(moved, 10).is_err());
            // even a direct layout command is refused
            assert!(s.try_execute(Command::Delete { id: plant }, 11).is_err());
            assert_eq!(s.doc(), &before);
            assert_eq!(s.change_count(), changes);
            assert_eq!(s.tool(), &ToolKind::Select);
        }

        #[test]
        fn garden_undo_stops_at_layout() {
            let mut s = state_with(vec![plant_at(1000., 1000., 600.)]);
            let id = s.doc().objects[0].id;
            s.handle(Input::Down(Point::new(1000., 1000.), NO), 1);
            s.handle(Input::Move(Point::new(1500., 1000.), NO), 2);
            s.handle(Input::Up(Point::new(1500., 1000.), NO), 3);
            assert_eq!(position(&s, 0), Point::new(1500., 1000.));
            s.set_mode(Mode::Garden);
            care_change(&mut s, id, 4);
            assert!(s.can_undo());
            assert!(s.undo(5));
            assert!(s.doc().care.is_empty());
            assert!(!s.can_undo());
            assert!(!s.undo(6));
            assert_eq!(position(&s, 0), Point::new(1500., 1000.));
            assert!(s.can_redo());
            assert!(s.redo(7));
            assert!(!s.doc().care.is_empty());
            s.set_mode(Mode::Design);
            assert!(s.undo(8));
            assert!(s.undo(9));
            assert_eq!(position(&s, 0), Point::new(1000., 1000.));
            // the next redo is the move: not offered in Garden mode
            s.set_mode(Mode::Garden);
            assert!(!s.can_redo());
            assert!(!s.redo(10));
            assert_eq!(position(&s, 0), Point::new(1000., 1000.));
        }

        #[test]
        fn taps_pick_plants_and_hedges() {
            let mut s = state_with(vec![
                lawn_rect(),
                plant_at(1000., 1000., 600.),
                hedge(),
                fence(),
                plant_at(9000., 1000., 600.),
                plant_at(12000., 1000., 600.),
            ]);
            let ids: Vec<ObjectId> = s.doc().objects.iter().map(|o| o.id).collect();
            s.set_mode(Mode::Garden);
            s.garden_tap(Point::new(1000., 1000.), false);
            assert_eq!(s.selection, vec![ids[1]]);
            s.garden_tap(Point::new(3000., 8000.), false);
            assert_eq!(s.selection, vec![ids[2]]);
            s.garden_tap(Point::new(1000., 1000.), true);
            assert_eq!(s.selection, vec![ids[2], ids[1]]);
            s.garden_tap(Point::new(3000., 8000.), true);
            assert_eq!(s.selection, vec![ids[1]]);
            // a lawn, a fence or empty ground clears
            s.garden_tap(Point::new(3000., 2000.), false);
            assert!(s.selection.is_empty());
            s.garden_tap(Point::new(1000., 1000.), false);
            s.garden_tap(Point::new(3000., 9500.), false);
            assert!(s.selection.is_empty());
            // an additive tap on nothing keeps the selection
            s.garden_tap(Point::new(1000., 1000.), false);
            s.garden_tap(Point::new(30000., 30000.), true);
            assert_eq!(s.selection, vec![ids[1]]);
            // locked objects and locked layers can be tapped, hidden layers can't
            let layer = s.doc().layers[0].id;
            let mut locked = s.doc().objects[4].clone();
            locked.locked = true;
            s.set_mode(Mode::Design);
            s.replace_object(locked, 1).unwrap();
            let hidden = loma_core::model::Layer {
                id: loma_core::model::LayerId::new(),
                name: "hidden".into(),
                visible: false,
                locked: false,
            };
            s.try_execute(
                Command::UpsertLayer {
                    layer: hidden.clone(),
                },
                2,
            )
            .unwrap();
            let mut on_hidden = s.doc().objects[5].clone();
            on_hidden.layer = hidden.id;
            s.replace_object(on_hidden, 3).unwrap();
            let mut lay = s.doc().layer(layer).unwrap().clone();
            lay.locked = true;
            s.try_execute(Command::UpsertLayer { layer: lay }, 4)
                .unwrap();
            s.set_mode(Mode::Garden);
            s.garden_tap(Point::new(9000., 1000.), false);
            assert_eq!(s.selection, vec![ids[4]]);
            s.garden_tap(Point::new(12000., 1000.), false);
            assert!(s.selection.is_empty());
            // taps do nothing in Design mode
            s.set_mode(Mode::Design);
            s.garden_tap(Point::new(9000., 1000.), false);
            assert!(s.selection.is_empty());
        }

        #[test]
        fn entering_garden_drops_layout_selection() {
            let mut s = state_with(vec![lawn_rect(), plant_at(9000., 1000., 600.)]);
            let ids: Vec<ObjectId> = s.doc().objects.iter().map(|o| o.id).collect();
            s.selection = ids.clone();
            s.set_mode(Mode::Garden);
            assert_eq!(s.selection, vec![ids[1]]);
            assert_eq!(s.mode(), Mode::Garden);
            // a half-drawn polygon is cancelled
            s.set_mode(Mode::Design);
            s.set_tool(ToolKind::Polygon(crate::tools::DrawTarget::Area(
                loma_core::model::AreaType::Bed,
            )));
            click(&mut s, Point::new(0., 0.), NO);
            click(&mut s, Point::new(1000., 0.), NO);
            assert_eq!(s.points_placed(), 2);
            s.set_mode(Mode::Garden);
            assert_eq!(s.points_placed(), 0);
            assert_eq!(s.tool(), &ToolKind::Select);
        }
    }
}
