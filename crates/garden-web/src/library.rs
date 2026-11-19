//! Keeping designs: autosave to IndexedDB, restore on start, and the
//! "My designs" list.

use std::time::Duration;

use garden_core::io;
use garden_core::model::Document;
use garden_editor::library::{Autosave, DesignSummary, summarize, summary_for_error};
use garden_editor::state::EditorState;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::app::now;
use crate::storage::{self, StorageError, Store, StoredDesign};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveStatus {
    Saved,
    Saving,
    /// The last save failed; it will be retried on the next change.
    Unsaved,
    /// Browser storage can't be used here.
    Unavailable,
}

#[derive(Clone, Copy)]
pub struct Library {
    pub store: StoredValue<Option<Store>, LocalStorage>,
    pub status: RwSignal<SaveStatus>,
    pub summaries: RwSignal<Vec<DesignSummary>>,
    autosave: StoredValue<Autosave, LocalStorage>,
}

impl Library {
    /// Start counting changes from the editor's current state (after opening a design).
    pub fn reset_autosave(&self, change_count: u64) {
        self.autosave.set_value(Autosave::new(change_count));
        self.status.set(if self.store.with_value(Option::is_some) {
            SaveStatus::Saved
        } else {
            SaveStatus::Unavailable
        });
    }
}

/// Parse a stored record, or describe why it can't be opened.
pub fn parse(record: &StoredDesign) -> Result<Document, String> {
    io::from_json(&record.json).map_err(|e| e.to_string())
}

fn summaries_of(records: &[StoredDesign]) -> Vec<DesignSummary> {
    records
        .iter()
        .map(|r| match parse(r) {
            Ok(doc) => summarize(&doc),
            Err(e) => summary_for_error(&r.id, &r.name, r.modified, &e),
        })
        .collect()
}

/// Re-read the list of stored designs.
pub async fn refresh_summaries(lib: Library) {
    let Some(store) = lib.store.get_value() else {
        return;
    };
    if let Ok(records) = store.list().await {
        lib.summaries.set(summaries_of(&records));
    }
}

/// Save the open design now (used before switching designs).
#[allow(dead_code)] // used by the My designs panel (next task)
pub async fn save_now(lib: Library, state: RwSignal<EditorState>) -> Result<(), StorageError> {
    let Some(store) = lib.store.get_value() else {
        return Err(StorageError::Unavailable(String::new()));
    };
    let (doc, count) = state.with_untracked(|s| (s.doc().clone(), s.change_count()));
    lib.autosave.update_value(|a| a.started());
    let result = store.put(&doc).await;
    lib.autosave
        .update_value(|a| a.finished(count, result.is_ok()));
    result
}

fn tick(lib: Library, state: RwSignal<EditorState>) {
    let (count, busy) = state.with_untracked(|s| (s.change_count(), s.is_busy()));
    let t = now();
    lib.autosave.update_value(|a| a.observe(count, t));
    let Some(store) = lib.store.get_value() else {
        if lib.autosave.with_value(Autosave::is_dirty) {
            lib.status.set(SaveStatus::Unavailable);
        }
        return;
    };
    if !lib.autosave.with_value(|a| a.due(t, busy)) {
        return;
    }
    lib.autosave.update_value(|a| a.started());
    lib.status.set(SaveStatus::Saving);
    let doc = state.with_untracked(|s| s.doc().clone());
    spawn_local(async move {
        let result = store.put(&doc).await;
        lib.autosave
            .update_value(|a| a.finished(count, result.is_ok()));
        lib.status.set(match result {
            Ok(()) => SaveStatus::Saved,
            Err(StorageError::Unavailable(_)) => SaveStatus::Unavailable,
            Err(_) => SaveStatus::Unsaved,
        });
        refresh_summaries(lib).await;
    });
}

/// Open storage, reopen the most recently edited design and start autosaving.
pub fn provide_library(state: RwSignal<EditorState>) -> Library {
    let lib = Library {
        store: StoredValue::new_local(None),
        status: RwSignal::new(SaveStatus::Saved),
        summaries: RwSignal::new(Vec::new()),
        autosave: StoredValue::new_local(Autosave::new(0)),
    };
    provide_context(lib);
    spawn_local(async move {
        let store = match storage::open().await {
            Ok(store) => store,
            Err(_) => {
                lib.status.set(SaveStatus::Unavailable);
                return;
            }
        };
        lib.store.set_value(Some(store.clone()));
        let Ok(records) = store.list().await else {
            lib.status.set(SaveStatus::Unavailable);
            return;
        };
        lib.summaries.set(summaries_of(&records));
        // Only replace the blank start-up design if nothing has been drawn yet.
        let untouched = state.with_untracked(|s| s.change_count() == 0);
        if let Some(doc) = records.iter().find_map(|r| parse(r).ok())
            && untouched
        {
            state.update(|s| s.open_document(doc));
        }
        lib.reset_autosave(state.with_untracked(|s| s.change_count()));
    });
    let _ = set_interval_with_handle(move || tick(lib, state), Duration::from_millis(250));
    lib
}
