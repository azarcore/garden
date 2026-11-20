//! The garden list card: what the design needs, with optional prices,
//! a CSV export and a printable page.

use std::collections::BTreeMap;

use garden_core::units::{UnitSystem, format_length, parse_length};
use garden_editor::list::{
    Row, Section, Unit, cost, garden_list, price_from_display, price_to_display, quantity_display,
    section_label, to_csv, total, unit_label,
};
use garden_editor::print::Paper;
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use leptos::prelude::*;

use crate::app::now;
use crate::icons::{Icon, icon};

/// Whether the list card is open.
#[derive(Clone, Copy)]
pub struct ListOpen(pub RwSignal<bool>);

/// Gravel depth used when nothing else is set (mm).
const DEFAULT_GRAVEL_DEPTH: f64 = 50.0;

/// A money amount with two decimals and the currency, e.g. "€ 12.50".
fn money(amount: f64, currency: &str) -> String {
    if currency.is_empty() {
        format!("{amount:.2}")
    } else {
        format!("{currency} {amount:.2}")
    }
}

/// Price as typed: blank clears it; a comma works as the decimal point.
fn parse_price(text: &str) -> Result<Option<f64>, ()> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    match t.replace(',', ".").parse::<f64>() {
        Ok(p) if p.is_finite() && p >= 0.0 => Ok(Some(p)),
        _ => Err(()),
    }
}

/// "per m²", "per ft²", "each".
fn per_unit(unit: Unit, units: UnitSystem) -> String {
    match unit {
        Unit::Each => EN.list_each.to_string(),
        u => format!("{} {}", EN.list_per, unit_label(u, units)),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The list as a printable table.
fn print_list(
    name: &str,
    rows: &[Row],
    prices: &BTreeMap<String, f64>,
    units: UnitSystem,
    currency: &str,
) {
    let date =
        js_sys::Date::new_0().to_locale_date_string("default", &wasm_bindgen::JsValue::UNDEFINED);
    let priced = total(rows, prices).is_some();
    let mut body = String::new();
    let mut section = None;
    for r in rows {
        if section != Some(r.section) {
            section = Some(r.section);
            body += &format!(
                r#"<tr class="list-section"><th colspan="{}">{}</th></tr>"#,
                if priced { 4 } else { 2 },
                escape(section_label(r.section))
            );
        }
        let detail = if r.detail.is_empty() {
            String::new()
        } else {
            format!(r#"<small>{}</small>"#, escape(&r.detail))
        };
        let money_cells = if priced {
            let price = prices
                .get(&r.key)
                .filter(|_| r.priceable)
                .map(|p| {
                    format!(
                        "{} {}",
                        money(price_to_display(*p, r.unit, units), currency),
                        escape(&per_unit(r.unit, units))
                    )
                })
                .unwrap_or_default();
            let c = cost(r, prices)
                .map(|c| money(c, currency))
                .unwrap_or_default();
            format!("<td>{price}</td><td class=\"num\">{c}</td>")
        } else {
            String::new()
        };
        body += &format!(
            r#"<tr><td>{}{detail}</td><td class="num">{}</td>{money_cells}</tr>"#,
            escape(&r.label),
            escape(&quantity_display(r, units)),
        );
    }
    let foot = total(rows, prices)
        .map(|t| {
            format!(
                r#"<tfoot><tr><th colspan="3">{}</th><th class="num">{}</th></tr></tfoot>"#,
                EN.list_total,
                money(t, currency)
            )
        })
        .unwrap_or_default();
    let html = format!(
        r#"<section class="print-page print-list"><header class="print-title"><strong>{name}</strong><span>{list}</span><span>{date}</span></header><table>{body}{foot}</table></section>"#,
        name = escape(name),
        list = EN.list_title,
        date = String::from(date),
    );
    crate::print::show_and_print(&html, Paper::A4, false);
}

/// Card listing what the design needs.
#[component]
pub fn ListCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ListOpen(open) = expect_context::<ListOpen>();
    let depth = RwSignal::new(DEFAULT_GRAVEL_DEPTH);
    let depth_error = RwSignal::new(false);
    // a price that couldn't be read, by row key
    let bad_price = RwSignal::new(None::<String>);

    let units = Memo::new(move |_| state.with(|s| s.doc().units));
    let rows = Memo::new(move |_| {
        if !open.get() {
            return Vec::new();
        }
        state.with(|s| garden_list(s.doc(), depth.get()))
    });
    let prices = Memo::new(move |_| state.with(|s| s.doc().prices.clone()));
    let currency = Memo::new(move |_| state.with(|s| s.doc().currency.clone()));
    let has_gravel = move || rows.with(|r| r.iter().any(|r| r.key == "gravel"));
    let sum = move || total(&rows.get(), &prices.get());

    let export_csv = move |_| {
        let (name, csv) = state.with_untracked(|s| {
            let d = s.doc();
            let csv = to_csv(&rows.get_untracked(), &d.prices, d.units, &d.currency);
            (d.name.clone(), csv)
        });
        // the byte-order mark lets spreadsheets read "m²" and "€" correctly
        crate::files::download(
            &format!("{name} list.csv"),
            "text/csv;charset=utf-8",
            crate::files::Download::Text(format!("\u{feff}{csv}")),
        );
    };
    let print = move |_| {
        state.with_untracked(|s| {
            let d = s.doc();
            print_list(
                &d.name,
                &rows.get_untracked(),
                &d.prices,
                d.units,
                &d.currency,
            );
        });
    };

    let row_view = move |r: Row| {
        let key = r.key.clone();
        let unit = r.unit;
        let priceable = r.priceable;
        let shown_price = {
            let key = key.clone();
            move || {
                prices
                    .with(|p| p.get(&key).copied())
                    .map(|p| {
                        let v = price_to_display(p, unit, units.get());
                        // enough decimals that re-typing what's shown keeps the cost
                        let t = format!("{v:.4}");
                        t.trim_end_matches('0').trim_end_matches('.').to_string()
                    })
                    .unwrap_or_default()
            }
        };
        let on_price = {
            let key = key.clone();
            move |e: leptos::ev::Event| match parse_price(&event_target_value(&e)) {
                Ok(p) => {
                    bad_price.set(None);
                    let per_metric = p.map(|p| price_from_display(p, unit, units.get_untracked()));
                    state.update(|s| s.set_price(&key, per_metric, now()));
                }
                Err(()) => bad_price.set(Some(key.clone())),
            }
        };
        let row_cost = {
            let r = r.clone();
            move || cost(&r, &prices.get()).map(|c| money(c, &currency.get()))
        };
        let invalid = {
            let key = key.clone();
            move || bad_price.with(|b| b.as_deref() == Some(key.as_str()))
        };
        view! {
            <li class="list-row">
                <div class="list-item">
                    <span class="list-label">{r.label.clone()}</span>
                    {(!r.detail.is_empty()).then(|| view! { <small>{r.detail.clone()}</small> })}
                </div>
                <span class="list-qty">{move || quantity_display(&r, units.get())}</span>
                {priceable.then(|| view! {
                    <label class="list-price" class:invalid=invalid>
                        <input type="text" inputmode="decimal" placeholder=EN.list_price
                            aria-label=format!("{} ({})", EN.list_price, per_unit(unit, units.get_untracked()))
                            prop:value=shown_price on:change=on_price />
                        <small>{move || per_unit(unit, units.get())}</small>
                    </label>
                    <span class="list-cost">{row_cost}</span>
                })}
            </li>
        }
    };

    let sections = move || {
        let mut groups: Vec<(Section, Vec<Row>)> = Vec::new();
        for r in rows.get() {
            match groups.last_mut() {
                Some((s, v)) if *s == r.section => v.push(r),
                _ => groups.push((r.section, vec![r])),
            }
        }
        groups
            .into_iter()
            .map(|(section, rs)| {
                view! {
                    <h3 class="list-heading">{section_label(section)}</h3>
                    <ul class="list-rows">{rs.into_iter().map(row_view).collect_view()}</ul>
                }
            })
            .collect_view()
    };

    view! {
        <Show when=move || open.get()>
            <section class="card list-card" aria-label=EN.list_title>
                <header class="trace-head">
                    <h2>{EN.list_title}</h2>
                    <button class="icon-btn" aria-label=EN.close on:click=move |_| open.set(false)>
                        {icon(Icon::Close)}
                    </button>
                </header>
                <Show when=move || !rows.with(Vec::is_empty)
                    fallback=|| view! { <p class="print-summary">{EN.list_empty}</p> }>
                    <div class="list-settings">
                        <label class="print-row">
                            <span>{EN.list_currency}</span>
                            <input class="list-currency" type="text" maxlength="8" placeholder="€, $, £, ₼…"
                                prop:value=move || currency.get()
                                on:change=move |e| {
                                    let c = event_target_value(&e);
                                    state.update(|s| s.set_currency(&c, now()));
                                } />
                        </label>
                        <Show when=has_gravel>
                            <label class="print-row" class:invalid=move || depth_error.get()>
                                <span>{EN.list_gravel_depth}</span>
                                <input class="list-depth" type="text"
                                    prop:value=move || format_length(depth.get(), units.get())
                                    on:change=move |e| match parse_length(&event_target_value(&e), units.get_untracked()) {
                                        Ok(mm) if mm > 0.0 => {
                                            depth_error.set(false);
                                            depth.set(mm);
                                        }
                                        _ => depth_error.set(true),
                                    } />
                            </label>
                        </Show>
                    </div>
                    <div class="list-body">{sections}</div>
                    {move || bad_price.get().map(|_| view! { <p class="file-message" role="alert">{EN.list_bad_price}</p> })}
                    <p class="list-total">
                        {move || sum().map(|t| format!("{} {}", EN.list_total, money(t, &currency.get())))
                            .unwrap_or_else(|| EN.list_price_hint.to_string())}
                    </p>
                    <div class="list-actions">
                        <button class="trace-secondary" on:click=export_csv>{EN.list_csv}</button>
                        <button class="trace-primary" on:click=print>{EN.print}</button>
                    </div>
                </Show>
            </section>
        </Show>
    }
}
