//! Looking after the garden: each plant's health, notes and photos, the log of
//! jobs done, and the products library with label intervals typed in by the person.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{LogId, ObjectId, PlantCategory, ProductId, TaskId};
use crate::date::Date;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    #[default]
    Healthy,
    Watch,
    Sick,
    Gone,
}

/// A photo of a plant. The image itself lives in browser storage under `image_ref`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub image_ref: String,
    pub date: Date,
    pub added_ms: i64,
}

/// What is known about one plant (or hedge) apart from its log.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlantCare {
    #[serde(default)]
    pub health: Health,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub photos: Vec<Photo>,
}

impl PlantCare {
    /// Healthy, with no notes and no photos: nothing worth saving.
    pub fn is_empty(&self) -> bool {
        self.health == Health::Healthy && self.notes.is_empty() && self.photos.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductKind {
    Fungicide,
    Insecticide,
    Herbicide,
    Fertiliser,
    Other,
}

/// A product from the person's library. The intervals are copied from its label;
/// the app never fills them in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Product {
    pub id: ProductId,
    pub name: String,
    pub kind: ProductKind,
    /// Days to wait after applying before harvesting.
    pub harvest_days: Option<u32>,
    /// Least number of days between two applications (≥ 1).
    pub reapply_days: Option<u32>,
    /// Most applications in a calendar year (≥ 1).
    pub max_per_season: Option<u32>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Job {
    Sprayed,
    Fed,
    Pruned,
    Watered,
    Harvested,
    Planted,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmountUnit {
    Kg,
    G,
    Lb,
    Pieces,
    L,
    Ml,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Amount {
    pub value: f64,
    pub unit: AmountUnit,
}

/// One job done on one or more plants.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: LogId,
    pub date: Date,
    pub job: Job,
    pub plants: Vec<ObjectId>,
    /// The plants' names when logged, so the entry still reads well after a
    /// plant is renamed or removed. Same length as `plants`.
    pub plant_names: Vec<String>,
    pub product: Option<ProductId>,
    /// The product's name when logged.
    #[serde(default)]
    pub product_name: String,
    pub amount: Option<Amount>,
    #[serde(default)]
    pub note: String,
    pub created_ms: i64,
}

/// What a repeating job covers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    /// These plants or hedges.
    Plants { ids: Vec<ObjectId> },
    /// Every plant placed from this catalog entry, including ones added later.
    Kind { catalog_id: String, name: String },
    /// Every plant of this category, including ones added later.
    Category { category: PlantCategory },
}

/// How a repeating job repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Repeat {
    /// Every so many days after it was last done.
    Every { days: u32 },
    /// Once in each season (the months), or every 365 days without months.
    Yearly,
}

/// A job that repeats, set up by the person.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CareTask {
    pub id: TaskId,
    pub job: Job,
    pub product: Option<ProductId>,
    #[serde(default)]
    pub product_name: String,
    #[serde(default)]
    pub note: String,
    pub target: Target,
    pub repeat: Repeat,
    /// (from, to) months 1..=12; from > to wraps past the year end.
    pub months: Option<(u32, u32)>,
    /// When it is first due for a plant that has never had it done.
    pub first_due: Date,
    /// The last skipped due date, per plant.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skips: BTreeMap<ObjectId, Date>,
}
