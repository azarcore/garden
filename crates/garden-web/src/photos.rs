//! Plant photos: scaled down, kept in the browser's image store.

use garden_core::model::ObjectId;
use garden_editor::state::EditorState;
use leptos::prelude::*;

/// The photos section of a plant card.
pub fn photo_strip(state: RwSignal<EditorState>, id: ObjectId) -> impl IntoView {
    let _ = (state, id);
}
