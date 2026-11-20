use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identity of an object, kept across edits, saves and (later) sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub Uuid);

impl ObjectId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        ObjectId(Uuid::new_v4())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub Uuid);

impl LayerId {
    /// Stands in for "no layer"; never the id of a real layer.
    pub const NIL: LayerId = LayerId(Uuid::nil());

    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        LayerId(Uuid::new_v4())
    }
}

/// Identity of a product in the products library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProductId(pub Uuid);

impl ProductId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        ProductId(Uuid::new_v4())
    }
}

/// Identity of a log entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LogId(pub Uuid);

impl LogId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        LogId(Uuid::new_v4())
    }
}
