//! Printing the plan at a chosen scale on A4 or Letter.

use leptos::prelude::*;
use loma_core::units::format_length;
use loma_editor::print::{
    Layout, Paper, STANDARD_SCALES, Tiling, best_standard_scale, cut_lines, fit_in, fit_to_page,
    layout_at, neighbours, tile,
};
use loma_editor::state::EditorState;
use loma_editor::strings::EN;

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
            .map(|o| loma_editor::objects::object_bounds(&o.kind))
            .reduce(|a, b| a.union(b))
            .map(|b| b.inflate(500.0, 500.0))
    })
}

/// A scale bar about 40–60 mm long on paper, labelled in real units.
fn scale_bar_svg(scale: f64, units: loma_core::units::UnitSystem) -> String {
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
    show_and_print(&html, paper, layout.landscape);
}

/// Print `tiling` as one page per sheet, after an overview page if asked for.
fn print_tiled(state: RwSignal<EditorState>, paper: Paper, tiling: &Tiling, overview: bool) {
    let Some(bounds) = plan_bounds(state) else {
        return;
    };
    let (name, units) = state.with_untracked(|s| (s.doc().name.clone(), s.doc().units));
    let name = html_escape(&name);
    let scale = format!("1:{:.0}", tiling.scale);
    let total = tiling.tiles.len() + usize::from(overview);
    let mut pages = Vec::with_capacity(total);
    if overview {
        let fit = fit_in(bounds, paper, tiling.landscape);
        if let Some((svg, _, _)) =
            crate::files::plan_svg(bounds, Some((fit.width_mm, fit.height_mm)))
        {
            // the page grid over the plan, in world units (fit.scale world mm per paper mm)
            let k = fit.scale;
            let grid: String = tiling
                .tiles
                .iter()
                .map(|t| {
                    let w = t.world;
                    format!(
                        r##"<rect x="{x}" y="{y}" width="{wd}" height="{ht}" fill="none" stroke="#c0392b" stroke-width="{sw}"/><text x="{tx}" y="{ty}" font-size="{fs}" fill="#c0392b" font-weight="700">{label}</text>"##,
                        x = w.x0, y = w.y0, wd = w.width(), ht = w.height(), sw = 0.4 * k,
                        tx = w.x0 + 2.0 * k, ty = w.y0 + 6.0 * k, fs = 5.0 * k, label = t.label,
                    )
                })
                .collect();
            let svg = match svg.rfind("</svg>") {
                Some(at) => format!(
                    "{}<g class=\"tile-grid\">{grid}</g>{}",
                    &svg[..at],
                    &svg[at..]
                ),
                None => svg,
            };
            pages.push(format!(
                r#"<section class="print-page"><header class="print-title"><strong>{name}</strong><span>{overview_title}</span><span>{scale} · {n} {pages_word}</span><span>{page} 1 {of} {total}</span></header>{svg}<footer class="print-bar">{bar}</footer></section>"#,
                overview_title = EN.overview_title,
                n = tiling.tiles.len(),
                pages_word = EN.pages,
                page = EN.page_word,
                of = EN.of_word,
                bar = scale_bar_svg(fit.scale, units),
            ));
        }
    }
    let (pw, ph) = tiling.page_mm;
    for (i, t) in tiling.tiles.iter().enumerate() {
        let Some((svg, _, _)) = crate::files::plan_svg(t.world, Some((pw, ph))) else {
            continue;
        };
        let [left, right, up, down] = neighbours(t, tiling);
        // a cut line in the middle of each overlap (the neighbour marks the same line), and the neighbours' labels
        let mut marks = String::new();
        let tick = |x1: f64, y1: f64, x2: f64, y2: f64| {
            format!(
                r##"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#1c241e" stroke-width="0.3"/>"##
            )
        };
        let dashed = |x1: f64, y1: f64, x2: f64, y2: f64| {
            format!(
                r##"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#999" stroke-width="0.2" stroke-dasharray="2 2"/>"##
            )
        };
        let tag = |x: f64, y: f64, anchor: &str, text: String| {
            format!(
                r##"<text x="{x}" y="{y}" font-size="3.2" text-anchor="{anchor}" fill="#1c241e" font-weight="700" paint-order="stroke" stroke="#fff" stroke-width="1.2">{text}</text>"##
            )
        };
        let [cut_l, cut_r, cut_u, cut_d] = cut_lines(t, tiling);
        let side = |at: Option<f64>, label: &Option<String>| at.zip(label.clone());
        if let Some((x, l)) = side(cut_l, &left) {
            marks += &tick(x, 0.0, x, 5.0);
            marks += &tick(x, ph - 5.0, x, ph);
            marks += &dashed(x, 5.0, x, ph - 5.0);
            marks += &tag(x + 1.5, ph / 2.0, "start", format!("← {l}"));
        }
        if let Some((x, r)) = side(cut_r, &right) {
            marks += &tick(x, 0.0, x, 5.0);
            marks += &tick(x, ph - 5.0, x, ph);
            marks += &dashed(x, 5.0, x, ph - 5.0);
            marks += &tag(x - 1.5, ph / 2.0, "end", format!("{r} →"));
        }
        if let Some((y, u)) = side(cut_u, &up) {
            marks += &tick(0.0, y, 5.0, y);
            marks += &tick(pw - 5.0, y, pw, y);
            marks += &dashed(5.0, y, pw - 5.0, y);
            marks += &tag(pw / 2.0, y + 4.0, "middle", format!("↑ {u}"));
        }
        if let Some((y, d)) = side(cut_d, &down) {
            marks += &tick(0.0, y, 5.0, y);
            marks += &tick(pw - 5.0, y, pw, y);
            marks += &dashed(5.0, y, pw - 5.0, y);
            marks += &tag(pw / 2.0, y - 1.5, "middle", format!("↓ {d}"));
        }
        let overlay = format!(
            r#"<svg class="print-marks" xmlns="http://www.w3.org/2000/svg" width="{pw}mm" height="{ph}mm" viewBox="0 0 {pw} {ph}">{marks}</svg>"#
        );
        let n = i + 1 + usize::from(overview);
        pages.push(format!(
            r#"<section class="print-page"><header class="print-title"><strong>{name}</strong><span class="print-label">{label}</span><span>{scale}</span><span>{page} {n} {of} {total}</span></header><div class="print-sheet" style="width:{pw}mm;height:{ph}mm">{svg}{overlay}</div><footer class="print-bar">{bar}</footer></section>"#,
            label = t.label,
            page = EN.page_word,
            of = EN.of_word,
            bar = scale_bar_svg(tiling.scale, units),
        ));
    }
    show_and_print(&pages.concat(), paper, tiling.landscape);
}

/// Replace the print area with `html`, set the page size and open the print dialog.
pub(crate) fn show_and_print(html: &str, paper: Paper, landscape: bool) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(doc) = window.document() else { return };
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
    root.set_inner_html(html);
    // page size and orientation for this print
    let page = format!(
        "@page {{ size: {} {}; margin: {}mm; }}",
        match paper {
            Paper::A4 => "A4",
            Paper::Letter => "letter",
        },
        if landscape { "landscape" } else { "portrait" },
        loma_editor::print::PRINT_MARGIN_MM
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
    let overview = RwSignal::new(true);
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
    // a standard scale that doesn't fit one page prints on several
    let tiling = move || -> Option<Tiling> {
        if layout().is_some() {
            return None;
        }
        let bounds = plan_bounds(state)?;
        match chosen()? {
            Choice::Standard(s) => tile(bounds, paper.get(), s),
            Choice::Fit => None,
        }
    };
    let paper_name = move || match paper.get() {
        Paper::A4 => EN.paper_a4,
        Paper::Letter => EN.paper_letter,
    };
    let summary = move || -> Option<String> {
        if let Some(t) = tiling() {
            return Some(format!(
                "{} {} {} ({} × {}), {} {}, 1:{:.0}",
                EN.prints_on,
                t.tiles.len(),
                EN.pages,
                t.cols,
                t.rows,
                paper_name(),
                if t.landscape {
                    EN.landscape
                } else {
                    EN.portrait
                },
                t.scale
            ));
        }
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
        if layout().is_some() || tiling().is_some() {
            return None;
        }
        // more than MAX_TILES pages: suggest the most detailed scale that tiles
        let chosen_scale = match chosen()? {
            Choice::Standard(s) => s,
            Choice::Fit => return None,
        };
        Some(
            match STANDARD_SCALES
                .into_iter()
                .filter(|s| *s > chosen_scale)
                .find(|s| tile(bounds, paper.get(), *s).is_some())
            {
                Some(s) => format!("{}{s}", EN.too_many_pages),
                None => EN.fit_page.to_string(),
            },
        )
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
                <Show when=move || tiling().is_some()>
                    <label class="print-row print-check">
                        <input type="checkbox" prop:checked=move || overview.get()
                            on:change=move |e| overview.set(event_target_checked(&e)) />
                        <span>{EN.overview_page}</span>
                    </label>
                </Show>
                {move || too_big().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
                <button class="trace-primary" disabled=move || layout().is_none() && tiling().is_none()
                    on:click=move |_| {
                        if let Some(l) = layout() {
                            print_now(state, paper.get_untracked(), l);
                        } else if let Some(t) = tiling() {
                            print_tiled(state, paper.get_untracked(), &t, overview.get_untracked());
                        }
                    }>{EN.print}</button>
            </section>
        </Show>
    }
}
