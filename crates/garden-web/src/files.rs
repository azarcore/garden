//! Files in and out: download, pick a file to import, and export the plan
//! as SVG or PNG.

use js_sys::{Array, Promise};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, CanvasRenderingContext2d, CssStyleSheet, Element, HtmlAnchorElement,
    HtmlCanvasElement, HtmlImageElement, HtmlInputElement, Url, XmlSerializer,
};

const SVG_NS: &str = "http://www.w3.org/2000/svg";

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

pub enum Download {
    Text(String),
    Blob(Blob),
}

/// Offer `content` to the person as a file called `file_name`.
pub fn download(file_name: &str, mime: &str, content: Download) {
    let blob = match content {
        Download::Blob(b) => Some(b),
        Download::Text(text) => {
            let parts = Array::of1(&JsValue::from_str(&text));
            let options = BlobPropertyBag::new();
            options.set_type(mime);
            Blob::new_with_str_sequence_and_options(&parts, &options).ok()
        }
    };
    let (Some(blob), Some(doc)) = (blob, document()) else {
        return;
    };
    let Ok(url) = Url::create_object_url_with_blob(&blob) else {
        return;
    };
    if let Ok(a) = doc
        .create_element("a")
        .map(|e| e.unchecked_into::<HtmlAnchorElement>())
    {
        a.set_href(&url);
        a.set_download(file_name);
        a.click();
    }
    // give the browser a moment to start the download before releasing the URL
    let release = Closure::once_into_js(move || {
        let _ = Url::revoke_object_url(&url);
    });
    if let Some(w) = web_sys::window() {
        let _ =
            w.set_timeout_with_callback_and_timeout_and_arguments_0(release.unchecked_ref(), 5_000);
    }
}

/// Ask the person to choose a file; resolves to its text, or `None` if cancelled.
pub async fn pick_file(accept: &str) -> Option<String> {
    let doc = document()?;
    let input: HtmlInputElement = doc.create_element("input").ok()?.unchecked_into();
    input.set_type("file");
    input.set_accept(accept);
    let chosen = Promise::new(&mut |resolve, _reject| {
        let target = input.clone();
        let on_change = Closure::once_into_js(move || {
            let file = target.files().and_then(|f| f.get(0));
            match file {
                Some(f) => {
                    let _ = resolve.call1(&JsValue::NULL, &f.text());
                }
                None => {
                    let _ = resolve.call1(&JsValue::NULL, &JsValue::NULL);
                }
            }
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
    });
    input.click();
    let text_promise = JsFuture::from(chosen).await.ok()?;
    if text_promise.is_null() {
        return None;
    }
    let text = JsFuture::from(text_promise.dyn_into::<Promise>().ok()?)
        .await
        .ok()?;
    text.as_string()
}

/// All CSS rules the page can read (cross-origin sheets such as web fonts are skipped).
/// Styles inside exported or printed copies of the plan are skipped too: they
/// are copies of the page's CSS, and copying them again would grow every print.
fn page_css(doc: &web_sys::Document) -> String {
    let mut css = String::new();
    let sheets = doc.style_sheets();
    for i in 0..sheets.length() {
        let Some(sheet) = sheets
            .item(i)
            .and_then(|s| s.dyn_into::<CssStyleSheet>().ok())
        else {
            continue;
        };
        let copied = sheet
            .owner_node()
            .and_then(|n| n.dyn_into::<Element>().ok())
            .is_some_and(|el| matches!(el.closest("#print-root, svg"), Ok(Some(_))));
        if copied {
            continue;
        }
        let Ok(rules) = sheet.css_rules() else {
            continue;
        };
        for j in 0..rules.length() {
            if let Some(rule) = rules.item(j) {
                css.push_str(&rule.css_text());
                css.push('\n');
            }
        }
    }
    css
}

/// A standalone SVG of the plan: no grid, previews or selection, light colours,
/// framed on `world` (mm) at 1 px = 20 mm.
/// With `size_mm`, the SVG is sized in millimetres (for printing at scale).
pub fn plan_svg(world: kurbo::Rect, size_mm: Option<(f64, f64)>) -> Option<(String, f64, f64)> {
    let doc = document()?;
    let live = doc.query_selector("svg.plan").ok()??;
    let svg: Element = live.clone_node_with_deep(true).ok()?.unchecked_into();
    for selector in [
        "pattern#dot-grid",
        "rect[fill='url(#dot-grid)']",
        ".preview",
        ".tag",
        ".snap",
        ".marquee",
        ".selection",
        ".trace-photo",
    ] {
        if let Ok(nodes) = svg.query_selector_all(selector) {
            for i in 0..nodes.length() {
                if let Some(n) = nodes.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
                    n.remove();
                }
            }
        }
    }
    if let Ok(Some(g)) = svg.query_selector("g[transform]") {
        let _ = g.remove_attribute("transform");
    }
    let (w, h) = (world.width() / 20.0, world.height() / 20.0);
    let _ = svg.set_attribute("xmlns", SVG_NS);
    let _ = svg.set_attribute(
        "viewBox",
        &format!(
            "{} {} {} {}",
            world.x0,
            world.y0,
            world.width(),
            world.height()
        ),
    );
    let (width, height) = match size_mm {
        Some((mw, mh)) => (format!("{mw:.2}mm"), format!("{mh:.2}mm")),
        None => (format!("{w:.0}"), format!("{h:.0}")),
    };
    let _ = svg.set_attribute("width", &width);
    let _ = svg.set_attribute("height", &height);
    let _ = svg.set_attribute("data-theme", "light");
    let _ = svg.set_attribute("style", "background:#ffffff");
    let style = doc.create_element_ns(Some(SVG_NS), "style").ok()?;
    style.set_text_content(Some(&page_css(&doc)));
    let _ = svg.prepend_with_node_1(&style);
    let text = XmlSerializer::new().ok()?.serialize_to_string(&svg).ok()?;
    Some((text, w, h))
}

/// Rasterise an SVG string to a PNG `width_px` wide.
pub async fn svg_to_png(svg: &str, width_px: f64, aspect: f64) -> Option<Blob> {
    let doc = document()?;
    let parts = Array::of1(&JsValue::from_str(svg));
    let options = BlobPropertyBag::new();
    options.set_type("image/svg+xml");
    let blob = Blob::new_with_str_sequence_and_options(&parts, &options).ok()?;
    let url = Url::create_object_url_with_blob(&blob).ok()?;
    let img = HtmlImageElement::new().ok()?;
    let loaded = Promise::new(&mut |resolve, reject| {
        img.set_onload(Some(&resolve));
        img.set_onerror(Some(&reject));
    });
    img.set_src(&url);
    let ok = JsFuture::from(loaded).await.is_ok();
    let _ = Url::revoke_object_url(&url);
    if !ok {
        return None;
    }
    let height = (width_px * aspect).round();
    let canvas: HtmlCanvasElement = doc.create_element("canvas").ok()?.unchecked_into();
    canvas.set_width(width_px as u32);
    canvas.set_height(height as u32);
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d").ok()??.unchecked_into();
    ctx.set_fill_style_str("#ffffff");
    ctx.fill_rect(0.0, 0.0, width_px, height);
    ctx.draw_image_with_html_image_element_and_dw_and_dh(&img, 0.0, 0.0, width_px, height)
        .ok()?;
    let png = Promise::new(&mut |resolve, _reject| {
        let cb = Closure::once_into_js(move |b: JsValue| {
            let _ = resolve.call1(&JsValue::NULL, &b);
        });
        let _ = canvas.to_blob(cb.unchecked_ref());
    });
    JsFuture::from(png).await.ok()?.dyn_into::<Blob>().ok()
}
