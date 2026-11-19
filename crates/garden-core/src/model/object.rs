use serde::{Deserialize, Serialize};

use super::{LayerId, ObjectId, PlantCategory, PlantSymbol};
use crate::geometry::{Point, Shape};

/// RGBA colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Style {
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub material: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaType {
    Bed,
    Lawn,
    Paving,
    Gravel,
    Water,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearKind {
    Fence,
    Wall,
    Hedge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructureKind {
    Shed,
    Greenhouse,
    Pergola,
    RaisedBed,
    Furniture,
    Other,
}

/// What an object is, with its geometry. All lengths in millimetres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObjectKind {
    PlotBoundary { shape: Shape },
    Area { area_type: AreaType, shape: Shape },
    Path { shape: Shape, width: f64 },
    Linear { kind: LinearKind, shape: Shape, thickness: f64, height: f64 },
    Structure { kind: StructureKind, footprint: Shape, height: f64 },
    Plant {
        position: Point,
        spread: f64,
        height: f64,
        symbol: PlantSymbol,
        category: PlantCategory,
        catalog_id: Option<String>,
    },
    Label { position: Point, text: String, size: f64 },
    Dimension { from: Point, to: Point, offset: f64 },
}

/// Anything placed in the design.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: String,
    pub kind: ObjectKind,
    pub style: Style,
    pub layer: LayerId,
    pub locked: bool,
    pub revision: u64,
    /// Unix epoch milliseconds of the last change.
    pub modified: i64,
}

impl Object {
    pub fn new(name: impl Into<String>, kind: ObjectKind, layer: LayerId) -> Object {
        Object {
            id: ObjectId::new(),
            name: name.into(),
            kind,
            style: Style::default(),
            layer,
            locked: false,
            revision: 0,
            modified: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Point, Shape};
    use crate::model::LayerId;

    #[test]
    fn object_kind_json_is_tagged() {
        let o = Object::new(
            "Bed",
            ObjectKind::Area { area_type: AreaType::Bed, shape: Shape::rect(Point::ZERO, 1., 1.) },
            LayerId::new(),
        );
        let v: serde_json::Value = serde_json::to_value(&o).unwrap();
        assert_eq!(v["kind"]["type"], "area");
        assert_eq!(v["kind"]["area_type"], "bed");
    }
}
