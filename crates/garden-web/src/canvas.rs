//! The SVG plan view: grid, design, previews, and pointer/wheel handling.

use garden_core::geometry::Point;
use garden_core::model::Color;
use garden_editor::render::{RenderItem, RenderShape, color_css, scene};
use garden_editor::state::EditorState;
use garden_editor::tools::{Input, Modifiers, Preview, ToolKind};
use leptos::ev;
use leptos::prelude::*;

use crate::app::{Cursor, SpaceHeld, now};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

fn modifiers(e: &ev::MouseEvent) -> Modifiers {
    Modifiers {
        shift: e.shift_key(),
        alt: e.alt_key(),
        ctrl: e.ctrl_key() || e.meta_key(),
    }
}

const ACCENT: &str = "var(--accent)";
const HAIRLINE_PX: f64 = 1.5;
const LABEL_PX: f64 = 12.0;

fn css(c: Option<Color>) -> String {
    c.map(color_css).unwrap_or_else(|| "none".to_string())
}

/// One drawn object. `mm_per_px` converts screen sizes to world sizes.
fn item_view(it: RenderItem, mm_per_px: f64) -> AnyView {
    let fill = css(it.fill);
    let stroke = css(it.stroke);
    let (stroke_width, vector_effect) = match it.stroke_width {
        Some(w) => (w, None),
        None => (HAIRLINE_PX, Some("non-scaling-stroke")),
    };
    let main = match it.shape.clone() {
        RenderShape::Path { d, .. } => view! {
            <path d=d fill=fill stroke=stroke stroke-width=stroke_width vector-effect=vector_effect
                stroke-linecap="round" stroke-linejoin="round" />
        }
        .into_any(),
        RenderShape::Circle { center, r } => view! {
            <circle cx=center.x cy=center.y r=r fill=fill stroke=stroke stroke-width=HAIRLINE_PX
                vector-effect="non-scaling-stroke" />
        }
        .into_any(),
        RenderShape::Line { from, to } => view! {
            <line x1=from.x y1=from.y x2=to.x y2=to.y stroke=stroke stroke-width=HAIRLINE_PX
                vector-effect="non-scaling-stroke" />
        }
        .into_any(),
        RenderShape::Text { at, text, size } => {
            let size = if size > 0.0 {
                size
            } else {
                LABEL_PX * mm_per_px
            };
            view! {
                <text x=at.x y=at.y font-size=size fill=stroke text-anchor="middle"
                    dominant-baseline="middle" class="label">{text}</text>
            }
            .into_any()
        }
    };
    if !it.selected {
        return main;
    }
    let highlight = highlight_view(&it.shape, mm_per_px);
    view! { <g>{main}{highlight}</g> }.into_any()
}

/// Accent outline drawn over selected objects and previews.
fn highlight_view(shape: &RenderShape, mm_per_px: f64) -> AnyView {
    match shape.clone() {
        RenderShape::Path { d, .. } => view! {
            <path d=d fill="none" stroke=ACCENT stroke-width="2" vector-effect="non-scaling-stroke" />
        }
        .into_any(),
        RenderShape::Circle { center, r } => view! {
            <circle cx=center.x cy=center.y r=r fill="none" stroke=ACCENT stroke-width="2"
                vector-effect="non-scaling-stroke" />
        }
        .into_any(),
        RenderShape::Line { from, to } => view! {
            <line x1=from.x y1=from.y x2=to.x y2=to.y stroke=ACCENT stroke-width="2"
                vector-effect="non-scaling-stroke" />
        }
        .into_any(),
        RenderShape::Text { at, .. } => {
            let r = 6.0 * mm_per_px;
            view! {
                <circle cx=at.x cy=at.y r=r fill="none" stroke=ACCENT stroke-width="2"
                    vector-effect="non-scaling-stroke" />
            }
            .into_any()
        }
    }
}

fn preview_view(p: Preview, mm_per_px: f64) -> impl IntoView {
    let shapes = p
        .shapes
        .iter()
        .map(|s| {
            let outline = highlight_view(s, mm_per_px);
            view! { <g class="preview">{outline}</g> }
        })
        .collect_view();
    let dims = p
        .dims
        .into_iter()
        .map(|(at, text)| {
            view! {
                <text x=at.x y=at.y font-size=LABEL_PX * mm_per_px text-anchor="middle"
                    dominant-baseline="middle" class="dim">{text}</text>
            }
        })
        .collect_view();
    let snap = p.snap.map(|s| {
        view! {
            <circle cx=s.point.x cy=s.point.y r=4.0 * mm_per_px class="snap" fill="none" stroke=ACCENT
                stroke-width="1.5" vector-effect="non-scaling-stroke" />
        }
    });
    let marquee = p.marquee.map(|r| {
        view! {
            <rect x=r.x0 y=r.y0 width=r.width() height=r.height() class="marquee"
                vector-effect="non-scaling-stroke" />
        }
    });
    view! { {shapes}{dims}{snap}{marquee} }
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
            <g transform=transform>
                {grid}
                {content}
                {preview}
            </g>
        </svg>
    }
}
