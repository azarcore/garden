//! Every change to a document is a `Command`; applying one returns its inverse.

use serde::{Deserialize, Serialize};

use crate::model::{
    BackgroundImage, Document, Layer, LayerId, Object, ObjectId, PlantDef, Tombstone,
};
use crate::units::UnitSystem;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// `index` is clamped to the number of objects.
    Insert {
        object: Object,
        index: usize,
    },
    Delete {
        id: ObjectId,
    },
    /// Replace the object with the same id, keeping its drawing position.
    Replace {
        object: Object,
    },
    /// `index` is clamped.
    Reorder {
        id: ObjectId,
        index: usize,
    },
    UpsertLayer {
        layer: Layer,
    },
    /// Fails if any object uses the layer or it is the last one.
    RemoveLayer {
        id: LayerId,
    },
    SetUnits {
        units: UnitSystem,
    },
    SetBackground {
        background: Option<BackgroundImage>,
    },
    UpsertCustomPlant {
        def: PlantDef,
    },
    RemoveCustomPlant {
        id: String,
    },
    Rename {
        name: String,
    },
    /// A garden list price (per metric unit); `None` removes it.
    SetPrice {
        key: String,
        price: Option<f64>,
    },
    SetCurrency {
        currency: String,
    },
    /// Gravel depth for the garden list (mm, > 0); `None` means the default.
    SetGravelDepth {
        depth: Option<f64>,
    },
    /// All-or-nothing.
    Batch {
        commands: Vec<Command>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CommandError {
    #[error("object not found")]
    NotFound,
    #[error("duplicate id")]
    DuplicateId,
    #[error("unknown layer")]
    UnknownLayer,
    #[error("layer in use")]
    LayerInUse,
    #[error("last layer")]
    LastLayer,
    #[error("a number is not finite")]
    InvalidValue,
}

impl Document {
    /// Apply `cmd` and return the command that undoes it.
    /// On error the document is left exactly as it was.
    pub fn apply(&mut self, cmd: Command, now_ms: i64) -> Result<Command, CommandError> {
        let backup = self.clone();
        match self.apply_inner(cmd, now_ms) {
            Ok(inverse) => {
                self.modified = now_ms;
                Ok(inverse)
            }
            Err(e) => {
                *self = backup;
                Err(e)
            }
        }
    }

    fn tombstone_revision(&self, id: ObjectId) -> u64 {
        self.tombstones
            .iter()
            .find(|t| t.id == id)
            .map_or(0, |t| t.revision)
    }

    fn apply_inner(&mut self, cmd: Command, now: i64) -> Result<Command, CommandError> {
        let finite = match &cmd {
            Command::Insert { object, .. } | Command::Replace { object } => object.kind.is_finite(),
            Command::SetBackground {
                background: Some(b),
            } => b.is_finite(),
            Command::UpsertCustomPlant { def } => def.is_finite(),
            Command::SetPrice { price: Some(p), .. } => p.is_finite() && *p >= 0.0,
            Command::SetGravelDepth { depth: Some(d) } => d.is_finite() && *d > 0.0,
            _ => true,
        };
        if !finite {
            return Err(CommandError::InvalidValue);
        }
        Ok(match cmd {
            Command::Insert { mut object, index } => {
                if self.index_of(object.id).is_some() {
                    return Err(CommandError::DuplicateId);
                }
                if self.layer(object.layer).is_none() {
                    return Err(CommandError::UnknownLayer);
                }
                object.revision = object
                    .revision
                    .max(self.tombstone_revision(object.id))
                    .saturating_add(1);
                object.modified = now;
                self.tombstones.retain(|t| t.id != object.id);
                let id = object.id;
                let index = index.min(self.objects.len());
                self.objects.insert(index, object);
                Command::Delete { id }
            }
            Command::Delete { id } => {
                let index = self.index_of(id).ok_or(CommandError::NotFound)?;
                let object = self.objects.remove(index);
                self.tombstones.retain(|t| t.id != id);
                self.tombstones.push(Tombstone {
                    id,
                    revision: object.revision.saturating_add(1),
                    modified: now,
                });
                Command::Insert { object, index }
            }
            Command::Replace { mut object } => {
                let index = self.index_of(object.id).ok_or(CommandError::NotFound)?;
                if self.layer(object.layer).is_none() {
                    return Err(CommandError::UnknownLayer);
                }
                let old = self.objects[index].clone();
                object.revision = object.revision.max(old.revision).saturating_add(1);
                object.modified = now;
                self.objects[index] = object;
                Command::Replace { object: old }
            }
            Command::Reorder { id, index } => {
                let old = self.index_of(id).ok_or(CommandError::NotFound)?;
                let object = self.objects.remove(old);
                let index = index.min(self.objects.len());
                self.objects.insert(index, object);
                Command::Reorder { id, index: old }
            }
            Command::UpsertLayer { layer } => {
                match self.layers.iter_mut().find(|l| l.id == layer.id) {
                    Some(existing) => Command::UpsertLayer {
                        layer: std::mem::replace(existing, layer),
                    },
                    None => {
                        let id = layer.id;
                        self.layers.push(layer);
                        Command::RemoveLayer { id }
                    }
                }
            }
            Command::RemoveLayer { id } => {
                let index = self
                    .layers
                    .iter()
                    .position(|l| l.id == id)
                    .ok_or(CommandError::UnknownLayer)?;
                if self.objects.iter().any(|o| o.layer == id) {
                    return Err(CommandError::LayerInUse);
                }
                if self.layers.len() == 1 {
                    return Err(CommandError::LastLayer);
                }
                Command::UpsertLayer {
                    layer: self.layers.remove(index),
                }
            }
            Command::SetUnits { units } => Command::SetUnits {
                units: std::mem::replace(&mut self.units, units),
            },
            Command::SetBackground { background } => Command::SetBackground {
                background: std::mem::replace(&mut self.background, background),
            },
            Command::UpsertCustomPlant { def } => {
                match self.custom_plants.iter_mut().find(|p| p.id == def.id) {
                    Some(existing) => Command::UpsertCustomPlant {
                        def: std::mem::replace(existing, def),
                    },
                    None => {
                        let id = def.id.clone();
                        self.custom_plants.push(def);
                        Command::RemoveCustomPlant { id }
                    }
                }
            }
            Command::RemoveCustomPlant { id } => {
                let index = self
                    .custom_plants
                    .iter()
                    .position(|p| p.id == id)
                    .ok_or(CommandError::NotFound)?;
                Command::UpsertCustomPlant {
                    def: self.custom_plants.remove(index),
                }
            }
            Command::Rename { name } => Command::Rename {
                name: std::mem::replace(&mut self.name, name),
            },
            Command::SetPrice { key, price } => {
                let old = match price {
                    Some(p) => self.prices.insert(key.clone(), p),
                    None => self.prices.remove(&key),
                };
                Command::SetPrice { key, price: old }
            }
            Command::SetCurrency { currency } => Command::SetCurrency {
                currency: std::mem::replace(&mut self.currency, currency),
            },
            Command::SetGravelDepth { depth } => Command::SetGravelDepth {
                depth: std::mem::replace(&mut self.gravel_depth, depth),
            },
            Command::Batch { commands } => {
                let mut inverses = Vec::with_capacity(commands.len());
                for c in commands {
                    inverses.push(self.apply_inner(c, now)?);
                }
                inverses.reverse();
                Command::Batch { commands: inverses }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;
    use crate::model::ObjectKind;

    #[test]
    fn set_price_undoes() {
        let mut d = Document::new("g", 0);
        let set = |price| Command::SetPrice {
            key: "area:lawn".into(),
            price,
        };
        let undo = d.apply(set(Some(2.5)), 1).unwrap();
        assert_eq!(d.prices.get("area:lawn"), Some(&2.5));
        let redo = d.apply(undo, 2).unwrap();
        assert!(d.prices.is_empty());
        d.apply(redo, 3).unwrap();
        assert_eq!(d.prices.get("area:lawn"), Some(&2.5));
        let undo = d.apply(set(Some(4.0)), 4).unwrap();
        d.apply(undo, 5).unwrap();
        assert_eq!(d.prices.get("area:lawn"), Some(&2.5));
        let undo = d.apply(set(None), 6).unwrap();
        assert!(d.prices.is_empty());
        d.apply(undo, 7).unwrap();
        assert_eq!(d.prices.get("area:lawn"), Some(&2.5));
        let undo = d
            .apply(
                Command::SetCurrency {
                    currency: "€".into(),
                },
                8,
            )
            .unwrap();
        assert_eq!(d.currency, "€");
        d.apply(undo, 9).unwrap();
        assert_eq!(d.currency, "");
    }

    #[test]
    fn gravel_depth_is_saved_and_undoes() {
        let mut d = Document::new("g", 0);
        assert_eq!(d.gravel_depth, None);
        let undo = d
            .apply(Command::SetGravelDepth { depth: Some(80.0) }, 1)
            .unwrap();
        assert_eq!(d.gravel_depth, Some(80.0));
        d.apply(undo, 2).unwrap();
        assert_eq!(d.gravel_depth, None);
        for bad in [0.0, -5.0, f64::NAN] {
            assert_eq!(
                d.apply(Command::SetGravelDepth { depth: Some(bad) }, 3),
                Err(CommandError::InvalidValue),
                "{bad}"
            );
        }
        let json = crate::io::to_json(&d).unwrap();
        assert!(!json.contains("gravel_depth"), "unset depth is not written");
        d.apply(Command::SetGravelDepth { depth: Some(80.0) }, 4)
            .unwrap();
        let back = crate::io::from_json(&crate::io::to_json(&d).unwrap()).unwrap();
        assert_eq!(back.gravel_depth, Some(80.0));
    }

    #[test]
    fn set_price_rejects_bad_values() {
        let mut d = Document::new("g", 0);
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            let cmd = Command::SetPrice {
                key: "k".into(),
                price: Some(bad),
            };
            assert_eq!(d.apply(cmd, 1), Err(CommandError::InvalidValue), "{bad}");
        }
        assert!(d.prices.is_empty());
    }

    fn setup() -> (Document, Object, Object) {
        let mut d = Document::new("g", 0);
        let l = d.layers[0].id;
        let a = Object::new(
            "a",
            ObjectKind::Label {
                position: Point::ZERO,
                text: "a".into(),
                size: 100.,
            },
            l,
        );
        let b = Object::new(
            "b",
            ObjectKind::Label {
                position: Point::ZERO,
                text: "b".into(),
                size: 100.,
            },
            l,
        );
        d.apply(
            Command::Insert {
                object: a.clone(),
                index: 0,
            },
            1,
        )
        .unwrap();
        d.apply(
            Command::Insert {
                object: b.clone(),
                index: 1,
            },
            2,
        )
        .unwrap();
        (d, a, b)
    }

    #[test]
    fn delete_then_inverse_restores_position_and_clears_tombstone() {
        let (mut d, a, _) = setup();
        let r = d.object(a.id).unwrap().revision;
        let inv = d.apply(Command::Delete { id: a.id }, 3).unwrap();
        assert_eq!(d.tombstones.len(), 1);
        assert_eq!(d.tombstones[0].revision, r + 1);
        assert_eq!(d.objects.len(), 1);
        d.apply(inv, 4).unwrap();
        assert_eq!(d.objects[0].id, a.id);
        assert!(d.tombstones.is_empty());
        assert_eq!(d.objects[0].revision, r + 2); // advanced past the tombstone
        assert_eq!(d.modified, 4);
    }

    #[test]
    fn replace_bumps_revision_and_inverts() {
        let (mut d, a, _) = setup();
        let mut a2 = d.object(a.id).unwrap().clone();
        a2.name = "renamed".into();
        let r0 = d.object(a.id).unwrap().revision;
        let inv = d.apply(Command::Replace { object: a2 }, 5).unwrap();
        assert_eq!(d.object(a.id).unwrap().revision, r0 + 1);
        assert_eq!(d.object(a.id).unwrap().modified, 5);
        d.apply(inv, 6).unwrap();
        assert_eq!(d.object(a.id).unwrap().name, "a");
    }

    #[test]
    fn errors_leave_document_unchanged() {
        let (mut d, a, _) = setup();
        let before = d.clone();
        assert_eq!(
            d.apply(
                Command::Insert {
                    object: a.clone(),
                    index: 0
                },
                9
            ),
            Err(CommandError::DuplicateId)
        );
        assert_eq!(
            d.apply(
                Command::Delete {
                    id: ObjectId::new()
                },
                9
            ),
            Err(CommandError::NotFound)
        );
        assert_eq!(
            d.apply(Command::RemoveLayer { id: d.layers[0].id }, 9),
            Err(CommandError::LayerInUse)
        );
        let bad = Command::Batch {
            commands: vec![
                Command::Delete { id: a.id },
                Command::Delete {
                    id: ObjectId::new(),
                },
            ],
        };
        assert_eq!(d.apply(bad, 9), Err(CommandError::NotFound));
        assert_eq!(d, before);
    }

    #[test]
    fn insert_on_unknown_layer_fails() {
        let mut d = Document::new("g", 0);
        let o = Object::new(
            "x",
            ObjectKind::Label {
                position: Point::ZERO,
                text: "x".into(),
                size: 1.,
            },
            LayerId::new(),
        );
        assert_eq!(
            d.apply(
                Command::Insert {
                    object: o,
                    index: 0
                },
                1
            ),
            Err(CommandError::UnknownLayer)
        );
    }

    #[test]
    fn cannot_remove_last_layer() {
        let mut d = Document::new("g", 0);
        assert_eq!(
            d.apply(Command::RemoveLayer { id: d.layers[0].id }, 1),
            Err(CommandError::LastLayer)
        );
    }

    #[test]
    fn reorder_and_inverse() {
        let (mut d, a, b) = setup();
        let inv = d
            .apply(
                Command::Reorder {
                    id: a.id,
                    index: 99,
                },
                3,
            )
            .unwrap();
        assert_eq!(d.objects[1].id, a.id);
        d.apply(inv, 4).unwrap();
        assert_eq!(d.objects[1].id, b.id);
    }

    #[test]
    fn commands_serialize() {
        let (_, a, _) = setup();
        let c = Command::Batch {
            commands: vec![
                Command::Delete { id: a.id },
                Command::SetUnits {
                    units: UnitSystem::Imperial,
                },
            ],
        };
        let back: Command = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
