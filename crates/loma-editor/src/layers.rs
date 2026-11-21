//! Layers (which one new objects go on, adding, changing and deleting them)
//! and stacking order of the selection.

use kurbo::Vec2;
use loma_core::command::{Command, CommandError};
use loma_core::model::{Layer, LayerId, Object, ObjectId};

use crate::objects::{object_bounds, translate_kind};

use crate::state::EditorState;
use crate::strings::EN;

/// Where to move the selection in the stacking order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZOrder {
    Front,
    Back,
    Forward,
    Backward,
}

/// Which edge or centre line of the selection to line objects up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

/// Commands that turn the order `current` into `target` (same ids), applied
/// one after another.
fn reorder_commands(mut current: Vec<ObjectId>, target: &[ObjectId]) -> Vec<Command> {
    let mut commands = Vec::new();
    for (index, id) in target.iter().enumerate() {
        if current[index] != *id {
            let from = current.iter().position(|c| c == id).unwrap_or(index);
            let moved = current.remove(from);
            current.insert(index, moved);
            commands.push(Command::Reorder { id: *id, index });
        }
    }
    commands
}

impl EditorState {
    /// The layer new objects go on: the chosen one while it is visible and
    /// unlocked, otherwise the first layer that is, otherwise the first layer.
    /// Documents are repaired to have a layer when opened; should one still
    /// have none, this gives the nil id instead of panicking.
    pub fn active_layer_id(&self) -> LayerId {
        let layers = &self.doc().layers;
        let usable = |l: &&Layer| l.visible && !l.locked;
        self.active
            .and_then(|id| layers.iter().find(|l| l.id == id))
            .filter(usable)
            .or_else(|| layers.iter().find(usable))
            .or_else(|| layers.first())
            .map_or(LayerId::NIL, |l| l.id)
    }

    /// Choose the layer new objects go on (ignored if it doesn't exist).
    pub fn set_active_layer(&mut self, id: LayerId) {
        if self.doc().layer(id).is_some() {
            self.active = Some(id);
        }
    }

    /// Add "Layer N" on top and make it the active layer.
    pub fn add_layer(&mut self, now_ms: i64) -> LayerId {
        let taken = |name: &str| self.doc().layers.iter().any(|l| l.name == name);
        let name = (2..)
            .map(|n| format!("{} {n}", EN.layer_prefix))
            .find(|n| !taken(n))
            .unwrap_or_default();
        let layer = Layer {
            id: LayerId::new(),
            name,
            visible: true,
            locked: false,
        };
        let id = layer.id;
        if self.execute(Command::UpsertLayer { layer }, now_ms) {
            self.active = Some(id);
        }
        id
    }

    /// Rename, show/hide or lock/unlock a layer (undoable).
    pub fn update_layer(&mut self, layer: Layer, now_ms: i64) {
        let name = layer.name.trim();
        if name.is_empty() {
            return;
        }
        let layer = Layer {
            name: name.to_string(),
            ..layer
        };
        if self
            .doc()
            .layer(layer.id)
            .is_some_and(|existing| *existing != layer)
        {
            let id = layer.id;
            if self.execute(Command::UpsertLayer { layer }, now_ms) {
                self.deselect_unusable_layer(id);
            }
        }
    }

    /// Delete an empty layer. Refused while objects are on it, or if it is the last.
    pub fn delete_layer(&mut self, id: LayerId, now_ms: i64) -> Result<(), CommandError> {
        self.try_execute(Command::RemoveLayer { id }, now_ms)?;
        if self.active == Some(id) {
            self.active = self.doc().layers.first().map(|l| l.id);
        }
        Ok(())
    }

    /// Move the selected (unlocked) objects to another layer, as one undo step.
    pub fn move_selection_to_layer(&mut self, id: LayerId, now_ms: i64) {
        if self.doc().layer(id).is_none() {
            return;
        }
        let commands: Vec<Command> = self
            .doc()
            .objects
            .iter()
            .filter(|o| !o.locked && o.layer != id && self.selection.contains(&o.id))
            .map(|o| Command::Replace {
                object: Object {
                    layer: id,
                    ..o.clone()
                },
            })
            .collect();
        if !commands.is_empty() && self.execute(Command::Batch { commands }, now_ms) {
            self.deselect_unusable_layer(id);
        }
    }

    /// Objects on a hidden or locked layer can't be seen or touched, so they
    /// can't stay selected.
    fn deselect_unusable_layer(&mut self, id: LayerId) {
        let doc = self.editor.doc();
        if doc.layer(id).is_some_and(|l| l.visible && !l.locked) {
            return;
        }
        self.selection
            .retain(|o| doc.object(*o).is_some_and(|o| o.layer != id));
    }

    /// Move the selected (unlocked) objects in the stacking order, keeping
    /// their order among themselves, as one undo step.
    pub fn reorder_selection(&mut self, to: ZOrder, now_ms: i64) {
        let doc = self.doc();
        let current: Vec<ObjectId> = doc.objects.iter().map(|o| o.id).collect();
        let picked = |id: &ObjectId| {
            self.selection.contains(id) && doc.object(*id).is_some_and(|o| !o.locked)
        };
        let mut target = current.clone();
        match to {
            ZOrder::Front | ZOrder::Back => {
                let (sel, rest): (Vec<ObjectId>, Vec<ObjectId>) =
                    current.iter().partition(|id| picked(id));
                target = if to == ZOrder::Front {
                    [rest, sel].concat()
                } else {
                    [sel, rest].concat()
                };
            }
            ZOrder::Forward => {
                for i in (0..target.len().saturating_sub(1)).rev() {
                    if picked(&target[i]) && !picked(&target[i + 1]) {
                        target.swap(i, i + 1);
                    }
                }
            }
            ZOrder::Backward => {
                for i in 1..target.len() {
                    if picked(&target[i]) && !picked(&target[i - 1]) {
                        target.swap(i, i - 1);
                    }
                }
            }
        }
        let commands = reorder_commands(current, &target);
        if !commands.is_empty() {
            self.execute(Command::Batch { commands }, now_ms);
        }
    }
}

impl EditorState {
    /// Line up two or more selected (unlocked) objects by their bounds, as one undo step.
    pub fn align_selection(&mut self, how: Align, now_ms: i64) {
        let objects: Vec<&Object> = self
            .doc()
            .objects
            .iter()
            .filter(|o| !o.locked && self.selection.contains(&o.id))
            .collect();
        if objects.len() < 2 {
            return;
        }
        let Some(all) = objects
            .iter()
            .map(|o| object_bounds(&o.kind))
            .reduce(|a, b| a.union(b))
        else {
            return;
        };
        let commands: Vec<Command> = objects
            .iter()
            .filter_map(|o| {
                let b = object_bounds(&o.kind);
                let d = match how {
                    Align::Left => Vec2::new(all.x0 - b.x0, 0.0),
                    Align::CenterX => Vec2::new(all.center().x - b.center().x, 0.0),
                    Align::Right => Vec2::new(all.x1 - b.x1, 0.0),
                    Align::Top => Vec2::new(0.0, all.y0 - b.y0),
                    Align::CenterY => Vec2::new(0.0, all.center().y - b.center().y),
                    Align::Bottom => Vec2::new(0.0, all.y1 - b.y1),
                };
                (d.hypot() > 1e-9).then(|| Command::Replace {
                    object: Object {
                        kind: translate_kind(&o.kind, d),
                        ..(*o).clone()
                    },
                })
            })
            .collect();
        if !commands.is_empty() {
            self.execute(Command::Batch { commands }, now_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::object_bounds;
    use crate::test_util::{plant_at, state_with};

    #[test]
    fn hiding_or_locking_a_layer_deselects_its_objects() {
        let mut s = state_with(vec![plant_at(0., 0., 600.)]);
        s.selection = vec![s.doc().objects[0].id];
        let mut layer = s.doc().layers[0].clone();
        layer.visible = false;
        s.update_layer(layer, 1);
        assert!(s.selection.is_empty());
    }

    #[test]
    fn moving_to_a_hidden_or_locked_layer_deselects() {
        let mut s = state_with(vec![plant_at(0., 0., 600.)]);
        let l2 = s.add_layer(1);
        let mut layer = s.doc().layers[1].clone();
        layer.locked = true;
        s.update_layer(layer, 2);
        s.selection = vec![s.doc().objects[0].id];
        s.move_selection_to_layer(l2, 3);
        assert_eq!(s.doc().objects[0].layer, l2);
        assert!(s.selection.is_empty());
    }

    #[test]
    fn align_left_lines_up_bounds() {
        let mut s = state_with(vec![
            plant_at(1000., 0., 600.),
            plant_at(3000., 2000., 1000.),
        ]);
        s.selection = s.doc().objects.iter().map(|o| o.id).collect();
        s.align_selection(Align::Left, 1);
        let lefts: Vec<f64> = s
            .doc()
            .objects
            .iter()
            .map(|o| object_bounds(&o.kind).x0)
            .collect();
        assert!((lefts[0] - 700.).abs() < 1e-9 && (lefts[1] - 700.).abs() < 1e-9);
        assert!(s.undo(2));
        assert!(!s.can_undo());
    }

    #[test]
    fn align_centres_and_bottoms() {
        let mut s = state_with(vec![
            plant_at(1000., 0., 600.),
            plant_at(3000., 2000., 1000.),
        ]);
        s.selection = s.doc().objects.iter().map(|o| o.id).collect();
        s.align_selection(Align::CenterY, 1);
        let mids: Vec<f64> = s
            .doc()
            .objects
            .iter()
            .map(|o| object_bounds(&o.kind).center().y)
            .collect();
        assert!((mids[0] - mids[1]).abs() < 1e-9);
        s.align_selection(Align::Bottom, 2);
        let bottoms: Vec<f64> = s
            .doc()
            .objects
            .iter()
            .map(|o| object_bounds(&o.kind).y1)
            .collect();
        assert!((bottoms[0] - bottoms[1]).abs() < 1e-9);
        // one object: nothing to align with
        s.selection.truncate(1);
        let before = s.change_count();
        s.align_selection(Align::Right, 3);
        assert_eq!(s.change_count(), before);
    }

    fn place_lavender(s: &mut EditorState) {
        let def = crate::catalog::builtin_plants()
            .into_iter()
            .find(|p| p.id == "lavender")
            .unwrap();
        s.set_tool(crate::tools::ToolKind::Plant(def));
        crate::test_util::click(
            s,
            loma_core::geometry::Point::new(300., 200.),
            crate::test_util::NO,
        );
    }

    #[test]
    fn a_file_without_layers_opens_and_takes_plants() {
        let doc = loma_core::io::from_json(include_str!(
            "../../loma-core/tests/fixtures/no_layers.loma.json"
        ))
        .unwrap();
        let mut s = EditorState::new(doc, 1000., 500.);
        place_lavender(&mut s);
        assert_eq!(s.doc().objects.len(), 2);
        let layer = s.doc().layers[0].id;
        assert!(s.doc().objects.iter().all(|o| o.layer == layer));
    }

    #[test]
    fn a_document_without_layers_is_repaired_when_opened() {
        let mut empty = loma_core::model::Document::new("g", 0);
        empty.layers.clear();
        let mut s = EditorState::new(empty.clone(), 1000., 500.);
        assert_eq!(s.doc().layers.len(), 1);
        assert_eq!(s.active_layer_id(), s.doc().layers[0].id);
        s.open_document(empty);
        assert_eq!(s.doc().layers.len(), 1);
        place_lavender(&mut s);
        assert_eq!(s.doc().objects.len(), 1);
        assert_eq!(s.doc().objects[0].layer, s.doc().layers[0].id);
    }
}
