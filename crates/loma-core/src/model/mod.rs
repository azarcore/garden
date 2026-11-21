//! The garden design document and everything placed in it.

mod background;
mod care;
mod document;
mod ids;
mod object;
mod plant;

pub use background::{BackgroundImage, CalibrationError};
pub use care::{
    Amount, AmountUnit, CareTask, Health, Job, LogEntry, Photo, PlantCare, Product, ProductKind,
    Repeat, Target,
};
pub use document::{Document, Layer, Tombstone};
pub use ids::{LayerId, LogId, ObjectId, ProductId, TaskId};
pub use object::{AreaType, Color, LinearKind, Object, ObjectKind, StructureKind, Style};
pub use plant::{PlantCategory, PlantDef, PlantSymbol};
