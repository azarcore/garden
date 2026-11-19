//! Printing the plan at a chosen scale on A4 or Letter.

use garden_core::units::format_length;
use garden_editor::print::{
    Layout, Paper, STANDARD_SCALES, best_standard_scale, fit_to_page, layout_at,
};
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use leptos::prelude::*;

/// Whether the print card is open.
#[derive(Clone, Copy)]
pub struct PrintOpen(pub RwSignal<bool>);

/// Which scale the person picked: a standard 1:n, or fit to the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    Standard(u32),
    Fit,
}

/// Everything drawn, framed with a little room around it.
fn plan_bounds(state: RwSignal<EditorState>) -> Option<kurbo::Rect> {
    state.with_untracked(|s| {
        s.doc()
            .objects
            .iter()
            .map(|o| garden_editor::objects::object_bounds(&o.kind))
            .reduce(|a, b| a.union(b))
            .map(|b| b.inflate(500.0, 500.0))
    })
}

/// A scale bar about 40–60 mm long on paper, labelled in real units.
fn scale_bar_svg(scale: f64, units: garden_core::units::UnitSystem) -> String {
    let lengths_mm = [
        500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0, 50_000.0, 100_000.0,
    ];
    let real = lengths_mm
        .into_iter()
        .rev()
        .find(|l| l / scale <= 60.0)
        .unwrap_or(500.0);
    let paper = real / scale;
    let quarter = paper / 4.0;
    let blocks: String = (0..4)
        .map(|i| {
            let fill = if i % 2 == 0 { "#1c241e" } else { "#ffffff" };
            format!(r##"<rect x="{x}mm" y="0" width="{quarter}mm" height="2mm" fill="{fill}" stroke="#1c241e" stroke-width="0.2mm"/>"##, x = i as f64 * quarter)
        })
        .collect();
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="8mm">{blocks}<text x="0" y="6.5mm" font-size="3mm">0</text><text x="{paper}mm" y="6.5mm" font-size="3mm" text-anchor="end">{label}</text></svg>"#,
        w = paper + 1.0,
        label = format_length(real, units)
    )
}

/// Build the print page and open the browser's print dialog.
fn print_now(state: RwSignal<EditorState>, paper: Paper, layout: Layout) {
    let Some(bounds) = plan_bounds(state) else {
        return;
    };
    let Some((svg, _, _)) =
        crate::files::plan_svg(bounds, Some((layout.width_mm, layout.height_mm)))
    else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(doc) = window.document() else { return };
    let (name, units) = state.with_untracked(|s| (s.doc().name.clone(), s.doc().units));
    let date =
        js_sys::Date::new_0().to_locale_date_string("default", &wasm_bindgen::JsValue::UNDEFINED);
    let scale = format!("1:{:.0}", layout.scale);
    let html = format!(
        r#"<header class="print-title"><strong>{name}</strong><span>{scale}</span><span>{date}</span></header>{svg}<footer class="print-bar">{bar}</footer>"#,
        name = html_escape(&name),
        date = String::from(date),
        bar = scale_bar_svg(layout.scale, units),
    );
    let root = match doc.get_element_by_id("print-root") {
        Some(el) => el,
        None => {
            let Ok(el) = doc.create_element("div") else {
                return;
            };
            el.set_id("print-root");
            if let Some(body) = doc.body() {
                let _ = body.append_child(&el);
            }
            el
        }
    };
    root.set_inner_html(&html);
    // page size and orientation for this print
    let page = format!(
        "@page {{ size: {} {}; margin: {}mm; }}",
        match paper {
            Paper::A4 => "A4",
            Paper::Letter => "letter",
        },
        if layout.landscape {
            "landscape"
        } else {
            "portrait"
        },
        garden_editor::print::PRINT_MARGIN_MM
    );
    let style = doc.get_element_by_id("print-page").or_else(|| {
        let el = doc.create_element("style").ok()?;
        el.set_id("print-page");
        doc.head()?.append_child(&el).ok()?;
        Some(el)
    });
    if let Some(style) = style {
        style.set_text_content(Some(&page));
    }
    let _ = window.print();
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Card to choose paper and scale, then print.
#[component]
pub fn PrintCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let PrintOpen(open) = expect_context::<PrintOpen>();
    let paper = RwSignal::new(Paper::A4);
    let choice = RwSignal::new(None::<Choice>);
    // the scale actually used: the person's choice, else the best that fits
    let chosen = move || {
        let bounds = plan_bounds(state)?;
        Some(
            choice
                .get()
                .unwrap_or_else(|| match best_standard_scale(bounds, paper.get()) {
                    Some(s) => Choice::Standard(s),
                    None => Choice::Fit,
                }),
        )
    };
    let layout = move || {
        let bounds = plan_bounds(state)?;
        match chosen()? {
            Choice::Standard(s) => layout_at(bounds, paper.get(), s),
            Choice::Fit => Some(fit_to_page(bounds, paper.get())),
        }
    };
    let summary = move || -> Option<String> {
        let l = layout()?;
        let units = state.with_untracked(|s| s.doc().units);
        let paper_name = match paper.get() {
            Paper::A4 => EN.paper_a4,
            Paper::Letter => EN.paper_letter,
        };
        let orientation = if l.landscape {
            EN.landscape
        } else {
            EN.portrait
        };
        Some(format!(
            "{} 1:{:.0} {} {} {} — {} × {}",
            EN.prints_at,
            l.scale,
            EN.on_paper,
            paper_name,
            orientation,
            format_length(l.width_mm, units),
            format_length(l.height_mm, units)
        ))
    };
    let too_big = move || -> Option<String> {
        let bounds = plan_bounds(state)?;
        if layout().is_some() {
            return None;
        }
        Some(match best_standard_scale(bounds, paper.get()) {
            Some(s) => format!("{}{s}", EN.print_too_big),
            None => EN.fit_page.to_string(),
        })
    };
    let options = move || {
        let current = chosen();
        let mut views = STANDARD_SCALES
            .into_iter()
            .map(|s| {
                let selected = current == Some(Choice::Standard(s));
                view! { <option value=s.to_string() selected=selected>{format!("1:{s}")}</option> }
                    .into_any()
            })
            .collect::<Vec<_>>();
        views.push(view! { <option value="fit" selected=current == Some(Choice::Fit)>{EN.fit_page}</option> }.into_any());
        views
    };
    view! {
        <Show when=move || open.get()>
            <section class="card print-card" aria-label=EN.print>
                <header class="trace-head">
                    <h2>{EN.print}</h2>
                    <button class="icon-btn" aria-label=EN.close on:click=move |_| open.set(false)>
                        {crate::icons::icon(crate::icons::Icon::Close)}
                    </button>
                </header>
                <div class="print-row">
                    <span>{EN.paper_label}</span>
                    <div class="segmented" role="group">
                        <button aria-pressed=move || (paper.get() == Paper::A4).to_string()
                            on:click=move |_| paper.set(Paper::A4)>{EN.paper_a4}</button>
                        <button aria-pressed=move || (paper.get() == Paper::Letter).to_string()
                            on:click=move |_| paper.set(Paper::Letter)>{EN.paper_letter}</button>
                    </div>
                </div>
                <label class="print-row">
                    <span>{EN.scale_label}</span>
                    <select class="print-scale" on:change=move |e| {
                        let v = event_target_value(&e);
                        choice.set(Some(match v.parse::<u32>() {
                            Ok(s) => Choice::Standard(s),
                            Err(_) => Choice::Fit,
                        }));
                    }>{options}</select>
                </label>
                <p class="print-summary">{summary}</p>
                {move || too_big().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
                <button class="trace-primary" disabled=move || layout().is_none()
                    on:click=move |_| {
                        if let Some(l) = layout() {
                            print_now(state, paper.get_untracked(), l);
                        }
                    }>{EN.print}</button>
            </section>
        </Show>
    }
}
