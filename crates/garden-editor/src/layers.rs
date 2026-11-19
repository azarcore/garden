//! Layers (which one new objects go on, adding, changing and deleting them)
//! and stacking order of the selection.

use garden_core::command::{Command, CommandError};
use garden_core::model::{Layer, LayerId, Object, ObjectId};

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
    /// unlocked, otherwise the first layer that is.
    pub fn active_layer_id(&self) -> LayerId {
        let layers = &self.doc().layers;
        let usable = |l: &&Layer| l.visible && !l.locked;
        self.active
            .and_then(|id| layers.iter().find(|l| l.id == id))
            .filter(usable)
            .or_else(|| layers.iter().find(usable))
            .unwrap_or(&layers[0])
            .id
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
            self.execute(Command::UpsertLayer { layer }, now_ms);
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
        if !commands.is_empty() {
            self.execute(Command::Batch { commands }, now_ms);
        }
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
