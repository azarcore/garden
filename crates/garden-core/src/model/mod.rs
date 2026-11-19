//! The garden design document and everything placed in it.

mod background;
mod document;
mod ids;
mod object;
mod plant;

pub use background::{BackgroundImage, CalibrationError};
pub use document::{Document, Layer, Tombstone};
pub use ids::{LayerId, ObjectId};
pub use object::{AreaType, Color, LinearKind, Object, ObjectKind, StructureKind, Style};
pub use plant::{PlantCategory, PlantDef, PlantSymbol};
