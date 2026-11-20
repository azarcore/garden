use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::{BackgroundImage, LayerId, Object, ObjectId, PlantDef};
use crate::io::SCHEMA_VERSION;
use crate::units::UnitSystem;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
}

impl Layer {
    /// The layer a new design starts with.
    fn default_layer() -> Layer {
        Layer {
            id: LayerId::new(),
            name: "Garden".into(),
            visible: true,
            locked: false,
        }
    }
}

/// Marker left behind when an object is deleted, so future sync can see the delete.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tombstone {
    pub id: ObjectId,
    pub revision: u64,
    pub modified: i64,
}

/// A whole garden design.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,
    pub created: i64,
    pub modified: i64,
    pub units: UnitSystem,
    pub layers: Vec<Layer>,
    /// Drawing order: first is bottom.
    pub objects: Vec<Object>,
    pub tombstones: Vec<Tombstone>,
    pub background: Option<BackgroundImage>,
    pub custom_plants: Vec<PlantDef>,
    /// Prices for the garden list, per metric unit, by list row key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prices: BTreeMap<String, f64>,
    /// What the prices are in, as the person typed it (e.g. "€").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub currency: String,
    /// Gravel depth for the garden list (mm); None means the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravel_depth: Option<f64>,
}

impl Document {
    pub fn new(name: impl Into<String>, now_ms: i64) -> Document {
        Document {
            schema_version: SCHEMA_VERSION,
            id: Uuid::new_v4(),
            name: name.into(),
            created: now_ms,
            modified: now_ms,
            units: UnitSystem::default(),
            layers: vec![Layer::default_layer()],
            objects: Vec::new(),
            tombstones: Vec::new(),
            background: None,
            custom_plants: Vec::new(),
            prices: BTreeMap::new(),
            currency: String::new(),
            gravel_depth: None,
        }
    }

    /// Fix a document that came from outside (a file, storage): give it a
    /// layer if it has none, and move objects on missing layers to the first
    /// layer. Returns true when anything changed.
    pub fn repair(&mut self) -> bool {
        let mut changed = false;
        if self.layers.is_empty() {
            self.layers.push(Layer::default_layer());
            changed = true;
        }
        let first = self.layers[0].id;
        let known: std::collections::HashSet<LayerId> = self.layers.iter().map(|l| l.id).collect();
        for object in &mut self.objects {
            if !known.contains(&object.layer) {
                object.layer = first;
                changed = true;
            }
        }
        changed
    }

    /// True when the whole document can be saved (no NaN or infinite numbers).
    pub fn is_finite(&self) -> bool {
        self.objects.iter().all(|o| o.kind.is_finite())
            && self
                .background
                .as_ref()
                .is_none_or(BackgroundImage::is_finite)
            && self.custom_plants.iter().all(PlantDef::is_finite)
    }

    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn index_of(&self, id: ObjectId) -> Option<usize> {
        self.objects.iter().position(|o| o.id == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_has_default_layer() {
        let d = Document::new("My garden", 1_700_000_000_000);
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.layers[0].name, "Garden");
        assert_eq!(d.created, 1_700_000_000_000);
        assert_eq!(d.schema_version, 1);
    }
}
