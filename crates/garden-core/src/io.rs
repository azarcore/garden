//! Save format: versioned JSON. Old versions always load (via migrations);
//! files from a newer version are rejected rather than misread.

use serde_json::Value;

use crate::model::Document;

/// Version of the JSON save format written by this build.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("not a valid garden file: {0}")]
    Parse(String),
    #[error("missing schema_version")]
    MissingVersion,
    #[error("file is from a newer version ({0})")]
    TooNew(u32),
    #[error("migration failed: {0}")]
    Migration(String),
}

/// Serialize a document as pretty-printed JSON.
pub fn to_json(doc: &Document) -> String {
    // Serializing plain data with string keys cannot fail.
    serde_json::to_string_pretty(doc).unwrap_or_default()
}

/// Load a document from any supported schema version.
pub fn from_json(s: &str) -> Result<Document, LoadError> {
    let value: Value = serde_json::from_str(s).map_err(|e| LoadError::Parse(e.to_string()))?;
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or(LoadError::MissingVersion)?;
    let version = u32::try_from(version).map_err(|_| LoadError::TooNew(u32::MAX))?;
    if version > SCHEMA_VERSION {
        return Err(LoadError::TooNew(version));
    }
    let mut value = migrate(value, version)?;
    value["schema_version"] = SCHEMA_VERSION.into();
    serde_json::from_value(value).map_err(|e| LoadError::Parse(e.to_string()))
}

/// One step per old version: `MIGRATIONS[n - 1]` upgrades version n to n + 1.
type Migration = fn(Value) -> Result<Value, LoadError>;
const MIGRATIONS: &[Migration] = &[];

fn migrate(mut value: Value, from: u32) -> Result<Value, LoadError> {
    if from == 0 {
        return Err(LoadError::Migration("unknown schema version 0".into()));
    }
    for step in MIGRATIONS.iter().skip(from as usize - 1) {
        value = step(value)?;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::geometry::{Point, Shape};
    use crate::model::{AreaType, Document, Object, ObjectKind};

    #[test]
    fn round_trip() {
        let mut d = Document::new("g", 5);
        let l = d.layers[0].id;
        let bed = Object::new(
            "bed",
            ObjectKind::Area {
                area_type: AreaType::Bed,
                shape: Shape::circle(Point::new(1., 2.), 3.),
            },
            l,
        );
        d.apply(
            Command::Insert {
                object: bed,
                index: 0,
            },
            6,
        )
        .unwrap();
        assert_eq!(from_json(&to_json(&d)).unwrap(), d);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(matches!(from_json("{nope"), Err(LoadError::Parse(_))));
        assert!(matches!(from_json("{}"), Err(LoadError::MissingVersion)));
        assert!(matches!(from_json("[1,2]"), Err(LoadError::MissingVersion)));
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0))).unwrap();
        v["schema_version"] = 99.into();
        assert!(matches!(
            from_json(&v.to_string()),
            Err(LoadError::TooNew(99))
        ));
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0))).unwrap();
        v["objects"] = serde_json::json!("not a list");
        assert!(matches!(
            from_json(&v.to_string()),
            Err(LoadError::Parse(_))
        ));
    }

    #[test]
    fn ignores_unknown_fields() {
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0))).unwrap();
        v["future_thing"] = serde_json::json!({"x": 1});
        assert!(from_json(&v.to_string()).is_ok());
    }
}
