//! Designs kept in the browser's IndexedDB.
//! One record per design: `{ id, name, modified, json }`, where `json` is exactly
//! what a `.garden.json` file contains (so loading always goes through migrations).

use garden_core::io;
use garden_core::model::Document;
use idb::{Database, DatabaseEvent, Factory, KeyPath, ObjectStoreParams, TransactionMode};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

const DB_NAME: &str = "garden-planner";
/// 1: designs. 2: adds the images store (traced photos).
const DB_VERSION: u32 = 2;
const STORE: &str = "designs";
const IMAGES: &str = "images";

#[derive(Debug, Clone, PartialEq)]
pub enum StorageError {
    /// Storage can't be used here (private window, blocked site data, …).
    Unavailable(String),
    /// The browser refused because storage is full.
    Quota,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDesign {
    pub id: String,
    pub name: String,
    pub modified: i64,
    pub json: String,
}

#[derive(Clone)]
pub struct Store {
    db: std::rc::Rc<Database>,
}

fn classify(e: idb::Error) -> StorageError {
    let text = e.to_string();
    if text.contains("QuotaExceeded") {
        StorageError::Quota
    } else {
        StorageError::Other(text)
    }
}

/// Open (and on first use create) the designs database.
pub async fn open() -> Result<Store, StorageError> {
    let factory = Factory::new().map_err(|e| StorageError::Unavailable(e.to_string()))?;
    let mut request = factory
        .open(DB_NAME, Some(DB_VERSION))
        .map_err(|e| StorageError::Unavailable(e.to_string()))?;
    request.on_upgrade_needed(|event| {
        let Ok(db) = event.database() else { return };
        let names = db.store_names();
        if !names.iter().any(|n| n == STORE) {
            let mut params = ObjectStoreParams::new();
            params.key_path(Some(KeyPath::new_single("id")));
            let _ = db.create_object_store(STORE, params);
        }
        if !names.iter().any(|n| n == IMAGES) {
            // keys are given explicitly (the image_ref)
            let _ = db.create_object_store(IMAGES, ObjectStoreParams::new());
        }
    });
    let db = request
        .await
        .map_err(|e| StorageError::Unavailable(e.to_string()))?;
    Ok(Store {
        db: std::rc::Rc::new(db),
    })
}

impl Store {
    /// Save (insert or replace) a design.
    pub async fn put(&self, doc: &Document) -> Result<(), StorageError> {
        let json = io::to_json(doc).map_err(|e| StorageError::Other(e.to_string()))?;
        let record = StoredDesign {
            id: doc.id.to_string(),
            name: doc.name.clone(),
            modified: doc.modified,
            json,
        };
        let value = serde_wasm_bindgen::to_value(&record)
            .map_err(|e| StorageError::Other(e.to_string()))?;
        let tx = self
            .db
            .transaction(&[STORE], TransactionMode::ReadWrite)
            .map_err(classify)?;
        let store = tx.object_store(STORE).map_err(classify)?;
        store
            .put(&value, None)
            .map_err(classify)?
            .await
            .map_err(classify)?;
        tx.commit().map_err(classify)?.await.map_err(classify)?;
        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<StoredDesign>, StorageError> {
        let tx = self
            .db
            .transaction(&[STORE], TransactionMode::ReadOnly)
            .map_err(classify)?;
        let store = tx.object_store(STORE).map_err(classify)?;
        let value: Option<JsValue> = store
            .get(JsValue::from_str(id))
            .map_err(classify)?
            .await
            .map_err(classify)?;
        Ok(value.and_then(|v| serde_wasm_bindgen::from_value(v).ok()))
    }

    /// Every stored design, most recently modified first. Unreadable records are skipped.
    pub async fn list(&self) -> Result<Vec<StoredDesign>, StorageError> {
        let tx = self
            .db
            .transaction(&[STORE], TransactionMode::ReadOnly)
            .map_err(classify)?;
        let store = tx.object_store(STORE).map_err(classify)?;
        let values: Vec<JsValue> = store
            .get_all(None, None)
            .map_err(classify)?
            .await
            .map_err(classify)?;
        let mut designs: Vec<StoredDesign> = values
            .into_iter()
            .filter_map(|v| serde_wasm_bindgen::from_value(v).ok())
            .collect();
        designs.sort_by_key(|d| std::cmp::Reverse(d.modified));
        Ok(designs)
    }

    pub async fn delete(&self, id: &str) -> Result<(), StorageError> {
        let tx = self
            .db
            .transaction(&[STORE], TransactionMode::ReadWrite)
            .map_err(classify)?;
        let store = tx.object_store(STORE).map_err(classify)?;
        store
            .delete(JsValue::from_str(id))
            .map_err(classify)?
            .await
            .map_err(classify)?;
        tx.commit().map_err(classify)?.await.map_err(classify)?;
        Ok(())
    }

    /// Store the bytes of a traced photo under its reference.
    pub async fn put_image(
        &self,
        image_ref: &str,
        blob: &web_sys::Blob,
    ) -> Result<(), StorageError> {
        let tx = self
            .db
            .transaction(&[IMAGES], TransactionMode::ReadWrite)
            .map_err(classify)?;
        let store = tx.object_store(IMAGES).map_err(classify)?;
        store
            .put(blob, Some(&JsValue::from_str(image_ref)))
            .map_err(classify)?
            .await
            .map_err(classify)?;
        tx.commit().map_err(classify)?.await.map_err(classify)?;
        Ok(())
    }

    pub async fn get_image(&self, image_ref: &str) -> Result<Option<web_sys::Blob>, StorageError> {
        use wasm_bindgen::JsCast;
        let tx = self
            .db
            .transaction(&[IMAGES], TransactionMode::ReadOnly)
            .map_err(classify)?;
        let store = tx.object_store(IMAGES).map_err(classify)?;
        let value: Option<JsValue> = store
            .get(JsValue::from_str(image_ref))
            .map_err(classify)?
            .await
            .map_err(classify)?;
        Ok(value.and_then(|v| v.dyn_into::<web_sys::Blob>().ok()))
    }

    #[allow(dead_code)] // kept for a later clean-up of unused photos
    pub async fn delete_image(&self, image_ref: &str) -> Result<(), StorageError> {
        let tx = self
            .db
            .transaction(&[IMAGES], TransactionMode::ReadWrite)
            .map_err(classify)?;
        let store = tx.object_store(IMAGES).map_err(classify)?;
        store
            .delete(JsValue::from_str(image_ref))
            .map_err(classify)?
            .await
            .map_err(classify)?;
        tx.commit().map_err(classify)?.await.map_err(classify)?;
        Ok(())
    }
}
