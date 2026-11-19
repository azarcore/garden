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
    /// Feedback shown in the My designs panel (import errors, refused switches).
    pub message: RwSignal<Option<&'static str>>,
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

/// Make sure the open design is safely stored before switching away from it.
/// Returns false (and says why) when switching would lose work.
pub async fn leave_current(lib: Library, state: RwSignal<EditorState>) -> bool {
    use garden_editor::strings::EN;
    let Some(store) = lib.store.get_value() else {
        lib.message.set(Some(EN.storage_needed));
        return false;
    };
    let (doc, count) = state.with_untracked(|s| (s.doc().clone(), s.change_count()));
    lib.autosave.update_value(|a| a.observe(count, now()));
    if !lib.autosave.with_value(Autosave::is_dirty) {
        return true; // already stored (or a blank design with nothing to lose)
    }
    lib.autosave.update_value(|a| a.started());
    let result = store.put(&doc).await;
    lib.autosave
        .update_value(|a| a.finished_at(count, result.is_ok(), now()));
    if result.is_err() {
        lib.status.set(SaveStatus::Unsaved);
        lib.message.set(Some(EN.switch_blocked));
        return false;
    }
    true
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
    if lib.status.get_untracked() != SaveStatus::Unsaved {
        lib.status.set(SaveStatus::Saving); // keep "will retry" steady while retrying
    }
    let doc = state.with_untracked(|s| s.doc().clone());
    spawn_local(async move {
        let result = store.put(&doc).await;
        let ok = result.is_ok();
        lib.autosave
            .update_value(|a| a.finished_at(count, ok, now()));
        lib.status.set(match result {
            Ok(()) => SaveStatus::Saved,
            Err(StorageError::Unavailable(_)) => SaveStatus::Unavailable,
            Err(_) => SaveStatus::Unsaved,
        });
        if ok {
            refresh_summaries(lib).await;
        }
    });
}

/// Open storage, reopen the most recently edited design and start autosaving.
pub fn provide_library(state: RwSignal<EditorState>) -> Library {
    let lib = Library {
        store: StoredValue::new_local(None),
        status: RwSignal::new(SaveStatus::Saved),
        summaries: RwSignal::new(Vec::new()),
        message: RwSignal::new(None),
        autosave: StoredValue::new_local(Autosave::new(0)),
    };
    provide_context(lib);
    provide_context(DesignsOpen(RwSignal::new(false)));
    provide_context(ImportMessage(lib.message));
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

/// Feedback from the last import (shown in the My designs panel).
#[derive(Clone, Copy)]
pub struct ImportMessage(pub RwSignal<Option<&'static str>>);

/// Whether the "My designs" panel is open.
#[derive(Clone, Copy)]
pub struct DesignsOpen(pub RwSignal<bool>);

fn current_id(state: RwSignal<EditorState>) -> String {
    state.with_untracked(|s| s.doc().id.to_string())
}

/// Open `doc` in the editor and start autosaving it from a clean slate.
fn show(lib: Library, state: RwSignal<EditorState>, doc: Document) {
    state.update(|s| s.open_document(doc));
    lib.reset_autosave(state.with_untracked(|s| s.change_count()));
}

/// Start a new, empty design (the current one is saved first; nothing
/// happens if it can't be).
/// Returns true when the new design is open.
pub async fn new_design(lib: Library, state: RwSignal<EditorState>) -> bool {
    if !leave_current(lib, state).await {
        return false;
    }
    create_new(lib, state).await;
    true
}

/// Create, store and open an empty design without touching the current one.
async fn create_new(lib: Library, state: RwSignal<EditorState>) {
    lib.message.set(None);
    refresh_summaries(lib).await; // names must include a just-saved rename
    let names: Vec<String> = lib
        .summaries
        .with_untracked(|s| s.iter().map(|d| d.name.clone()).collect());
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let doc = Document::new(garden_editor::library::next_design_name(&refs), now());
    if let Some(store) = lib.store.get_value() {
        let _ = store.put(&doc).await;
    }
    show(lib, state, doc);
    refresh_summaries(lib).await;
}

/// Switch to a stored design (the current one is saved first).
/// Returns true when the design is open.
pub async fn open_design(lib: Library, state: RwSignal<EditorState>, id: String) -> bool {
    if id == current_id(state) {
        return true;
    }
    if !leave_current(lib, state).await {
        return false;
    }
    lib.message.set(None);
    let Some(store) = lib.store.get_value() else {
        return false;
    };
    let mut opened = false;
    if let Ok(Some(record)) = store.get(&id).await
        && let Ok(doc) = parse(&record)
    {
        show(lib, state, doc);
        opened = true;
    }
    refresh_summaries(lib).await;
    opened
}

/// Delete a stored design. Deleting the open one switches to the newest other
/// design, or a new one.
pub async fn delete_design(lib: Library, state: RwSignal<EditorState>, id: String) {
    let Some(store) = lib.store.get_value() else {
        return;
    };
    let deleting_open = id == current_id(state);
    if deleting_open {
        // the open design is going away: stop autosave from writing it back
        lib.reset_autosave(state.with_untracked(|s| s.change_count()));
    }
    let _ = store.delete(&id).await;
    if deleting_open {
        let next = match store.list().await {
            Ok(records) => records.iter().find_map(|r| parse(r).ok()),
            Err(_) => None,
        };
        match next {
            Some(doc) => show(lib, state, doc),
            None => {
                create_new(lib, state).await;
                return;
            }
        }
    }
    refresh_summaries(lib).await;
}

/// Import a `.garden.json` chosen by the person; never overwrites a stored design.
pub async fn import_file(
    lib: Library,
    state: RwSignal<EditorState>,
    message: RwSignal<Option<&'static str>>,
) {
    let Some(text) = crate::files::pick_file(".json,.garden.json,application/json").await else {
        return;
    };
    import_text(lib, state, &text, message).await;
}

/// Import a design from file contents (picked or dropped on the plan).
pub async fn import_text(
    lib: Library,
    state: RwSignal<EditorState>,
    text: &str,
    message: RwSignal<Option<&'static str>>,
) {
    use garden_editor::strings::EN;
    let doc = match io::from_json(text) {
        Ok(doc) => doc,
        Err(io::LoadError::TooNew(_)) => {
            message.set(Some(EN.too_new));
            return;
        }
        Err(_) => {
            message.set(Some(EN.import_failed));
            return;
        }
    };
    message.set(None);
    if !leave_current(lib, state).await {
        return;
    }
    let ids: Vec<uuid::Uuid> = lib.summaries.with_untracked(|s| {
        s.iter()
            .filter_map(|d| uuid::Uuid::parse_str(&d.id).ok())
            .collect()
    });
    let doc = garden_editor::library::prepare_import(doc, &ids, now());
    if let Some(store) = lib.store.get_value()
        && store.put(&doc).await.is_err()
    {
        message.set(Some(EN.switch_blocked));
        return;
    }
    show(lib, state, doc);
    refresh_summaries(lib).await;
}

/// Download the open design as a `.garden.json` file.
pub fn download_design(state: RwSignal<EditorState>) {
    let doc = state.with_untracked(|s| s.doc().clone());
    if let Ok(json) = io::to_json(&doc) {
        let name = garden_editor::library::export_file_name(&doc.name, "garden.json");
        crate::files::download(
            &name,
            "application/json",
            crate::files::Download::Text(json),
        );
    }
}

/// Export the plan as SVG, or PNG (2400 px wide) when `png` is set.
pub async fn export_image(state: RwSignal<EditorState>, png: bool) {
    let (name, bounds) = state.with_untracked(|s| {
        let b = s
            .doc()
            .objects
            .iter()
            .map(|o| garden_editor::objects::object_bounds(&o.kind))
            .reduce(|a, b| a.union(b));
        (s.doc().name.clone(), b)
    });
    let Some(bounds) = bounds else { return };
    let framed = bounds.inflate(1000.0, 1000.0);
    let Some((svg, w, h)) = crate::files::plan_svg(framed) else {
        return;
    };
    if png {
        if let Some(blob) = crate::files::svg_to_png(&svg, 2400.0, h / w).await {
            let file = garden_editor::library::export_file_name(&name, "png");
            crate::files::download(&file, "image/png", crate::files::Download::Blob(blob));
        }
    } else {
        let file = garden_editor::library::export_file_name(&name, "svg");
        crate::files::download(&file, "image/svg+xml", crate::files::Download::Text(svg));
    }
}

#[component]
fn DesignRow(summary: DesignSummary) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let lib = expect_context::<Library>();
    let DesignsOpen(open) = expect_context::<DesignsOpen>();
    let confirming = RwSignal::new(false);
    let id = summary.id.clone();
    let is_current = {
        let id = id.clone();
        Memo::new(move |_| state.with(|s| s.doc().id.to_string() == id))
    };
    let can_open = summary.error.is_none();
    let detail = match (&summary.error, &summary.size) {
        (Some(_), _) => garden_editor::strings::EN.cant_open.to_string(),
        (None, Some(size)) => size.clone(),
        (None, None) => garden_editor::strings::EN.no_plot_yet.to_string(),
    };
    let when = garden_editor::library::relative_time(now(), summary.modified);
    let open_id = id.clone();
    let delete_id = id.clone();
    view! {
        <li class="design-row" class:current=move || is_current.get() class:broken=!can_open>
            <button class="design-open" disabled=!can_open aria-current=move || is_current.get().then_some("true")
                on:click=move |_| {
                    let id = open_id.clone();
                    spawn_local(async move {
                        if open_design(lib, state, id).await {
                            open.set(false); // stays open to show why if it didn't switch
                        }
                    });
                }>
                <span class="design-name">{summary.name.clone()}</span>
                <span class="design-meta">{detail}</span>
                <span class="design-when">{when}</span>
            </button>
            <Show when=move || !confirming.get() fallback=move || {
                let id = delete_id.clone();
                view! {
                    <div class="confirm">
                        <span>{garden_editor::strings::EN.delete_confirm}</span>
                        <button class="danger" on:click=move |_| {
                            let id = id.clone();
                            spawn_local(async move { delete_design(lib, state, id).await });
                        }>{garden_editor::strings::EN.delete_design}</button>
                        <button on:click=move |_| confirming.set(false)>{garden_editor::strings::EN.keep}</button>
                    </div>
                }
            }>
                <button class="icon-btn design-delete" aria-label=garden_editor::strings::EN.delete_design
                    data-tip=garden_editor::strings::EN.delete_design on:click=move |_| confirming.set(true)>
                    {crate::icons::icon(crate::icons::Icon::Delete)}
                </button>
            </Show>
        </li>
    }
}

/// The "My designs" card, opened from the title card.
#[component]
pub fn MyDesigns() -> impl IntoView {
    use garden_editor::strings::EN;
    let state = expect_context::<RwSignal<EditorState>>();
    let lib = expect_context::<Library>();
    let DesignsOpen(open) = expect_context::<DesignsOpen>();
    let name = Memo::new(move |_| state.with(|s| s.doc().name.clone()));
    let ImportMessage(message) = expect_context::<ImportMessage>();
    let close_on_escape = window_event_listener(leptos::ev::keydown, move |e| {
        if e.key() == "Escape" && open.get_untracked() {
            open.set(false);
        }
    });
    on_cleanup(move || close_on_escape.remove());
    let rows = move || {
        lib.summaries
            .get()
            .into_iter()
            .map(|summary| view! { <DesignRow summary=summary /> })
            .collect_view()
    };
    view! {
        <Show when=move || open.get()>
            <section class="card designs" aria-label=EN.my_designs>
                <div class="field design-title">
                    <label for="design-name">{EN.rename}</label>
                    <input id="design-name" type="text" prop:value=move || name.get()
                        on:change=move |e| {
                            let value = event_target_value(&e);
                            state.update(|s| s.rename(&value, now()));
                        } />
                </div>
                <button class="new-design" on:click=move |_| {
                    spawn_local(async move {
                        if new_design(lib, state).await {
                            open.set(false);
                        }
                    });
                }>{crate::icons::icon(crate::icons::Icon::Leaf)}{EN.new_design}</button>
                <h3>{EN.my_designs}</h3>
                <ul class="design-list">{rows}</ul>
                <div class="file-actions">
                    <button on:click=move |_| spawn_local(async move { import_file(lib, state, message).await })>
                        {EN.import_file}
                    </button>
                    <button on:click=move |_| download_design(state)>{EN.export_file}</button>
                    <button on:click=move |_| spawn_local(async move { export_image(state, false).await })>
                        {EN.export_svg}
                    </button>
                    <button on:click=move |_| spawn_local(async move { export_image(state, true).await })>
                        {EN.export_png}
                    </button>
                </div>
                {move || message.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
            </section>
        </Show>
    }
}
