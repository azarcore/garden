//! Every change to a document is a `Command`; applying one returns its inverse.

use serde::{Deserialize, Serialize};

use crate::model::{
    BackgroundImage, Document, Layer, LayerId, LogEntry, LogId, Object, ObjectId, PlantCare,
    PlantDef, Product, ProductId, Tombstone,
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
    /// Replace a plant's care record; `None` (or an empty record) removes it.
    /// The object need not exist, so this undoes cleanly after a delete.
    SetCare {
        id: ObjectId,
        care: Option<PlantCare>,
    },
    /// Replace the log entry with the same id, or add it.
    UpsertLogEntry {
        entry: LogEntry,
    },
    RemoveLogEntry {
        id: LogId,
    },
    /// Replace the product with the same id, or add it.
    UpsertProduct {
        product: Product,
    },
    RemoveProduct {
        id: ProductId,
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
    #[error("the layout can't be changed in Garden mode")]
    LayoutLocked,
}

impl Command {
    /// True for changes to plant care, the log and the products library, which
    /// never touch the layout. A batch counts only when it is non-empty and all care.
    pub fn is_care(&self) -> bool {
        match self {
            Command::SetCare { .. }
            | Command::UpsertLogEntry { .. }
            | Command::RemoveLogEntry { .. }
            | Command::UpsertProduct { .. }
            | Command::RemoveProduct { .. } => true,
            Command::Batch { commands } => {
                !commands.is_empty() && commands.iter().all(Command::is_care)
            }
            _ => false,
        }
    }
}

impl Command {
    /// True for changes that never touch the layout: plant care, the design's
    /// name, and the garden list's prices, currency and gravel depth.
    /// These are the only changes allowed in Garden mode.
    pub fn is_layout_free(&self) -> bool {
        match self {
            Command::Rename { .. }
            | Command::SetPrice { .. }
            | Command::SetCurrency { .. }
            | Command::SetGravelDepth { .. } => true,
            Command::Batch { commands } => {
                !commands.is_empty() && commands.iter().all(Command::is_layout_free)
            }
            c => c.is_care(),
        }
    }
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
            Command::UpsertLogEntry { entry } => {
                !entry.plants.is_empty()
                    && entry.plants.len() == entry.plant_names.len()
                    && entry
                        .amount
                        .is_none_or(|a| a.value.is_finite() && a.value > 0.0)
            }
            Command::UpsertProduct { product } => {
                !product.name.trim().is_empty()
                    && product.reapply_days != Some(0)
                    && product.max_per_season != Some(0)
            }
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
            Command::SetCare { id, care } => {
                let old = match care.filter(|c| !c.is_empty()) {
                    Some(c) => self.care.insert(id, c),
                    None => self.care.remove(&id),
                };
                Command::SetCare { id, care: old }
            }
            Command::UpsertLogEntry { entry } => {
                match self.log.iter_mut().find(|e| e.id == entry.id) {
                    Some(slot) => Command::UpsertLogEntry {
                        entry: std::mem::replace(slot, entry),
                    },
                    None => {
                        let id = entry.id;
                        self.log.push(entry);
                        Command::RemoveLogEntry { id }
                    }
                }
            }
            Command::RemoveLogEntry { id } => {
                let i = self
                    .log
                    .iter()
                    .position(|e| e.id == id)
                    .ok_or(CommandError::NotFound)?;
                Command::UpsertLogEntry {
                    entry: self.log.remove(i),
                }
            }
            Command::UpsertProduct { product } => {
                match self.products.iter_mut().find(|p| p.id == product.id) {
                    Some(slot) => Command::UpsertProduct {
                        product: std::mem::replace(slot, product),
                    },
                    None => {
                        let id = product.id;
                        self.products.push(product);
                        Command::RemoveProduct { id }
                    }
                }
            }
            Command::RemoveProduct { id } => {
                let i = self
                    .products
                    .iter()
                    .position(|p| p.id == id)
                    .ok_or(CommandError::NotFound)?;
                Command::UpsertProduct {
                    product: self.products.remove(i),
                }
            }
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

    mod care {
        use super::*;
        use crate::date::Date;
        use crate::model::{
            Amount, AmountUnit, Health, Job, LogEntry, LogId, Photo, PlantCare, Product, ProductId,
            ProductKind,
        };

        fn day(s: &str) -> Date {
            Date::parse(s).unwrap()
        }

        pub(super) fn entry(plants: &[ObjectId]) -> LogEntry {
            LogEntry {
                id: LogId::new(),
                date: day("2024-06-01"),
                job: Job::Harvested,
                plants: plants.to_vec(),
                plant_names: plants.iter().map(|_| "Apple".to_string()).collect(),
                product: None,
                product_name: String::new(),
                amount: Some(Amount {
                    value: 2.5,
                    unit: AmountUnit::Kg,
                }),
                note: "first".into(),
                created_ms: 10,
            }
        }

        pub(super) fn product() -> Product {
            Product {
                id: ProductId::new(),
                name: "Copper".into(),
                kind: ProductKind::Fungicide,
                harvest_days: Some(21),
                reapply_days: Some(14),
                max_per_season: Some(3),
                notes: String::new(),
            }
        }

        #[test]
        fn set_care_undoes() {
            let mut d = Document::new("g", 0);
            let id = ObjectId::new();
            let care = PlantCare {
                health: Health::Watch,
                notes: "aphids".into(),
                photos: vec![Photo {
                    image_ref: "photo-1".into(),
                    date: day("2024-06-01"),
                    added_ms: 5,
                }],
            };
            let undo = d
                .apply(
                    Command::SetCare {
                        id,
                        care: Some(care.clone()),
                    },
                    1,
                )
                .unwrap();
            assert_eq!(d.care.get(&id), Some(&care));
            let redo = d.apply(undo, 2).unwrap();
            assert!(d.care.is_empty());
            d.apply(redo, 3).unwrap();
            assert_eq!(d.care.get(&id), Some(&care));
            // an empty record is the same as none
            let undo = d
                .apply(
                    Command::SetCare {
                        id,
                        care: Some(PlantCare::default()),
                    },
                    4,
                )
                .unwrap();
            assert!(d.care.is_empty());
            d.apply(undo, 5).unwrap();
            assert_eq!(d.care.get(&id), Some(&care));
        }

        #[test]
        fn log_entries_undo() {
            let mut d = Document::new("g", 0);
            let first = entry(&[ObjectId::new()]);
            let undo_add = d
                .apply(
                    Command::UpsertLogEntry {
                        entry: first.clone(),
                    },
                    1,
                )
                .unwrap();
            let mut edited = first.clone();
            edited.note = "edited".into();
            let undo_edit = d
                .apply(
                    Command::UpsertLogEntry {
                        entry: edited.clone(),
                    },
                    2,
                )
                .unwrap();
            assert_eq!(d.log, vec![edited.clone()]);
            d.apply(undo_edit, 3).unwrap();
            assert_eq!(d.log, vec![first.clone()]);
            d.apply(undo_add, 4).unwrap();
            assert!(d.log.is_empty());
            // remove, then undo, restores an equal entry
            d.apply(
                Command::UpsertLogEntry {
                    entry: first.clone(),
                },
                5,
            )
            .unwrap();
            let undo = d
                .apply(Command::RemoveLogEntry { id: first.id }, 6)
                .unwrap();
            assert!(d.log.is_empty());
            d.apply(undo, 7).unwrap();
            assert_eq!(d.log, vec![first.clone()]);
            assert_eq!(
                d.apply(Command::RemoveLogEntry { id: LogId::new() }, 8),
                Err(CommandError::NotFound)
            );
        }

        #[test]
        fn products_undo() {
            let mut d = Document::new("g", 0);
            let p = product();
            let undo_add = d
                .apply(Command::UpsertProduct { product: p.clone() }, 1)
                .unwrap();
            let mut edited = p.clone();
            edited.harvest_days = Some(7);
            let undo_edit = d
                .apply(
                    Command::UpsertProduct {
                        product: edited.clone(),
                    },
                    2,
                )
                .unwrap();
            assert_eq!(d.products, vec![edited]);
            d.apply(undo_edit, 3).unwrap();
            assert_eq!(d.products, vec![p.clone()]);
            d.apply(undo_add, 4).unwrap();
            assert!(d.products.is_empty());
            d.apply(Command::UpsertProduct { product: p.clone() }, 5)
                .unwrap();
            let undo = d.apply(Command::RemoveProduct { id: p.id }, 6).unwrap();
            assert!(d.products.is_empty());
            d.apply(undo, 7).unwrap();
            assert_eq!(d.products, vec![p.clone()]);
            assert_eq!(
                d.apply(
                    Command::RemoveProduct {
                        id: ProductId::new()
                    },
                    8
                ),
                Err(CommandError::NotFound)
            );
        }

        #[test]
        fn care_rejects_bad_values() {
            let mut d = Document::new("g", 0);
            let before = d.clone();
            let bad = |e: LogEntry| Command::UpsertLogEntry { entry: e };
            let mut no_plants = entry(&[]);
            no_plants.plant_names.clear();
            let mut names_mismatch = entry(&[ObjectId::new()]);
            names_mismatch.plant_names.push("extra".into());
            let mut nan = entry(&[ObjectId::new()]);
            nan.amount = Some(Amount {
                value: f64::NAN,
                unit: AmountUnit::Kg,
            });
            let mut zero = entry(&[ObjectId::new()]);
            zero.amount = Some(Amount {
                value: 0.0,
                unit: AmountUnit::G,
            });
            let mut blank = product();
            blank.name = "  ".into();
            let mut zero_reapply = product();
            zero_reapply.reapply_days = Some(0);
            let mut zero_max = product();
            zero_max.max_per_season = Some(0);
            for cmd in [
                bad(no_plants),
                bad(names_mismatch),
                bad(nan),
                bad(zero),
                Command::UpsertProduct { product: blank },
                Command::UpsertProduct {
                    product: zero_reapply,
                },
                Command::UpsertProduct { product: zero_max },
            ] {
                assert_eq!(
                    d.apply(cmd.clone(), 1),
                    Err(CommandError::InvalidValue),
                    "{cmd:?}"
                );
            }
            assert_eq!(d, before);
            // zero days before harvest is a real label value
            let mut ok = product();
            ok.harvest_days = Some(0);
            assert!(d.apply(Command::UpsertProduct { product: ok }, 2).is_ok());
        }

        #[test]
        fn deleting_a_plant_keeps_its_care() {
            let (mut d, a, _) = super::setup();
            let care = PlantCare {
                health: Health::Sick,
                ..PlantCare::default()
            };
            d.apply(
                Command::SetCare {
                    id: a.id,
                    care: Some(care.clone()),
                },
                1,
            )
            .unwrap();
            let e = entry(&[a.id]);
            d.apply(Command::UpsertLogEntry { entry: e.clone() }, 2)
                .unwrap();
            d.apply(Command::Delete { id: a.id }, 3).unwrap();
            assert_eq!(d.care.get(&a.id), Some(&care));
            assert_eq!(d.log, vec![e]);
        }

        #[test]
        fn is_care_sorts_commands() {
            let care = [
                Command::SetCare {
                    id: ObjectId::new(),
                    care: None,
                },
                Command::UpsertLogEntry {
                    entry: entry(&[ObjectId::new()]),
                },
                Command::RemoveLogEntry { id: LogId::new() },
                Command::UpsertProduct { product: product() },
                Command::RemoveProduct {
                    id: ProductId::new(),
                },
            ];
            for c in &care {
                assert!(c.is_care(), "{c:?}");
            }
            assert!(
                Command::Batch {
                    commands: care.to_vec()
                }
                .is_care()
            );
            let layout = Command::Delete {
                id: ObjectId::new(),
            };
            assert!(!layout.is_care());
            assert!(
                !Command::SetPrice {
                    key: "x".into(),
                    price: None
                }
                .is_care()
            );
            assert!(!Command::Batch { commands: vec![] }.is_care());
            assert!(
                !Command::Batch {
                    commands: vec![care[0].clone(), layout]
                }
                .is_care()
            );
        }
    }
}
