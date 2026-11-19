//! Choosing how a new design starts: blank or one of the starter gardens.

use garden_editor::render::scene;
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use garden_editor::templates::{Template, TemplateId, instantiate, size_label, templates};
use garden_editor::tools::ToolKind;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::icons::{Icon, icon};
use crate::library::{DesignsOpen, Library, new_design_from};
use crate::plan_art::item_view;

/// Whether the "Start a new design" chooser is open.
#[derive(Clone, Copy)]
pub struct ChooserOpen(pub RwSignal<bool>);

/// Thumbnail width in screen pixels (for line weights).
const THUMB_PX: f64 = 220.0;

/// A small drawing of the template's plan.
fn thumbnail(t: &Template) -> impl IntoView + use<> {
    let doc = instantiate(t.id, t.name, 0);
    let (w, h) = t.size_mm;
    let margin = 0.05 * w.max(h);
    let mm_per_px = (w + 2.0 * margin) / THUMB_PX;
    let items = scene(&doc, &[])
        .into_iter()
        .map(|it| item_view(it, mm_per_px))
        .collect_view();
    view! {
        <svg class="template-thumb plan-thumb" aria-hidden="true" preserveAspectRatio="xMidYMid meet"
            viewBox=format!("{} {} {} {}", -margin, -margin, w + 2.0 * margin, h + 2.0 * margin)>
            {items}
        </svg>
    }
}

/// The chooser: Blank, then each starter garden.
#[component]
pub fn TemplateChooser() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let lib = expect_context::<Library>();
    let ChooserOpen(open) = expect_context::<ChooserOpen>();
    let DesignsOpen(designs_open) = expect_context::<DesignsOpen>();
    let busy = RwSignal::new(false);
    let choose = move |id: Option<TemplateId>| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            if new_design_from(lib, state, id).await {
                open.set(false);
                designs_open.set(false);
            }
            busy.set(false);
        });
    };
    let message = lib.message;
    // keyboard focus starts on the first choice, not on the page behind
    Effect::new(move |_| {
        if open.get() {
            request_animation_frame(|| {
                let first = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.query_selector(".chooser .template-tile").ok().flatten())
                    .and_then(|e| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(e).ok());
                if let Some(el) = first {
                    let _ = el.focus();
                }
            });
        }
    });
    // rebuilt each time the chooser opens (cheap; keeps the units current)
    let tiles = move || {
        let units = state.with_untracked(|s| s.doc().units);
        let blank = view! {
            <button class="template-tile" on:click=move |_| choose(None)>
                <div class="template-thumb blank" aria-hidden="true"></div>
                <strong>{EN.tpl_blank}</strong>
                <span>{EN.tpl_blank_blurb}</span>
            </button>
        };
        let starters = templates()
            .into_iter()
            .map(|t| {
                let id = t.id;
                view! {
                    <button class="template-tile" on:click=move |_| choose(Some(id))>
                        {thumbnail(&t)}
                        <strong>{t.name}</strong>
                        <span>{t.blurb}</span>
                        <em>{size_label(t.size_mm, units)}</em>
                    </button>
                }
            })
            .collect_view();
        view! { {blank}{starters} }
    };
    view! {
        <Show when=move || open.get()>
            <div class="chooser-backdrop" on:click=move |_| open.set(false)></div>
            <section class="card chooser" role="dialog" aria-modal="true" aria-label=EN.start_new
                class:busy=move || busy.get()>
                <header class="trace-head">
                    <h2>{EN.start_new}</h2>
                    <button class="icon-btn" aria-label=EN.close on:click=move |_| open.set(false)>
                        {icon(Icon::Close)}
                    </button>
                </header>
                <div class="template-grid">{tiles}</div>
                {move || message.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
            </section>
        </Show>
    }
}

/// Shown over an empty plan: start from a template, or draw a plot.
#[component]
pub fn EmptyPlanPrompt() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ChooserOpen(open) = expect_context::<ChooserOpen>();
    let empty = Memo::new(move |_| {
        state.with(|s| {
            // not while a traced photo is in, which people draw over next
            s.doc().objects.is_empty()
                && s.doc().background.is_none()
                && matches!(s.tool(), ToolKind::Select)
                && !s.is_busy()
        })
    });
    view! {
        <Show when=move || empty.get() && !open.get()>
            <div class="empty-prompt">
                <div class="card empty-card">
                    <strong>{EN.empty_plan}</strong>
                    <button class="trace-primary" on:click=move |_| open.set(true)>{EN.start_from_template}</button>
                    <span>{EN.or_draw_plot}</span>
                </div>
            </div>
        </Show>
    }
}
