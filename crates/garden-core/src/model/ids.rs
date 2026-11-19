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
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        LayerId(Uuid::new_v4())
    }
}
