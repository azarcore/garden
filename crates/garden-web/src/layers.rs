//! The layers card: which layer new objects go on, add, rename, show/hide,
//! lock and delete layers.

use garden_core::command::CommandError;
use garden_core::model::Layer;
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use leptos::prelude::*;

use crate::app::now;
use crate::icons::{Icon, icon};

/// Whether the layers card is open.
#[derive(Clone, Copy)]
pub struct LayersOpen(pub RwSignal<bool>);

fn error_message(e: CommandError) -> &'static str {
    match e {
        CommandError::LastLayer => EN.last_layer,
        _ => EN.layer_in_use,
    }
}

/// One layer: make it active, rename, show/hide, lock/unlock, delete.
fn layer_row(layer: Layer, active: bool, message: RwSignal<Option<&'static str>>) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let id = layer.id;
    let rename = {
        let layer = layer.clone();
        move |name: String| {
            let next = Layer {
                name,
                ..layer.clone()
            };
            state.update(|s| s.update_layer(next, now()));
        }
    };
    let toggle_visible = {
        let layer = layer.clone();
        move |_| {
            let next = Layer {
                visible: !layer.visible,
                ..layer.clone()
            };
            state.update(|s| s.update_layer(next, now()));
        }
    };
    let toggle_lock = {
        let layer = layer.clone();
        move |_| {
            let next = Layer {
                locked: !layer.locked,
                ..layer.clone()
            };
            state.update(|s| s.update_layer(next, now()));
        }
    };
    let delete = move |_| {
        let mut result = Ok(());
        state.update(|s| result = s.delete_layer(id, now()));
        message.set(result.err().map(error_message));
    };
    let (show_label, show_icon) = if layer.visible {
        (EN.hide, Icon::Eye)
    } else {
        (EN.show, Icon::EyeOff)
    };
    let (lock_label, lock_icon) = if layer.locked {
        (EN.unlock, Icon::Lock)
    } else {
        (EN.lock, Icon::Unlock)
    };
    let name = layer.name.clone();
    view! {
        <li class="layer-row" class:active=active class:hidden-layer=!layer.visible>
            <button class="layer-active" aria-pressed=active.to_string() aria-label=EN.active_layer data-tip=EN.active_layer
                on:click=move |_| state.update(|s| s.set_active_layer(id))></button>
            <input type="text" class="layer-name" aria-label=EN.layers prop:value=name
                on:keydown=move |e: leptos::ev::KeyboardEvent| if e.key() == "Enter" {
                    let _ = event_target::<web_sys::HtmlInputElement>(&e).blur();
                }
                on:change=move |e| rename(event_target_value(&e)) />
            <button class="icon-btn toggle" aria-pressed=(!layer.visible).to_string() aria-label=show_label data-tip=show_label
                on:click=toggle_visible>{icon(show_icon)}</button>
            <button class="icon-btn toggle" aria-pressed=layer.locked.to_string() aria-label=lock_label data-tip=lock_label
                on:click=toggle_lock>{icon(lock_icon)}</button>
            <button class="icon-btn" aria-label=EN.delete_layer data-tip=EN.delete_layer on:click=delete>{icon(Icon::Delete)}</button>
        </li>
    }
}

#[component]
pub fn LayersCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let LayersOpen(open) = expect_context::<LayersOpen>();
    let message = RwSignal::new(None::<&'static str>);
    let layers = Memo::new(move |_| state.with(|s| (s.doc().layers.clone(), s.active_layer_id())));
    let rows = move || {
        let (all, active) = layers.get();
        // top layer first, as people expect from drawing apps
        all.into_iter()
            .rev()
            .map(|l| {
                let is_active = l.id == active;
                layer_row(l, is_active, message)
            })
            .collect_view()
    };
    view! {
        <Show when=move || open.get()>
            <section class="card layers-card" aria-label=EN.layers>
                <header class="trace-head">
                    <h2>{EN.layers}</h2>
                    <button class="icon-btn" aria-label=EN.close on:click=move |_| open.set(false)>
                        {icon(Icon::Close)}
                    </button>
                </header>
                <ul class="layer-list">{rows}</ul>
                {move || message.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
                <button class="trace-primary" on:click=move |_| {
                    message.set(None);
                    state.update(|s| { s.add_layer(now()); });
                }>{icon(Icon::Plus)}{EN.add_layer}</button>
            </section>
        </Show>
    }
}
