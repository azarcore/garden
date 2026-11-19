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

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SaveError {
    #[error("the design contains an invalid (non-finite) number")]
    NonFinite,
    #[error("could not serialize: {0}")]
    Serialize(String),
}

/// Serialize a document as pretty-printed JSON.
pub fn to_json(doc: &Document) -> Result<String, SaveError> {
    if !doc.is_finite() {
        return Err(SaveError::NonFinite);
    }
    serde_json::to_string_pretty(doc).map_err(|e| SaveError::Serialize(e.to_string()))
}

/// An image carried inside a `.garden.json` file (e.g. the traced photo).
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedImage {
    pub image_ref: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// `to_json` plus a top-level `"images"` array (left out when there are none).
/// Older readers ignore the extra field, so the schema version stays the same.
pub fn to_json_with_images(doc: &Document, images: &[EmbeddedImage]) -> Result<String, SaveError> {
    use base64::Engine as _;
    let text = to_json(doc)?;
    if images.is_empty() {
        return Ok(text);
    }
    let mut value: Value =
        serde_json::from_str(&text).map_err(|e| SaveError::Serialize(e.to_string()))?;
    value["images"] = images
        .iter()
        .map(|i| {
            serde_json::json!({
                "ref": i.image_ref,
                "mime": i.mime,
                "data": base64::engine::general_purpose::STANDARD.encode(&i.bytes),
            })
        })
        .collect();
    serde_json::to_string_pretty(&value).map_err(|e| SaveError::Serialize(e.to_string()))
}

/// Images carried by a file; malformed entries (or files) give none.
pub fn extract_images(text: &str) -> Vec<EmbeddedImage> {
    use base64::Engine as _;
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = value.get("images").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let image_ref = entry.get("ref")?.as_str()?.to_string();
            let mime = entry.get("mime")?.as_str()?.to_string();
            let data = entry.get("data")?.as_str()?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .ok()?;
            Some(EmbeddedImage {
                image_ref,
                mime,
                bytes,
            })
        })
        .collect()
}

/// Load a document from any supported schema version, repairing missing
/// layers (see [`Document::repair`]).
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
    let mut doc: Document =
        serde_json::from_value(value).map_err(|e| LoadError::Parse(e.to_string()))?;
    doc.repair();
    Ok(doc)
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
        assert_eq!(from_json(&to_json(&d).unwrap()).unwrap(), d);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(matches!(from_json("{nope"), Err(LoadError::Parse(_))));
        assert!(matches!(from_json("{}"), Err(LoadError::MissingVersion)));
        assert!(matches!(from_json("[1,2]"), Err(LoadError::MissingVersion)));
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0)).unwrap()).unwrap();
        v["schema_version"] = 99.into();
        assert!(matches!(
            from_json(&v.to_string()),
            Err(LoadError::TooNew(99))
        ));
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0)).unwrap()).unwrap();
        v["objects"] = serde_json::json!("not a list");
        assert!(matches!(
            from_json(&v.to_string()),
            Err(LoadError::Parse(_))
        ));
    }

    #[test]
    fn ignores_unknown_fields() {
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0)).unwrap()).unwrap();
        v["future_thing"] = serde_json::json!({"x": 1});
        assert!(from_json(&v.to_string()).is_ok());
    }

    #[test]
    fn images_round_trip_and_old_readers_ignore_them() {
        let d = Document::new("g", 0);
        let img = EmbeddedImage {
            image_ref: "bg-1".into(),
            mime: "image/png".into(),
            bytes: vec![137, 80, 78, 71, 0, 255],
        };
        let text = to_json_with_images(&d, std::slice::from_ref(&img)).unwrap();
        assert_eq!(extract_images(&text), vec![img]);
        assert_eq!(from_json(&text).unwrap(), d); // documents still load as before
    }

    #[test]
    fn no_images_means_no_images_field() {
        let text = to_json_with_images(&Document::new("g", 0), &[]).unwrap();
        assert!(!text.contains("\"images\""));
        assert!(extract_images(&text).is_empty());
    }

    #[test]
    fn malformed_images_are_skipped() {
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0)).unwrap()).unwrap();
        v["images"] = serde_json::json!([{"ref": "a", "mime": "image/png", "data": "!!!not base64"}, {"ref": "b"}, 7]);
        assert!(extract_images(&v.to_string()).is_empty());
        assert!(extract_images("{nope").is_empty());
    }
}
