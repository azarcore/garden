//! Garden mode in the browser: the Design / Garden switch, today's date, and
//! the health marks drawn on the plan.

use garden_core::date::Date;
use garden_core::geometry::Point;
use garden_core::model::{Document, Health, ObjectKind};
use garden_editor::care::{GardenMark, garden_marks};
use garden_editor::state::{EditorState, Mode};
use garden_editor::strings::EN;
use leptos::prelude::*;

const MODE_KEY: &str = "garden-mode";

/// On phones, taps add to or remove from the selection instead of replacing it.
#[derive(Clone, Copy)]
pub struct SelectSeveral(pub RwSignal<bool>);

/// Today in the person's own time zone.
pub fn today() -> Date {
    let d = js_sys::Date::new_0();
    Date::from_ymd(d.get_full_year() as i32, d.get_month() + 1, d.get_date())
        .unwrap_or_else(|| Date::parse("2000-01-01").expect("valid date"))
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// The mode this browser used last.
pub fn saved_mode() -> Mode {
    match storage().and_then(|s| s.get_item(MODE_KEY).ok().flatten()) {
        Some(m) if m == "garden" => Mode::Garden,
        _ => Mode::Design,
    }
}

/// Switch modes, remember the choice and close the design-only panels.
pub fn set_mode(state: RwSignal<EditorState>, mode: Mode) {
    if state.with_untracked(|s| s.mode()) == mode {
        return;
    }
    state.update(|s| s.set_mode(mode));
    if let Some(s) = storage() {
        let _ = s.set_item(
            MODE_KEY,
            if mode == Mode::Garden {
                "garden"
            } else {
                "design"
            },
        );
    }
    if mode == Mode::Garden {
        if let Some(c) = use_context::<crate::catalog::CatalogOpen>() {
            c.0.set(false);
        }
        if let Some(l) = use_context::<crate::layers::LayersOpen>() {
            l.0.set(false);
        }
        if let Some(t) = use_context::<crate::trace::Trace>() {
            t.open.set(false);
        }
    } else {
        if let Some(SelectSeveral(several)) = use_context::<SelectSeveral>() {
            several.set(false);
        }
        if let Some(ui) = use_context::<crate::care::CareUi>() {
            ui.products_open.set(false);
            ui.form.set(None);
        }
    }
}

/// Design / Garden, at the start of the controls.
#[component]
pub fn ModeSwitch() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let mode = Memo::new(move |_| state.with(|s| s.mode()));
    view! {
        <div class="segmented mode-switch" role="group" aria-label=EN.mode_label>
            <button aria-pressed=move || (mode.get() == Mode::Design).to_string()
                on:click=move |_| set_mode(state, Mode::Design)>{EN.mode_design}</button>
            <button aria-pressed=move || (mode.get() == Mode::Garden).to_string()
                on:click=move |_| set_mode(state, Mode::Garden)>{EN.mode_garden}</button>
        </div>
    }
}

/// Half-way along a line, by length.
fn midpoint(points: &[Point]) -> Option<Point> {
    let total: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut left = total / 2.0;
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        if len >= left && len > 0.0 {
            return Some(w[0].lerp(w[1], left / len));
        }
        left -= len;
    }
    points.first().copied()
}

fn badge(at: Point, mm: f64) -> impl IntoView {
    let r = 8.0 * mm;
    view! {
        <g class="mark-badge">
            <title>{EN.not_safe_to_harvest}</title>
            <circle cx=at.x cy=at.y r=r />
            <text x=at.x y=at.y font-size=11.0 * mm text-anchor="middle" dominant-baseline="central">"!"</text>
        </g>
    }
}

fn mark_view(doc: &Document, m: &GardenMark, mm: f64) -> Option<AnyView> {
    let o = doc.object(m.id)?;
    let class = match m.health {
        Health::Healthy => "",
        Health::Watch => "mark mark-watch",
        Health::Sick => "mark mark-sick",
        Health::Gone => "mark mark-gone",
    };
    Some(match &o.kind {
        ObjectKind::Plant {
            position, spread, ..
        } => {
            let r = spread / 2.0;
            let ring = (m.health != Health::Healthy).then(|| {
                let cross = (m.health == Health::Gone).then(|| {
                    let c = r * 0.45;
                    view! {
                        <path class="mark-cross" vector-effect="non-scaling-stroke"
                            d=format!("M{} {}L{} {}M{} {}L{} {}",
                                position.x - c, position.y - c, position.x + c, position.y + c,
                                position.x - c, position.y + c, position.x + c, position.y - c) />
                    }
                });
                view! {
                    <circle class=class cx=position.x cy=position.y r=r + 3.0 * mm
                        vector-effect="non-scaling-stroke" />
                    {cross}
                }
            });
            let badge = m.not_safe.then(|| {
                let d = r * std::f64::consts::FRAC_1_SQRT_2;
                badge(Point::new(position.x + d, position.y - d), mm)
            });
            view! { <g>{ring}{badge}</g> }.into_any()
        }
        ObjectKind::Linear { shape, .. } => {
            let line = (m.health != Health::Healthy).then(|| {
                view! {
                    <path class=format!("{class} open") d=shape.to_bez_path().to_svg()
                        vector-effect="non-scaling-stroke" />
                }
            });
            let badge = m
                .not_safe
                .then(|| midpoint(&shape.vertices()).map(|p| badge(p, mm)))
                .flatten();
            view! { <g>{line}{badge}</g> }.into_any()
        }
        _ => return None,
    })
}

/// Health rings, gone crosses and "not safe to harvest" badges, in Garden mode.
pub fn marks_view(state: RwSignal<EditorState>) -> impl IntoView {
    move || {
        state.with(|s| {
            if s.mode() != Mode::Garden {
                return None;
            }
            let mm = s.viewport.px_to_mm(1.0);
            let doc = s.doc();
            let views = garden_marks(doc, today())
                .iter()
                .filter_map(|m| mark_view(doc, m, mm))
                .collect_view();
            Some(view! { <g class="garden-marks">{views}</g> })
        })
    }
}
