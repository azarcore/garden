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

use crate::app::{Cursor, SpaceHeld, now};
use crate::plan_art::{defs, item_view, preview_view};

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
    view! {
        <div class="legend">{bar}</div>
        <div class="readout">{readout}</div>
    }
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
    let on_dblclick = move |e: ev::MouseEvent| {
        let at = screen_point(e.client_x(), e.client_y());
        let m = modifiers(&e);
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            s.handle(Input::DoubleClick(w, m), now());
        });
    };
    let drawing = Memo::new(move |_| state.with(|s| !matches!(s.tool(), ToolKind::Select)));

    let grid = move || {
        state.with(|s| {
            let v = &s.viewport;
            let a = v.screen_to_world(Point::ZERO);
            let b = v.screen_to_world(Point::new(v.width, v.height));
            // smallest step that keeps lines at least 12 px apart
            let step = [100.0, 500.0, 1000.0, 5000.0, 10_000.0, 50_000.0, 100_000.0]
                .into_iter()
                .find(|st| st * v.scale >= 12.0)
                .unwrap_or(100_000.0);
            let mut d = String::new();
            let mut x = (a.x / step).floor() * step;
            while x <= b.x {
                d.push_str(&format!("M{x} {} V{} ", a.y, b.y));
                x += step;
            }
            let mut y = (a.y / step).floor() * step;
            while y <= b.y {
                d.push_str(&format!("M{} {y} H{} ", a.x, b.x));
                y += step;
            }
            view! { <path d=d class="grid-line" fill="none" vector-effect="non-scaling-stroke" /> }
        })
    };
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
            on:pointermove=on_move on:pointerup=on_up on:dblclick=on_dblclick on:contextmenu=|e: ev::MouseEvent| e.prevent_default()>
            {defs()}
            <g transform=transform>
                {grid}
                {content}
                {preview}
            </g>
        </svg>
        <PlanLegend />
    }
}
