//! Save format: versioned JSON. Old versions always load (via migrations);
//! files from a newer version are rejected rather than misread.

use serde_json::Value;

use crate::model::{Document, PlantDef};

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
pub fn to_json_with_images(doc: &Document, images: &[EmbeddedImage]) -> Result<String, SaveError> {
    to_json_with_extras(doc, images, &[])
}

/// The most custom plants read from one file.
const MAX_FILE_PLANTS: usize = 1000;

/// `to_json` plus top-level `"images"` and `"plants"` (custom plants the design
/// uses) arrays, each left out when empty. Older readers ignore the extra
/// fields, so the schema version stays the same.
pub fn to_json_with_extras(
    doc: &Document,
    images: &[EmbeddedImage],
    plants: &[PlantDef],
) -> Result<String, SaveError> {
    use base64::Engine as _;
    let text = to_json(doc)?;
    if images.is_empty() && plants.is_empty() {
        return Ok(text);
    }
    let mut value: Value =
        serde_json::from_str(&text).map_err(|e| SaveError::Serialize(e.to_string()))?;
    if !plants.is_empty() {
        value["plants"] =
            serde_json::to_value(plants).map_err(|e| SaveError::Serialize(e.to_string()))?;
    }
    if images.is_empty() {
        return serde_json::to_string_pretty(&value)
            .map_err(|e| SaveError::Serialize(e.to_string()));
    }
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

/// Custom plants carried by a file; malformed or unsafe entries are skipped
/// (sizes must be finite and above zero, ids must start with `custom-`).
pub fn extract_plants(text: &str) -> Vec<PlantDef> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = value.get("plants").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| serde_json::from_value::<PlantDef>(entry.clone()).ok())
        .filter(|p| {
            p.id.starts_with("custom-")
                && !p.name.trim().is_empty()
                && p.is_finite()
                && p.spread > 0.0
                && p.height > 0.0
        })
        .map(|p| PlantDef { custom: true, ..p })
        .take(MAX_FILE_PLANTS)
        .collect()
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
    fn old_apple_file_loads() {
        use crate::model::{PlantCategory, PlantSymbol};
        let mut d = Document::new("g", 5);
        let layer = d.layers[0].id;
        d.apply(
            Command::Insert {
                object: Object::new(
                    "Apple Tree",
                    ObjectKind::Plant {
                        position: Point::new(0., 0.),
                        spread: 4000.,
                        height: 4000.,
                        symbol: PlantSymbol::Round,
                        category: PlantCategory::Tree,
                        catalog_id: Some("apple".into()),
                    },
                    layer,
                ),
                index: 0,
            },
            6,
        )
        .unwrap();
        let json = to_json(&d).unwrap();
        assert!(json.contains("\"category\": \"tree\"") || json.contains("\"category\":\"tree\""));
        let back = from_json(&json).unwrap();
        assert!(matches!(
            back.objects[0].kind,
            ObjectKind::Plant {
                category: PlantCategory::Tree,
                ..
            }
        ));
        assert_eq!(to_json(&back).unwrap(), json);
    }

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

    fn custom(id: &str) -> crate::model::PlantDef {
        use crate::model::{PlantCategory, PlantDef, PlantSymbol};
        PlantDef {
            id: id.into(),
            name: "Blue Lupin".into(),
            category: PlantCategory::Perennial,
            spread: 450.,
            height: 900.,
            symbol: PlantSymbol::Spiky,
            custom: true,
            aliases: vec!["lupine".into()],
        }
    }

    #[test]
    fn plants_round_trip_and_are_left_out_when_none() {
        let d = Document::new("g", 0);
        let text = to_json_with_extras(&d, &[], &[custom("custom-1")]).unwrap();
        assert_eq!(extract_plants(&text), vec![custom("custom-1")]);
        assert!(from_json(&text).is_ok());
        assert!(
            !to_json_with_extras(&d, &[], &[])
                .unwrap()
                .contains("\"plants\"")
        );
    }

    #[test]
    fn bad_plant_entries_are_skipped() {
        let mut v: serde_json::Value =
            serde_json::from_str(&to_json(&Document::new("g", 0)).unwrap()).unwrap();
        let ok = serde_json::to_value(custom("custom-ok")).unwrap();
        let mut negative = ok.clone();
        negative["spread"] = (-1.0).into();
        negative["id"] = "custom-neg".into();
        let mut builtin = ok.clone();
        builtin["id"] = "lavender".into();
        let mut not_custom = ok.clone();
        not_custom["id"] = "custom-nc".into();
        not_custom["custom"] = false.into();
        v["plants"] = serde_json::json!([ok, negative, builtin, {"id": "custom-x"}, 7, not_custom]);
        let got = extract_plants(&v.to_string());
        assert_eq!(
            got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["custom-ok", "custom-nc"]
        );
        assert!(got.iter().all(|p| p.custom));
        assert!(extract_plants("{nope").is_empty());
        // a huge list is cut short
        let many: Vec<_> = (0..1500)
            .map(|i| serde_json::to_value(custom(&format!("custom-{i}"))).unwrap())
            .collect();
        v["plants"] = serde_json::Value::Array(many);
        assert_eq!(extract_plants(&v.to_string()).len(), 1000);
    }

    #[test]
    fn plant_defs_without_aliases_still_load() {
        let json = r#"{"id":"custom-a","name":"A","category":"shrub","spread":500.0,"height":500.0,"symbol":"round","custom":true}"#;
        let p: crate::model::PlantDef = serde_json::from_str(json).unwrap();
        assert!(p.aliases.is_empty());
        assert!(!serde_json::to_string(&p).unwrap().contains("aliases"));
    }
}
