use serde::{Deserialize, Serialize};

use super::{LayerId, Object, ObjectKind};
use crate::geometry::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlantCategory {
    Tree,
    Shrub,
    Perennial,
    Annual,
    Grass,
    Climber,
    Vegetable,
    Herb,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlantSymbol {
    Round,
    Star,
    Spiky,
    Leafy,
    Conifer,
}

/// A catalog entry (built-in or user-defined). Spread and height are mature sizes in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlantDef {
    pub id: String,
    pub name: String,
    pub category: PlantCategory,
    pub spread: f64,
    pub height: f64,
    pub symbol: PlantSymbol,
    pub custom: bool,
}

impl PlantDef {
    /// Create a placed plant. Values are copied so later catalog changes never resize it.
    pub fn place(&self, position: Point, layer: LayerId) -> Object {
        Object::new(
            self.name.clone(),
            ObjectKind::Plant {
                position,
                spread: self.spread,
                height: self.height,
                symbol: self.symbol,
                category: self.category,
                catalog_id: Some(self.id.clone()),
            },
            layer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerId, ObjectKind};

    #[test]
    fn placing_a_plant_copies_catalog_values() {
        let mut def = PlantDef {
            id: "lavender".into(),
            name: "Lavender".into(),
            category: PlantCategory::Shrub,
            spread: 600.,
            height: 500.,
            symbol: PlantSymbol::Spiky,
            custom: false,
        };
        let layer = LayerId::new();
        let o = def.place(Point::new(1., 2.), layer);
        def.spread = 9999.; // later catalog change
        match o.kind {
            ObjectKind::Plant {
                spread, catalog_id, ..
            } => {
                assert_eq!(spread, 600.);
                assert_eq!(catalog_id.as_deref(), Some("lavender"));
            }
            _ => panic!("not a plant"),
        }
        assert_eq!(o.name, "Lavender");
    }
}
