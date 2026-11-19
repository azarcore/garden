//! The SVG plan view: events, viewport, and the overlays (scale bar, readout).

use garden_core::geometry::Point;
use garden_core::units::{UnitSystem, format_length};
use garden_editor::render::scene;
use garden_editor::state::EditorState;
use garden_editor::tools::{Input, Modifiers, ToolKind};
use garden_editor::viewport::Viewport;
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use garden_editor::handles::{Corner, HandleKind};

use crate::app::{Cursor, SpaceHeld, now};
use crate::plan_art::{defs, item_view, preview_view};

/// Blur a focused inspector field so its edit is committed before a press on
/// the plan changes the selection (which replaces the inspector panel).
fn commit_focused_field() {
    let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return;
    };
    if matches!(el.tag_name().as_str(), "INPUT" | "SELECT" | "TEXTAREA") {
        let _ = el.blur();
    }
}

fn modifiers(e: &ev::MouseEvent) -> Modifiers {
    Modifiers {
        shift: e.shift_key(),
        alt: e.alt_key(),
        ctrl: e.ctrl_key() || e.meta_key(),
    }
}

/// A "nice" scale-bar length (mm) that is at most `max_px` wide on screen.
fn scale_bar_length(scale: f64, units: UnitSystem, max_px: f64) -> f64 {
    let steps: &[f64] = match units {
        UnitSystem::Metric => &[
            100., 200., 500., 1000., 2000., 5000., 10_000., 20_000., 50_000., 100_000.,
        ],
        UnitSystem::Imperial => &[
            304.8, 609.6, 1524., 3048., 6096., 15_240., 30_480., 60_960., 152_400.,
        ],
    };
    steps
        .iter()
        .rev()
        .copied()
        .find(|mm| mm * scale <= max_px)
        .unwrap_or(steps[0])
}

/// Map-style scale bar plus cursor readout, floating over the plan.
#[component]
fn PlanLegend() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let Cursor(cursor) = expect_context::<Cursor>();
    let view_info = Memo::new(move |_| state.with(|s| (s.viewport.scale, s.doc().units)));
    let bar = move || {
        let (scale, units) = view_info.get();
        let len = scale_bar_length(scale, units, 150.0);
        let px = len * scale;
        let seg = px / 4.0;
        let blocks = (0..4)
            .map(|i| {
                let class = if i % 2 == 0 { "bar-dark" } else { "bar-light" };
                view! { <rect x=i as f64 * seg y="0" width=seg height="6" class=class /> }
            })
            .collect_view();
        view! {
            <div class="scale-bar">
                <svg width=px + 2.0 height="8" viewBox=format!("-1 -1 {} 8", px + 2.0) aria-hidden="true">
                    {blocks}
                    <rect x="0" y="0" width=px height="6" class="bar-frame" />
                </svg>
                <div class="scale-labels" style=format!("width:{px}px")>
                    <span>"0"</span>
                    <span>{format_length(len, units)}</span>
                </div>
            </div>
        }
    };
    let readout = move || {
        let (scale, units) = view_info.get();
        let zoom = (scale / Viewport::DEFAULT_SCALE * 100.0).round();
        let pos = cursor
            .get()
            .map(|p| {
                format!(
                    "{}, {}",
                    format_length(p.x, units),
                    format_length(p.y, units)
                )
            })
            .unwrap_or_default();
        view! {
            <span class="pos">{pos}</span>
            <span class="zoom">{format!("{zoom}%")}</span>
        }
    };
    view! { <div class="card legend">{bar}<div class="readout">{readout}</div></div> }
}

/// Resize corners, the rotate handle, and in node mode the points and curve
/// controls of the selected object, drawn in screen-sized squares and dots.
fn handles_view(s: &EditorState) -> impl IntoView + use<> {
    let handles = s.handles();
    if handles.is_empty() || !s.preview().shapes.is_empty() {
        return None; // nothing to grab, or the shape is being dragged
    }
    let mm = s.viewport.px_to_mm(1.0);
    let shape = s
        .selection
        .first()
        .and_then(|id| s.doc().object(*id))
        .and_then(|o| garden_editor::objects::kind_shape(&o.kind).cloned());
    let selected = s.selected_vertex();
    let half = 4.5 * mm;
    let views = handles
        .into_iter()
        .map(|h| {
            let square = |class: String, cursor: &'static str| {
                view! {
                    <rect x=h.at.x - half y=h.at.y - half width=2.0 * half height=2.0 * half
                        class=class style=format!("cursor:{cursor}") />
                }
                .into_any()
            };
            match h.kind {
                HandleKind::Resize(c) => {
                    let cursor = match c {
                        Corner::NorthWest | Corner::SouthEast => "nwse-resize",
                        Corner::NorthEast | Corner::SouthWest => "nesw-resize",
                    };
                    square("handle".into(), cursor)
                }
                HandleKind::Rotate => {
                    let stem_end = h.at.y + 28.0 * mm;
                    view! {
                        <g class="handle-rotate" style="cursor:grab">
                            <line x1=h.at.x y1=h.at.y x2=h.at.x y2=stem_end class="handle-stem" />
                            <circle cx=h.at.x cy=h.at.y r=5.0 * mm class="handle" />
                        </g>
                    }
                    .into_any()
                }
                HandleKind::Vertex(i) => {
                    let class = if selected == Some(i) { "handle vertex selected" } else { "handle vertex" };
                    square(class.into(), "move")
                }
                HandleKind::Control { segment, second } => {
                    // the point this control belongs to
                    let anchor = shape.as_ref().map(|sh| match (second, segment) {
                        (true, k) => sh.segments[k].to(),
                        (false, 0) => sh.start,
                        (false, k) => sh.segments[k - 1].to(),
                    });
                    view! {
                        <g class="handle-control" style="cursor:move">
                            {anchor.map(|a| view! { <line x1=a.x y1=a.y x2=h.at.x y2=h.at.y class="handle-stem" /> })}
                            <circle cx=h.at.x cy=h.at.y r=4.0 * mm class="handle control" />
                        </g>
                    }
                    .into_any()
                }
            }
        })
        .collect_view();
    Some(view! { <g class="handles">{views}</g> })
}

#[component]
pub fn Canvas() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let Cursor(cursor) = expect_context::<Cursor>();
    let SpaceHeld(space) = expect_context::<SpaceHeld>();
    let svg_ref = NodeRef::<leptos::svg::Svg>::new();
    // Last screen position while panning.
    let pan_from = RwSignal::new(None::<Point>);

    // Keep the viewport in step with the element's size (window resizes,
    // the plant picker opening, ...).
    // Page position of the canvas's top-left corner at the last measurement.
    let last_origin = StoredValue::new(None::<Point>);
    let measure = move || {
        let Some(el) = svg_ref.get_untracked() else {
            return;
        };
        let r = el.get_bounding_client_rect();
        let origin = Point::new(r.left(), r.top());
        let changed = state
            .with_untracked(|s| s.viewport.width != r.width() || s.viewport.height != r.height());
        if changed {
            state.update(|s| {
                // Keep the drawing still on the page while the canvas moves/resizes.
                let old = last_origin.get_value().unwrap_or(origin);
                let anchor = s.viewport.screen_to_world(Point::ZERO);
                s.viewport.resize(r.width(), r.height());
                let now_at = s.viewport.screen_to_world((old - origin).to_point());
                s.viewport.center += anchor - now_at;
            });
        }
        last_origin.set_value(Some(origin));
    };
    let screen_point = move |client_x: i32, client_y: i32| -> Point {
        svg_ref
            .get_untracked()
            .map(|el| {
                let r = el.get_bounding_client_rect();
                Point::new(client_x as f64 - r.left(), client_y as f64 - r.top())
            })
            .unwrap_or(Point::ZERO)
    };
    // Watch the element itself so any layout change (window resize, the plant
    // picker opening or closing) updates the viewport.
    Effect::new(move |_| {
        let Some(el) = svg_ref.get() else {
            return;
        };
        let callback = Closure::<dyn Fn()>::new(measure);
        if let Ok(observer) = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) {
            observer.observe(&el);
            // Keep the observer alive for the lifetime of the canvas.
            StoredValue::new_local(observer);
        }
        callback.forget();
        measure();
        state.update(|s| s.fit_to_content());
    });

    let on_wheel = move |e: ev::WheelEvent| {
        e.prevent_default();
        let at = screen_point(e.client_x(), e.client_y());
        let factor = (-e.delta_y() * 0.0015).exp();
        state.update(|s| s.viewport.zoom_at(at, factor));
    };
    let on_down = move |e: ev::PointerEvent| {
        commit_focused_field();
        let at = screen_point(e.client_x(), e.client_y());
        if e.button() == 1 || (e.button() == 0 && space.get_untracked()) {
            e.prevent_default();
            if let Some(el) = svg_ref.get_untracked() {
                let _ = el.set_pointer_capture(e.pointer_id());
            }
            pan_from.set(Some(at));
            return;
        }
        if e.button() != 0 {
            return;
        }
        if let Some(el) = svg_ref.get_untracked() {
            let _ = el.set_pointer_capture(e.pointer_id());
        }
        let m = modifiers(&e);
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            s.handle(Input::Down(w, m), now());
        });
    };
    let on_move = move |e: ev::PointerEvent| {
        let at = screen_point(e.client_x(), e.client_y());
        if let Some(from) = pan_from.get_untracked() {
            state.update(|s| s.viewport.pan_by(at - from));
            pan_from.set(Some(at));
            return;
        }
        let m = modifiers(&e);
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            cursor.set(Some(w));
            s.handle(Input::Move(w, m), now());
        });
    };
    let on_up = move |e: ev::PointerEvent| {
        if pan_from.get_untracked().is_some() {
            pan_from.set(None);
            return;
        }
        if e.button() != 0 {
            return;
        }
        let at = screen_point(e.client_x(), e.client_y());
        let m = modifiers(&e);
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            s.handle(Input::Up(w, m), now());
        });
    };
    // The browser took the pointer away (gesture, alert, window switch):
    // stop panning and abandon whatever was being drawn.
    let on_cancel = move |_e: ev::PointerEvent| {
        pan_from.set(None);
        state.update(|s| {
            s.handle(
                Input::Key(garden_editor::tools::Key::Escape, Modifiers::default()),
                now(),
            )
        });
    };
    let on_lost_capture = move |_e: ev::PointerEvent| pan_from.set(None);
    let on_dblclick = move |e: ev::MouseEvent| {
        let at = screen_point(e.client_x(), e.client_y());
        let m = modifiers(&e);
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            s.handle(Input::DoubleClick(w, m), now());
        });
    };
    // Dropping a .garden.json file on the plan imports it.
    let lib = expect_context::<crate::library::Library>();
    let crate::library::ImportMessage(import_message) =
        expect_context::<crate::library::ImportMessage>();
    let crate::library::DesignsOpen(designs_open) = expect_context::<crate::library::DesignsOpen>();
    let cache = expect_context::<crate::trace::ImageCache>();
    let trace = expect_context::<crate::trace::Trace>();
    // load the traced photo whenever the design's background changes
    // (tracks only the image reference, not every editor change)
    let photo_ref = Memo::new(move |_| {
        state.with(|s| s.doc().background.as_ref().map(|b| b.image_ref.clone()))
    });
    Effect::new(move |_| {
        if let Some(image_ref) = photo_ref.get() {
            leptos::task::spawn_local(crate::trace::ensure_image(lib, cache, trace, image_ref));
        }
    });
    let photo = crate::trace::background_view(state, cache);
    let on_drag_over = move |e: ev::DragEvent| e.prevent_default();
    let on_drop = move |e: ev::DragEvent| {
        e.prevent_default();
        let Some(file) = e
            .data_transfer()
            .and_then(|d| d.files())
            .and_then(|f| f.get(0))
        else {
            return;
        };
        if file.type_().starts_with("image/") {
            let blob: web_sys::Blob = file.into();
            leptos::task::spawn_local(async move {
                crate::trace::add_photo(lib, state, cache, trace, blob).await;
            });
            return;
        }
        leptos::task::spawn_local(async move {
            let Ok(text) = wasm_bindgen_futures::JsFuture::from(file.text()).await else {
                return;
            };
            let Some(text) = text.as_string() else { return };
            crate::library::import_text(lib, state, &text, import_message).await;
            if import_message.get_untracked().is_some() {
                designs_open.set(true); // show why it failed
            }
        });
    };
    let drawing = Memo::new(move |_| state.with(|s| !matches!(s.tool(), ToolKind::Select)));

    // Dot grid at the snapping step (coarser when zoomed out).
    let grid = move || {
        state.with(|s| {
            let v = &s.viewport;
            let a = v.screen_to_world(Point::ZERO);
            let b = v.screen_to_world(Point::new(v.width, v.height));
            let step = [100.0, 500.0, 1000.0, 5000.0, 10_000.0, 50_000.0, 100_000.0]
                .into_iter()
                .find(|st| st * v.scale >= 16.0)
                .unwrap_or(100_000.0);
            let dot = v.px_to_mm(1.2);
            view! {
                <pattern id="dot-grid" width=step height=step patternUnits="userSpaceOnUse">
                    <circle cx="0" cy="0" r=dot class="grid-dot" />
                    <circle cx=step cy="0" r=dot class="grid-dot" />
                    <circle cx="0" cy=step r=dot class="grid-dot" />
                    <circle cx=step cy=step r=dot class="grid-dot" />
                </pattern>
                <rect x=a.x y=a.y width=b.x - a.x height=b.y - a.y fill="url(#dot-grid)" />
            }
        })
    };
    // Name tag floating above a single selected object.
    let tag = move || {
        state.with(|s| {
            let [id] = s.selection.as_slice() else { return None };
            let o = s.doc().object(*id)?;
            if !s.preview().shapes.is_empty() {
                return None; // hide while dragging
            }
            let mm = s.viewport.px_to_mm(1.0);
            let b = garden_editor::objects::object_bounds(&o.kind);
            // sit above the rotate handle when there is one
            let lift = if s.handles().iter().any(|h| h.kind == HandleKind::Rotate) { 30.0 } else { 0.0 };
            let detail = match &o.kind {
                garden_core::model::ObjectKind::Plant { spread, .. } => format!(" · {}", format_length(*spread, s.doc().units)),
                _ => String::new(),
            };
            let text = format!("{}{detail}", o.name);
            let w = (text.chars().count() as f64 * 7.2 + 22.0) * mm;
            let h = 24.0 * mm;
            let x = b.center().x;
            let y = b.y0 - (12.0 + lift) * mm - h;
            Some(view! {
                <g class="tag">
                    <rect x=x - w / 2.0 y=y width=w height=h rx=h / 2.0 />
                    <text x=x y=y + h / 2.0 font-size=12.5 * mm text-anchor="middle" dominant-baseline="central">{text}</text>
                </g>
            })
        })
    };
    let handles = move || state.with(handles_view);
    let content = move || {
        state.with(|s| {
            let mm_per_px = s.viewport.px_to_mm(1.0);
            scene(s.doc(), &s.selection)
                .into_iter()
                .map(|it| item_view(it, mm_per_px))
                .collect_view()
        })
    };
    let preview =
        move || state.with(|s| preview_view(s.preview().clone(), s.viewport.px_to_mm(1.0)));
    let transform = move || state.with(|s| s.viewport.svg_transform());
    let class = move || {
        if pan_from.get().is_some() {
            "plan panning"
        } else if space.get() {
            "plan can-pan"
        } else if drawing.get() {
            "plan drawing"
        } else {
            "plan"
        }
    };

    view! {
        <svg node_ref=svg_ref class=class on:wheel=on_wheel on:pointerdown=on_down
            on:pointermove=on_move on:pointerup=on_up on:pointercancel=on_cancel on:lostpointercapture=on_lost_capture
            on:dblclick=on_dblclick on:dragover=on_drag_over on:drop=on_drop on:contextmenu=|e: ev::MouseEvent| e.prevent_default()>
            {defs()}
            <g transform=transform>
                {grid}
                {photo}
                <g class="plan-content">{content}</g>
                {preview}
                {tag}
                {handles}
            </g>
        </svg>
        <PlanLegend />
    }
}
