//! The sun map: hours of direct sun over the plan, in three bands, worked
//! out a day at a time so the page stays responsive. A view setting only.

use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;
use loma_core::date::Date;
use loma_editor::state::EditorState;
use loma_editor::strings::EN;
use loma_editor::sunmap::{Band, Span, SunGrid, SunMapJob, band};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::app::{CompassBox, Cursor};

/// Whether the map shows the growing season or one day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanChoice {
    Season,
    Day,
}

#[derive(Clone, Copy)]
pub struct SunMap {
    pub on: RwSignal<bool>,
    span: RwSignal<SpanChoice>,
    day: RwSignal<Date>,
    /// The last finished map, and its picture.
    grid: RwSignal<Option<Rc<SunGrid>>, LocalStorage>,
    picture: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    /// Bumped for every new job; an older job stops when it sees it moved on.
    generation: StoredValue<u64>,
}

pub fn provide_sun_map() {
    provide_context(SunMap {
        on: RwSignal::new(false),
        span: RwSignal::new(SpanChoice::Season),
        day: RwSignal::new(crate::garden::today()),
        grid: RwSignal::new_local(None),
        picture: RwSignal::new(None),
        busy: RwSignal::new(false),
        generation: StoredValue::new(0),
    });
}

/// Colours of the bands (sRGB), drawn see-through over the plan.
fn band_colour(b: Band) -> &'static str {
    match b {
        Band::Full => "#f2c14e",
        Band::Part => "#9fc46a",
        Band::Shade => "#4f6d8a",
    }
}

/// Turn the map on or off. Without a location it opens the compass box at
/// the Location field instead.
pub fn toggle(map: SunMap, state: RwSignal<EditorState>, compass: CompassBox) {
    if map.on.get_untracked() {
        map.on.set(false);
        return;
    }
    if state.with_untracked(|s| s.doc().location.is_none()) {
        compass.at_place.set(true);
        compass.open.set(true);
        return;
    }
    map.on.set(true);
}

async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(p).await;
}

/// The grid as a picture: one pixel per square, in band colours.
fn paint(grid: &SunGrid) -> Option<String> {
    let doc = web_sys::window()?.document()?;
    let canvas: web_sys::HtmlCanvasElement = doc.create_element("canvas").ok()?.unchecked_into();
    canvas.set_width(grid.cols as u32);
    canvas.set_height(grid.rows as u32);
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok()??.unchecked_into();
    for row in 0..grid.rows {
        // runs of one band in a row become one rectangle
        let mut col = 0;
        while col < grid.cols {
            let b = band(grid.hours[row * grid.cols + col]);
            let mut end = col + 1;
            while end < grid.cols && band(grid.hours[row * grid.cols + end]) == b {
                end += 1;
            }
            ctx.set_fill_style_str(band_colour(b));
            ctx.fill_rect(col as f64, row as f64, (end - col) as f64, 1.0);
            col = end;
        }
    }
    canvas.to_data_url().ok()
}

/// Work for at most this long (ms) before letting the page draw.
const BUDGET_MS: f64 = 8.0;

/// Work out the map again whenever it's on and the design, its place or the
/// chosen days change; quietly, a day at a time, after a short pause.
pub fn run_sun_map() {
    let map = expect_context::<SunMap>();
    let state = expect_context::<RwSignal<EditorState>>();
    // only what shadows depend on: a rename or a care note changes nothing
    let key = Memo::new(move |_| state.with(|s| loma_editor::sunmap::fingerprint(s.doc())));
    let design = Memo::new(move |_| state.with(|s| s.doc().id));
    // a map from another design, or from before the map was switched off,
    // is never shown over this one
    let shown_for = StoredValue::new(None::<uuid::Uuid>);
    Effect::new(move |_| {
        let on = map.on.get();
        let _ = key.get();
        let id = design.get();
        if !on || shown_for.get_value() != Some(id) {
            map.grid.set(None);
            map.picture.set(None);
            shown_for.set_value(on.then_some(id));
        }
        let span = match map.span.get() {
            SpanChoice::Season => Span::Season {
                year: crate::garden::today().year(),
            },
            SpanChoice::Day => Span::Day(map.day.get()),
        };
        let generation = map.generation.get_value() + 1;
        map.generation.set_value(generation);
        if !on {
            map.busy.set(false);
            return;
        }
        let current = move || map.generation.get_value() == generation;
        spawn_local(async move {
            sleep(300).await; // let a burst of edits settle
            if !current() {
                return;
            }
            let Some(mut job) = state.with_untracked(|s| SunMapJob::new(s.doc(), span)) else {
                // no location (any more) or nothing drawn
                map.grid.set(None);
                map.picture.set(None);
                map.busy.set(false);
                if state.with_untracked(|s| s.doc().location.is_none()) {
                    map.on.set(false);
                }
                return;
            };
            map.busy.set(true);
            // a few milliseconds of work between frames, so nothing stutters
            let clock = || js_sys::Date::now();
            let mut slice_start = clock();
            loop {
                let finished = job.step();
                if finished || clock() - slice_start > BUDGET_MS {
                    sleep(0).await; // let the page draw and respond
                    if !current() {
                        return; // a newer job has taken over
                    }
                    slice_start = clock();
                }
                if finished {
                    break;
                }
            }
            let grid = job.grid().clone();
            map.picture.set(paint(&grid));
            map.grid.set(Some(Rc::new(grid)));
            map.busy.set(false);
        });
    });
}

/// The map over the plan, in plan coordinates (inside the plan's transform),
/// kept inside the plot boundary when there is one.
pub fn layer(map: SunMap) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let outline = Memo::new(move |_| state.with(|s| loma_editor::sunmap::plot_outline(s.doc())));
    move || {
        let on = map.on.get();
        let picture = map.picture.get()?;
        let rect = map.grid.with(|g| g.as_ref().map(|g| g.rect()))?;
        let clip = outline.get();
        on.then(|| {
            let clip_to = clip.as_ref().map(|_| "url(#sun-map-clip)");
            view! {
                <g class="sun-map-layer">
                    {clip.map(|d| view! { <clipPath id="sun-map-clip"><path d=d /></clipPath> })}
                    <image href=picture x=rect.x0 y=rect.y0 width=rect.width()
                        height=rect.height() preserveAspectRatio="none" clip-path=clip_to />
                </g>
            }
        })
    }
}

/// The key, the season/day choice and the hours at the pointer.
#[component]
pub fn SunMapCard() -> impl IntoView {
    let map = expect_context::<SunMap>();
    let state = expect_context::<RwSignal<EditorState>>();
    let Cursor(cursor) = expect_context::<Cursor>();
    let south = move || state.with_untracked(|s| s.doc().location.is_some_and(|p| p.lat < 0.0));
    let preset = move |month: u32, day: u32| {
        let year = crate::garden::today().year();
        if let Some(d) = Date::from_ymd(year, month, day) {
            map.day.set(d);
        }
    };
    let here = move || {
        let p = cursor.get()?;
        if !state.with(|s| loma_editor::sunmap::in_garden(s.doc(), p)) {
            return None;
        }
        let h = map.grid.with(|g| g.as_ref().and_then(|g| g.hours_at(p)))?;
        let rounded = (h * 2.0).round() / 2.0; // to the half hour
        let text = if rounded.fract() == 0.0 {
            format!("{rounded:.0}")
        } else {
            format!("{rounded:.1}")
        };
        Some(EN.sun_here.replace("{}", &text))
    };
    let row = |b: Band, name: &'static str, range: &'static str| {
        view! {
            <li><span class="sun-swatch" style=format!("background:{}", band_colour(b))></span>
                <span class="sun-band">{name}</span><span class="sun-range">{range}</span></li>
        }
    };
    let span_button = move |choice: SpanChoice, label: &'static str| {
        view! {
            <button aria-pressed=move || (map.span.get() == choice).to_string()
                on:click=move |_| map.span.set(choice)>{label}</button>
        }
    };
    view! {
        <Show when=move || map.on.get()>
            <section class="card sun-map-card" aria-label=EN.sun_map>
                <ul class="sun-key">
                    {row(Band::Full, EN.sun_full, EN.sun_full_range)}
                    {row(Band::Part, EN.sun_part, EN.sun_part_range)}
                    {row(Band::Shade, EN.sun_shade, EN.sun_shade_range)}
                </ul>
                <div class="sun-span">
                    {span_button(SpanChoice::Season, EN.sun_season)}
                    {span_button(SpanChoice::Day, EN.sun_one_day)}
                </div>
                <Show when=move || map.span.get() == SpanChoice::Day>
                    <input type="date" class="sun-day" prop:value=move || map.day.get().to_string()
                        on:change=move |e| {
                            if let Some(d) = Date::parse(&event_target_value(&e)) {
                                map.day.set(d);
                            }
                        } />
                    <div class="sun-presets">
                        <button on:click=move |_| if south() { preset(12, 21) } else { preset(6, 21) }>
                            {EN.sun_midsummer}</button>
                        <button on:click=move |_| preset(3, 20)>{EN.sun_equinox}</button>
                        <button on:click=move |_| if south() { preset(6, 21) } else { preset(12, 21) }>
                            {EN.sun_midwinter}</button>
                    </div>
                </Show>
                <p class="sun-here" aria-live="polite">
                    {move || if map.busy.get() { Some(EN.sun_working.to_string()) } else { here() }}
                </p>
            </section>
        </Show>
    }
}
