//! The SVG plan view: events, viewport, and the overlays (scale bar, readout).

use leptos::ev;
use leptos::prelude::*;
use loma_core::geometry::Point;
use loma_core::units::format_length;
use loma_editor::render::scene;
use loma_editor::state::{EditorState, Mode};
use loma_editor::tools::{Input, Modifiers, ToolKind};
use loma_editor::viewport::Viewport;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use loma_editor::handles::{Corner, HandleKind};

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

/// Garden mode: pointer travel (px) after which a press pans instead of tapping.
const GARDEN_TAP_PX: f64 = 4.0;

/// Pointer travel (px) before a press on the compass becomes a turn, so a
/// click or double-click never moves north by itself.
const COMPASS_DRAG_PX: f64 = 3.0;

/// The plan's compass: shows true north, drag to turn it (5° steps, Alt for
/// free), double-click (or Enter) to type a bearing. Locked in Garden mode.
#[component]
fn Compass() -> impl IntoView {
    use loma_editor::compass::{
        bearing_from_pointer, bearing_text, free_bearing, location_text, parse_bearing,
        parse_location, snap,
    };
    use loma_editor::strings::EN;

    let state = expect_context::<RwSignal<EditorState>>();
    let north = Memo::new(move |_| state.with(|s| s.doc().north));
    let design = Memo::new(move |_| state.with(|s| s.mode() == Mode::Design));
    // the angle being dragged to, shown until the drag ends
    let turning = RwSignal::new(None::<f64>);
    let press = StoredValue::new(None::<(f64, f64)>);
    let moved = StoredValue::new(false);
    let compass_box = expect_context::<crate::app::CompassBox>();
    let editing = compass_box.open;
    let button = NodeRef::<leptos::html::Button>::new();
    let input = NodeRef::<leptos::html::Input>::new();
    let place_input = NodeRef::<leptos::html::Input>::new();
    let location = Memo::new(move |_| state.with(|s| s.doc().location));
    let saved_zone = Memo::new(move |_| state.with(|s| s.doc().time_zone.clone()));
    let place_text = move || location.get().map(location_text).unwrap_or_default();
    // "Use my location": waiting for the browser, or it said no
    let locating = RwSignal::new(false);
    let place_error = RwSignal::new(None::<&'static str>);
    let commit_place = move || {
        let Some(el) = place_input.get_untracked() else {
            return;
        };
        let text = el.value();
        if text.trim().is_empty() {
            return;
        }
        match parse_location(&text) {
            Some(p) => {
                state.update(|s| {
                    // the same place again keeps a zone the person chose
                    let zone = if s.doc().location == Some(p) {
                        s.doc().time_zone.clone()
                    } else {
                        let device = crate::app::device_zone();
                        Some(loma_editor::timezones::suggest_zone(p, device.as_deref()))
                    };
                    s.set_place(Some(p), zone, now());
                });
                place_error.set(None);
            }
            // not a place: say so, and show what's saved again
            None => {
                el.set_value(&place_text());
                place_error.set(Some(EN.location_unreadable));
            }
        }
    };
    let use_my_location = move |_| {
        locating.set(true);
        place_error.set(None);
        crate::app::current_position(move |found| {
            locating.set(false);
            match found {
                Some((lat, lon)) => {
                    let p = loma_core::model::GeoPoint::new(lat, lon);
                    state.update(|s| {
                        // the device is where the garden is: its zone is right
                        let zone = p.map(|p| {
                            let device = crate::app::device_zone();
                            loma_editor::timezones::zone_here(p, device.as_deref())
                        });
                        s.set_place(p, zone, now());
                    });
                }
                None => place_error.set(Some(EN.location_denied)),
            }
        });
    };

    let shown = move || turning.get().unwrap_or_else(|| north.get());
    let bearing_at = move |e: &ev::PointerEvent| -> Option<f64> {
        let r = button.get_untracked()?.get_bounding_client_rect();
        let (cx, cy) = (r.left() + r.width() / 2.0, r.top() + r.height() / 2.0);
        let b = bearing_from_pointer(cx, cy, e.client_x() as f64, e.client_y() as f64)?;
        Some(if e.alt_key() {
            free_bearing(b)
        } else {
            snap(b, 5.0)
        })
    };
    let stop = move || {
        press.set_value(None);
        moved.set_value(false);
        turning.set(None);
    };
    let on_down = move |e: ev::PointerEvent| {
        e.stop_propagation();
        if e.button() != 0 || !design.get_untracked() {
            return;
        }
        commit_focused_field();
        if let Some(el) = button.get_untracked() {
            // focused, so Escape reaches it even where a click doesn't focus buttons
            let _ = el.focus();
            let _ = el.set_pointer_capture(e.pointer_id());
        }
        press.set_value(Some((e.client_x() as f64, e.client_y() as f64)));
        moved.set_value(false);
    };
    let on_move = move |e: ev::PointerEvent| {
        let Some((x, y)) = press.get_value() else {
            return;
        };
        if !moved.get_value()
            && (e.client_x() as f64 - x).hypot(e.client_y() as f64 - y) < COMPASS_DRAG_PX
        {
            return;
        }
        moved.set_value(true);
        if let Some(b) = bearing_at(&e) {
            turning.set(Some(b));
        }
    };
    let on_up = move |_: ev::PointerEvent| {
        if let Some(b) = turning.get_untracked() {
            state.update(|s| {
                s.set_north(b, now());
            });
        }
        stop();
    };
    let on_key = move |e: ev::KeyboardEvent| match e.key().as_str() {
        "Escape" if press.get_value().is_some() => {
            e.stop_propagation();
            stop();
        }
        "Enter" | " " if design.get_untracked() => {
            e.prevent_default();
            e.stop_propagation(); // not an editor key (Enter would finish a line)
            editing.set(true);
        }
        _ => {}
    };
    let on_dblclick = move |e: ev::MouseEvent| {
        e.stop_propagation();
        if design.get_untracked() {
            editing.set(true);
        }
    };

    // the bearing editor
    let commit = move || {
        // only a changed text changes north: opening the box and leaving it
        // must not nudge a bearing it shows rounded
        if let Some(b) = input
            .get_untracked()
            .and_then(|el| parse_bearing(&el.value()))
            .filter(|b| bearing_text(*b) != bearing_text(north.get_untracked()))
        {
            state.update(|s| {
                s.set_north(b, now());
            });
        }
    };
    let close = move || {
        editing.set(false);
        if let Some(el) = button.get_untracked() {
            let _ = el.focus();
        }
    };
    Effect::new(move |_| {
        if editing.get() {
            let at_place = compass_box.at_place.get_untracked();
            compass_box.at_place.set(false);
            if at_place && let Some(el) = place_input.get() {
                let _ = el.focus();
            } else if let Some(el) = input.get() {
                let _ = el.focus();
            }
        }
    });
    let shown_text = move || bearing_text(north.get());

    let editor = move || {
        (editing.get() && design.get()).then(|| {
            view! {
                <div class="compass-edit" role="dialog" aria-label=EN.compass_bearing
                    // pressing the box (or its button) keeps focus in the field,
                    // so the box doesn't close before the button's click lands
                    on:mousedown=|e: ev::MouseEvent| {
                        let on_field = e
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                            .is_some_and(|el| el.tag_name() == "INPUT");
                        if !on_field {
                            e.prevent_default();
                        }
                    }
                    on:focusout=move |e: ev::FocusEvent| {
                        // leaving the box altogether closes it
                        let inside = e
                            .related_target()
                            .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                            .zip(e.current_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok()))
                            .is_some_and(|(t, me)| me.contains(Some(&t)));
                        // after Enter, Escape or Reset the box is already closing:
                        // leaving the field then must not save its text again
                        if !inside && editing.get_untracked() {
                            commit();
                            editing.set(false);
                        }
                    }>
                    <label>
                        <span>{EN.compass_bearing}</span>
                        <span class="compass-field">
                            <input node_ref=input type="text" inputmode="decimal" prop:value=shown_text
                                on:focus=move |_| {
                                    // select once the click that focused it is over
                                    if let Some(el) = input.get_untracked() {
                                        let cb = Closure::once_into_js(move || el.select());
                                        if let Some(w) = web_sys::window() {
                                            let _ = w.set_timeout_with_callback(cb.unchecked_ref());
                                        }
                                    }
                                }
                                on:keydown=move |e: ev::KeyboardEvent| match e.key().as_str() {
                                    "Enter" => { e.prevent_default(); commit(); close(); }
                                    "Escape" => { e.prevent_default(); e.stop_propagation(); close(); }
                                    _ => {}
                                } />
                            <span>"°"</span>
                        </span>
                    </label>
                    <p class="compass-hint">{EN.compass_hint}</p>
                    <button class="compass-reset" on:click=move |_| {
                        state.update(|s| { s.set_north(0.0, now()); });
                        close();
                    }>{EN.compass_reset}</button>
                    <label class="compass-place">
                        <span>{EN.location_title}</span>
                        <input node_ref=place_input type="text" inputmode="decimal"
                            placeholder="51.48, -0.01" prop:value=place_text
                            on:change=move |_| commit_place()
                            on:keydown=move |e: ev::KeyboardEvent| match e.key().as_str() {
                                "Enter" => { e.prevent_default(); commit_place(); }
                                "Escape" => {
                                    e.prevent_default();
                                    e.stop_propagation();
                                    // a cancel: put back what's saved so leaving the field saves nothing
                                    if let Some(el) = place_input.get_untracked() {
                                        el.set_value(&place_text());
                                    }
                                    close();
                                }
                                _ => {}
                            } />
                    </label>
                    <p class="compass-hint">{EN.location_hint}</p>
                    {move || place_error.get().map(|m| view! {
                        <p class="compass-hint error" role="alert">{m}</p>
                    })}
                    {move || location.get().is_some().then(|| {
                        let current = saved_zone.get();
                        // the table's zones, plus the saved one if it isn't among them
                        let mut zones: Vec<String> =
                            loma_editor::timezones::zone_names().map(String::from).collect();
                        if let Some(z) = &current && !zones.contains(z) {
                            zones.insert(0, z.clone());
                        }
                        let selected = current.clone().unwrap_or_default();
                        // a garden from before zones were saved has none yet
                        let unset = current.is_none();
                        view! {
                            <label class="compass-zone">
                                <span>{EN.time_zone}</span>
                                <select on:change=move |e| {
                                    let zone = Some(event_target_value(&e)).filter(|z| !z.is_empty());
                                    state.update(|s| { s.set_time_zone(zone, now()); });
                                }>
                                    <option value="" selected=unset>{EN.zone_not_set}</option>
                                    {zones.into_iter().map(|z| {
                                        let is = z == selected;
                                        view! { <option value=z.clone() selected=is>{z.replace('_', " ")}</option> }
                                    }).collect_view()}
                                </select>
                            </label>
                        }
                    })}
                    <div class="compass-place-actions">
                        <button class="compass-reset" disabled=move || locating.get()
                            on:click=use_my_location>{EN.use_my_location}</button>
                        {move || location.get().is_some().then(|| view! {
                            <button class="compass-reset" on:click=move |_| {
                                state.update(|s| { s.set_place(None, None, now()); });
                            }>{EN.clear_location}</button>
                        })}
                    </div>
                </div>
            }
        })
    };

    let needle = move || format!("rotate({:.2} 16 16)", shown());
    let label = move || {
        let (sin, cos) = shown().to_radians().sin_cos();
        (16.0 + 13.0 * sin, 16.0 - 13.0 * cos)
    };
    let title = move || {
        if design.get() {
            EN.compass_title
        } else {
            EN.compass_locked
        }
    };
    view! {
        <div class="card compass-wrap">
            <button node_ref=button class="compass" class:locked=move || !design.get()
                class:turning=move || turning.get().is_some()
                aria-disabled=move || (!design.get()).then_some("true")
                title=title aria-label=move || format!("{} {}°", title(), shown_text())
                on:pointerdown=on_down on:pointermove=on_move on:pointerup=on_up
                on:pointercancel=move |_| stop() on:lostpointercapture=move |_| stop()
                on:keydown=on_key on:dblclick=on_dblclick>
                <svg viewBox="0 0 32 32" width="40" height="40" aria-hidden="true">
                    <circle cx="16" cy="16" r="9.5" class="compass-ring" />
                    <g transform=needle>
                        <polygon points="16,7 18.6,16 13.4,16" class="compass-north" />
                        <polygon points="16,25 18.6,16 13.4,16" class="compass-south" />
                    </g>
                    <text x=move || label().0 y=move || label().1 class="compass-n">"N"</text>
                </svg>
            </button>
            {editor}
        </div>
    }
}

/// Map-style scale bar plus cursor readout, floating over the plan.
#[component]
fn PlanLegend() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let Cursor(cursor) = expect_context::<Cursor>();
    let view_info = Memo::new(move |_| state.with(|s| (s.viewport.scale, s.doc().units)));
    let bar = move || {
        let (scale, units) = view_info.get();
        let len = loma_editor::viewport::scale_bar_length(scale, units, 130.0); // fits the legend beside the dock
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
        .and_then(|o| loma_editor::objects::kind_shape(&o.kind).cloned());
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
    // Garden mode: where a press started, until it moves far enough to pan.
    let tap_from = StoredValue::new(None::<Point>);
    let garden = Memo::new(move |_| state.with(|s| s.mode() == Mode::Garden));
    let crate::garden::SelectSeveral(several) = expect_context::<crate::garden::SelectSeveral>();

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
        if garden.get_untracked() {
            // nothing moves in Garden mode: a press is a tap or the start of a
            // pan. A second finger while panning is ignored, so the plan
            // doesn't jump to it.
            if pan_from.get_untracked().is_none() && tap_from.get_value().is_none() {
                tap_from.set_value(Some(at));
            }
            return;
        }
        let m = modifiers(&e);
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            s.handle(Input::Down(w, m), now());
        });
    };
    let on_move = move |e: ev::PointerEvent| {
        let at = screen_point(e.client_x(), e.client_y());
        if let Some(start) = tap_from.get_value()
            && start.distance(at) > GARDEN_TAP_PX
        {
            tap_from.set_value(None);
            pan_from.set(Some(start));
        }
        if let Some(from) = pan_from.get_untracked() {
            state.update(|s| s.viewport.pan_by(at - from));
            pan_from.set(Some(at));
            return;
        }
        let m = modifiers(&e);
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            cursor.set(Some(w));
            if !garden.get_untracked() {
                s.handle(Input::Move(w, m), now());
            }
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
        if let Some(start) = tap_from.get_value() {
            tap_from.set_value(None);
            let additive = m.shift || m.ctrl || several.get_untracked();
            state.update(|s| {
                let w = s.viewport.screen_to_world(start);
                s.garden_tap(w, additive);
            });
            return;
        }
        state.update(|s| {
            let w = s.viewport.screen_to_world(at);
            s.handle(Input::Up(w, m), now());
        });
    };
    // The browser took the pointer away (gesture, alert, window switch):
    // stop panning and abandon whatever was being drawn.
    let on_cancel = move |_e: ev::PointerEvent| {
        pan_from.set(None);
        tap_from.set_value(None);
        state.update(|s| {
            s.handle(
                Input::Key(loma_editor::tools::Key::Escape, Modifiers::default()),
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
    // Dropping a .loma.json file on the plan imports it.
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
            if garden.get_untracked() {
                return; // tracing a photo changes the design
            }
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
            let b = loma_editor::objects::object_bounds(&o.kind);
            // sit above the rotate handle when there is one
            let lift = if s.handles().iter().any(|h| h.kind == HandleKind::Rotate) { 30.0 } else { 0.0 };
            let detail = match &o.kind {
                loma_core::model::ObjectKind::Plant { spread, .. } => format!(" · {}", format_length(*spread, s.doc().units)),
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
        } else if garden.get() {
            "plan garden"
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
                {crate::sunmap::layer(expect_context::<crate::sunmap::SunMap>())}
                {crate::garden::marks_view(state)}
                {preview}
                {tag}
                {handles}
            </g>
        </svg>
        <PlanLegend />
        <Compass />
    }
}
