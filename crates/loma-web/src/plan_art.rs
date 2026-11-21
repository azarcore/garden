//! How the plan is drawn: a landscape architect's planting plan on vellum.
//! Plants are canopy symbols, surfaces carry textures, the plot boundary is a
//! property line. Everything here is world-space SVG (millimetres).

use std::f64::consts::TAU;
use std::fmt::Write as _;

use leptos::prelude::*;
use loma_core::geometry::Point;
use loma_core::model::{Color, PlantCategory, PlantSymbol};
use loma_editor::render::{RenderItem, RenderShape, color_css};
use loma_editor::tools::Preview;

const HAIRLINE_PX: f64 = 1.25;
const LABEL_PX: f64 = 12.0;

fn css(c: Option<Color>) -> String {
    c.map(color_css).unwrap_or_else(|| "none".to_string())
}

fn polar(c: Point, r: f64, a: f64) -> Point {
    Point::new(c.x + r * a.cos(), c.y + r * a.sin())
}

/// A deterministic small wobble so symbols look hand-drawn but stay stable.
fn wobble(seed: f64, i: usize) -> f64 {
    let x = ((seed * 0.000_37 + i as f64 * 12.9898).sin() * 43_758.545_3).fract();
    x.abs()
}

/// Outline path of a plant canopy symbol.
pub fn canopy_path(center: Point, r: f64, symbol: PlantSymbol) -> String {
    let seed = center.x * 3.1 + center.y * 1.7;
    let mut d = String::new();
    match symbol {
        PlantSymbol::Round | PlantSymbol::Leafy => {
            // scalloped outline: lobes bulge outwards
            let lobes = if symbol == PlantSymbol::Leafy { 7 } else { 11 };
            let inner = if symbol == PlantSymbol::Leafy {
                0.8
            } else {
                0.88
            };
            let start = wobble(seed, 0) * TAU;
            for i in 0..=lobes {
                let a = start + i as f64 / lobes as f64 * TAU;
                let p = polar(center, r * inner, a);
                if i == 0 {
                    let _ = write!(d, "M{:.1} {:.1}", p.x, p.y);
                } else {
                    let mid = a - TAU / lobes as f64 / 2.0;
                    let bulge = r * (1.06 + 0.06 * wobble(seed, i));
                    let c = polar(center, bulge, mid);
                    let _ = write!(d, " Q{:.1} {:.1} {:.1} {:.1}", c.x, c.y, p.x, p.y);
                }
            }
            d.push('Z');
        }
        PlantSymbol::Spiky | PlantSymbol::Star => {
            let points = if symbol == PlantSymbol::Star { 5 } else { 13 };
            let inner = if symbol == PlantSymbol::Star {
                0.45
            } else {
                0.55
            };
            let start = wobble(seed, 0) * TAU;
            for i in 0..points * 2 {
                let a = start + i as f64 / (points * 2) as f64 * TAU;
                let rr = if i % 2 == 0 { r } else { r * inner };
                let p = polar(center, rr, a);
                let _ = write!(
                    d,
                    "{}{:.1} {:.1} ",
                    if i == 0 { "M" } else { "L" },
                    p.x,
                    p.y
                );
            }
            d.push('Z');
        }
        PlantSymbol::Conifer => {
            let _ = write!(
                d,
                "M{:.1} {:.1} A{r:.1} {r:.1} 0 1 0 {:.1} {:.1} A{r:.1} {r:.1} 0 1 0 {:.1} {:.1}Z",
                center.x + r,
                center.y,
                center.x - r,
                center.y,
                center.x + r,
                center.y
            );
        }
    }
    d
}

/// Inner marks of a symbol: branch spokes for conifers and trees, veins for leafy plants.
fn canopy_marks(center: Point, r: f64, symbol: PlantSymbol, category: PlantCategory) -> String {
    let seed = center.x * 1.3 + center.y * 2.9;
    let mut d = String::new();
    let spokes = match (symbol, category) {
        (PlantSymbol::Conifer, _) => 16,
        (_, PlantCategory::Tree | PlantCategory::FruitTree) => 6,
        (PlantSymbol::Leafy, _) => 3,
        _ => 0,
    };
    let reach = if symbol == PlantSymbol::Conifer {
        0.95
    } else {
        0.55
    };
    let start = wobble(seed, 1) * TAU;
    for i in 0..spokes {
        let a = start + i as f64 / spokes as f64 * TAU;
        let p = polar(center, r * reach, a);
        let _ = write!(d, "M{:.1} {:.1}L{:.1} {:.1}", center.x, center.y, p.x, p.y);
    }
    d
}

/// Gradients, materials and shadows shared by the plan (world millimetres).
pub fn defs() -> impl IntoView {
    view! {
        <defs>
            // plants: lit from the top-left
            <radialGradient id="pl-tree" cx="38%" cy="34%" r="70%">
                <stop offset="0" stop-color="#a3d077" /><stop offset=".62" stop-color="#5f9b47" /><stop offset="1" stop-color="#3b7231" />
            </radialGradient>
            <radialGradient id="pl-conifer" cx="38%" cy="34%" r="70%">
                <stop offset="0" stop-color="#6aa47c" /><stop offset="1" stop-color="#275a43" />
            </radialGradient>
            <radialGradient id="pl-shrub" cx="38%" cy="34%" r="70%">
                <stop offset="0" stop-color="#b0da84" /><stop offset="1" stop-color="#5e9842" />
            </radialGradient>
            <radialGradient id="pl-bed-plant" cx="40%" cy="36%" r="70%">
                <stop offset="0" stop-color="#9cc978" /><stop offset="1" stop-color="#5b8e45" />
            </radialGradient>
            <radialGradient id="pl-perennial" cx="40%" cy="36%" r="70%">
                <stop offset="0" stop-color="#cbb6f0" /><stop offset="1" stop-color="#8669bf" />
            </radialGradient>
            <radialGradient id="pl-annual" cx="40%" cy="36%" r="70%">
                <stop offset="0" stop-color="#f9b8a8" /><stop offset="1" stop-color="#df6f5a" />
            </radialGradient>
            <radialGradient id="pl-climber" cx="40%" cy="36%" r="70%">
                <stop offset="0" stop-color="#f3bad6" /><stop offset="1" stop-color="#c9679a" />
            </radialGradient>
            <radialGradient id="pl-grass" cx="40%" cy="35%" r="70%">
                <stop offset="0" stop-color="#ece09c" /><stop offset="1" stop-color="#b6a352" />
            </radialGradient>
            <radialGradient id="pl-herb" cx="40%" cy="35%" r="70%">
                <stop offset="0" stop-color="#c9d8c0" /><stop offset="1" stop-color="#7f9878" />
            </radialGradient>
            // surfaces
            <linearGradient id="m-lawn" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0" stop-color="#a2d070" /><stop offset="1" stop-color="#7db353" />
            </linearGradient>
            <pattern id="m-mow" width="1600" height="1600" patternUnits="userSpaceOnUse" patternTransform="rotate(-20)">
                <rect width="800" height="1600" fill="rgba(255,255,255,0.11)" />
            </pattern>
            <pattern id="m-mulch" width="500" height="500" patternUnits="userSpaceOnUse">
                <rect width="500" height="500" fill="#6e4b35" />
                <path d="M60 100l90 25M300 70l75-38M170 280l100 12M380 330l38 62M70 420l76-25M280 450l88 10M440 160l-34 60"
                    stroke="#4f3526" stroke-width="30" stroke-linecap="round" />
                <path d="M120 190l50 12M350 210l38-25M30 320l50 0M230 380l40 22" stroke="#8a654b" stroke-width="24"
                    stroke-linecap="round" />
            </pattern>
            <pattern id="m-stone" width="1200" height="800" patternUnits="userSpaceOnUse">
                <rect width="1200" height="800" fill="#e2dbce" />
                <rect x="20" y="20" width="560" height="360" rx="14" fill="#e9e3d7" />
                <rect x="620" y="20" width="560" height="360" rx="14" fill="#ddd5c6" />
                <rect x="320" y="420" width="560" height="360" rx="14" fill="#e6dfd2" />
                <rect x="-280" y="420" width="560" height="360" rx="14" fill="#d9d1c1" />
                <rect x="920" y="420" width="560" height="360" rx="14" fill="#d9d1c1" />
            </pattern>
            <pattern id="m-gravel" width="420" height="420" patternUnits="userSpaceOnUse">
                <rect width="420" height="420" fill="#e8e0cd" />
                <circle cx="50" cy="70" r="26" fill="#cfc4ac" /><circle cx="240" cy="40" r="20" fill="#d8ceb8" />
                <circle cx="350" cy="210" r="28" fill="#c9bea4" /><circle cx="160" cy="260" r="22" fill="#d4c9b2" />
                <circle cx="70" cy="370" r="18" fill="#cbbfa6" /><circle cx="300" cy="350" r="24" fill="#d6ccb6" />
            </pattern>
            <radialGradient id="m-water" cx="45%" cy="40%" r="65%">
                <stop offset="0" stop-color="#86cbe0" /><stop offset=".7" stop-color="#4396b8" /><stop offset="1" stop-color="#2c7495" />
            </radialGradient>
            <pattern id="m-roof" width="400" height="200" patternUnits="userSpaceOnUse">
                <rect width="400" height="200" fill="#a9784f" />
                <path d="M0 100H400" stroke="#8a5f3e" stroke-width="22" />
                <path d="M100 0V100M300 100V200" stroke="#94683f" stroke-width="10" />
            </pattern>
            <pattern id="m-other" width="400" height="400" patternUnits="userSpaceOnUse">
                <rect width="400" height="400" fill="#d6d6c8" />
            </pattern>
            <filter id="lift" x="-10%" y="-10%" width="120%" height="130%">
                <feDropShadow dx="0" dy="40" stdDeviation="40" flood-color="#1b2a12" flood-opacity="0.18" />
            </filter>
        </defs>
    }
}

fn material(class: &str) -> Option<&'static str> {
    Some(match class {
        "lawn" => "url(#m-lawn)",
        "bed" => "url(#m-mulch)",
        "paving" => "url(#m-stone)",
        "gravel" => "url(#m-gravel)",
        "water" => "url(#m-water)",
        "structure" => "url(#m-roof)",
        "area" => "url(#m-other)",
        _ => return None,
    })
}

fn plant_paint(category: PlantCategory, symbol: PlantSymbol) -> &'static str {
    match (category, symbol) {
        (_, PlantSymbol::Conifer) => "url(#pl-conifer)",
        (PlantCategory::Tree | PlantCategory::FruitTree, _) => "url(#pl-tree)",
        (PlantCategory::Shrub, _) => "url(#pl-shrub)",
        (PlantCategory::Grass, _) => "url(#pl-grass)",
        (PlantCategory::Herb, _) => "url(#pl-herb)",
        (PlantCategory::Perennial, _) => "url(#pl-perennial)",
        (PlantCategory::Annual, _) => "url(#pl-annual)",
        (PlantCategory::Climber, _) => "url(#pl-climber)",
        _ => "url(#pl-bed-plant)",
    }
}

/// Flower colour for plants that show blooms, if any.
fn bloom(category: PlantCategory) -> Option<(&'static str, &'static str)> {
    match category {
        PlantCategory::Perennial => Some(("#ede4fb", "#b79de6")),
        PlantCategory::Annual => Some(("#fde3dc", "#f29a86")),
        PlantCategory::Climber => Some(("#fbe1ee", "#e597bf")),
        _ => None,
    }
}

/// One drawn object. `mm_per_px` converts screen sizes to world sizes.
pub fn item_view(it: RenderItem, mm_per_px: f64) -> AnyView {
    let class = format!("obj {}", it.class);
    let main = match it.shape.clone() {
        RenderShape::Path { d, closed } => match it.stroke_width {
            // paths, fences, walls, hedges: a darker edge under a lighter body
            Some(w) => {
                let inner = (w - 4.0 * mm_per_px).max(w * 0.5);
                view! {
                    <g class=class>
                        <path d=d.clone() fill="none" class="edge" stroke-width=w stroke-linecap="round"
                            stroke-linejoin="round" />
                        <path d=d.clone() fill="none" class="body" stroke-width=inner stroke-linecap="round"
                            stroke-linejoin="round" />
                        <path d=d fill="none" class="detail" stroke-width=inner * 0.6 stroke-linecap="round"
                            stroke-linejoin="round" />
                    </g>
                }
                .into_any()
            }
            None => {
                let paint = material(it.class)
                    .map(str::to_string)
                    .unwrap_or_else(|| css(it.fill));
                let mow =
                    (it.class == "lawn").then(|| view! { <path d=d.clone() fill="url(#m-mow)" /> });
                let lift = matches!(
                    it.class,
                    "lawn" | "bed" | "paving" | "gravel" | "structure" | "area"
                );
                view! {
                    <g class=class>
                        <path d=d.clone() class="surface" fill=paint filter=lift.then_some("url(#lift)")
                            fill-rule=if closed { "nonzero" } else { "evenodd" } />
                        {mow}
                        <path d=d fill="none" class="rim" stroke-width=HAIRLINE_PX vector-effect="non-scaling-stroke"
                            stroke-linejoin="round" />
                    </g>
                }
                .into_any()
            }
        },
        RenderShape::Plant {
            center,
            r,
            symbol,
            category,
        } => {
            let outline = canopy_path(center, r, symbol);
            // soft shadow cast down-right; bigger canopies cast longer shadows
            let shadow = canopy_path(
                Point::new(center.x + r * 0.16, center.y + r * 0.22),
                r,
                symbol,
            );
            let blur = format!("filter: blur({:.0}px)", (r * 0.12).max(2.0 * mm_per_px));
            let shine = canopy_path(
                Point::new(center.x - r * 0.18, center.y - r * 0.2),
                r * 0.5,
                symbol,
            );
            let marks = canopy_marks(center, r, symbol, category);
            let blooms = bloom(category).map(|(dark, light)| {
                (0..9)
                    .map(|i| {
                        let a = i as f64 * 2.4 + center.x * 0.01;
                        let rr = r * (0.2 + 0.55 * ((i * 37 % 10) as f64) / 10.0);
                        let p = polar(center, rr, a);
                        let fill = if i % 3 == 0 { light } else { dark };
                        view! { <circle cx=p.x cy=p.y r=r * 0.13 fill=fill /> }
                    })
                    .collect_view()
            });
            view! {
                <g class=format!("{class} {}", symbol_class(symbol))>
                    <path d=shadow class="plant-shadow" style=blur />
                    <path d=outline fill=plant_paint(category, symbol) class="canopy" stroke-width="1"
                        vector-effect="non-scaling-stroke" stroke-linejoin="round" />
                    <path d=shine class="shine" />
                    <path d=marks class="marks" stroke-width="1" vector-effect="non-scaling-stroke" />
                    {blooms}
                </g>
            }
            .into_any()
        }
        RenderShape::Line { from, to } => view! {
            <line x1=from.x y1=from.y x2=to.x y2=to.y class=class stroke-width=HAIRLINE_PX
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
                <text x=at.x y=at.y font-size=size class=format!("{class} lettering") text-anchor="middle"
                    dominant-baseline="middle">{text}</text>
            }
            .into_any()
        }
    };
    if !it.selected {
        return main;
    }
    let highlight = highlight_view(&it.shape, mm_per_px, "selection");
    view! { <g>{main}{highlight}</g> }.into_any()
}

fn symbol_class(s: PlantSymbol) -> &'static str {
    match s {
        PlantSymbol::Round => "sym-round",
        PlantSymbol::Leafy => "sym-leafy",
        PlantSymbol::Spiky => "sym-spiky",
        PlantSymbol::Star => "sym-star",
        PlantSymbol::Conifer => "sym-conifer",
    }
}

/// Outline used for selection highlights and drawing previews.
pub fn highlight_view(shape: &RenderShape, mm_per_px: f64, class: &'static str) -> AnyView {
    match shape.clone() {
        RenderShape::Path { d, closed } => {
            // open lines (paths, fences) must not look like filled areas
            let class = if closed { class.to_string() } else { format!("{class} open") };
            view! { <path d=d class=class vector-effect="non-scaling-stroke" /> }.into_any()
        }
        RenderShape::Plant { center, r, .. } => view! {
            <circle cx=center.x cy=center.y r=r + 2.0 * mm_per_px class=class vector-effect="non-scaling-stroke" />
        }
        .into_any(),
        RenderShape::Line { from, to } => view! {
            <line x1=from.x y1=from.y x2=to.x y2=to.y class=class vector-effect="non-scaling-stroke" />
        }
        .into_any(),
        RenderShape::Text { at, .. } => {
            let r = 6.0 * mm_per_px;
            view! { <circle cx=at.x cy=at.y r=r class=class vector-effect="non-scaling-stroke" /> }.into_any()
        }
    }
}

pub fn preview_view(p: Preview, mm_per_px: f64) -> impl IntoView {
    let shapes = p
        .shapes
        .iter()
        .map(|s| highlight_view(s, mm_per_px, "preview"))
        .collect_view();
    let dims = p
        .dims
        .into_iter()
        .map(|(at, text)| {
            view! {
                <text x=at.x y=at.y - 10.0 * mm_per_px font-size=LABEL_PX * mm_per_px text-anchor="middle"
                    dominant-baseline="auto" class="dim">{text}</text>
            }
        })
        .collect_view();
    let snap = p.snap.map(|s| {
        let r = 5.0 * mm_per_px;
        view! {
            <g class="snap">
                <circle cx=s.point.x cy=s.point.y r=r vector-effect="non-scaling-stroke" />
                <path d=format!("M{} {}h{}M{} {}v{}", s.point.x - 2.0 * r, s.point.y, 4.0 * r, s.point.x,
                    s.point.y - 2.0 * r, 4.0 * r) vector-effect="non-scaling-stroke" />
            </g>
        }
    });
    let marquee = p.marquee.map(|r| {
        view! {
            <rect x=r.x0 y=r.y0 width=r.width() height=r.height() class="marquee"
                vector-effect="non-scaling-stroke" />
        }
    });
    let guides = p
        .guides
        .into_iter()
        .map(|g| guide_view(g, mm_per_px))
        .collect_view();
    view! { {shapes}{dims}{snap}{marquee}<g class="guides">{guides}</g> }
}

/// A smart guide: a thin coral line; measured gaps get end ticks and a label,
/// equal gaps a tick across the middle.
fn guide_view(g: loma_editor::guides::GuideLine, mm_per_px: f64) -> AnyView {
    use loma_editor::guides::GuideKind;
    let (a, b) = (g.from, g.to);
    let d = b - a;
    let len = d.hypot();
    if len < 1e-9 {
        return ().into_any();
    }
    // unit vector across the line, for ticks
    let n = kurbo::Vec2::new(-d.y / len, d.x / len) * (4.0 * mm_per_px);
    let tick = |p: Point| format!("M{} {}L{} {}", p.x - n.x, p.y - n.y, p.x + n.x, p.y + n.y);
    let class = match g.kind {
        GuideKind::Align => "guide align",
        GuideKind::Distance => "guide distance",
        GuideKind::Equal => "guide equal",
    };
    let mid = a.midpoint(b);
    let ticks = match g.kind {
        GuideKind::Align => String::new(),
        GuideKind::Distance => format!("{}{}", tick(a), tick(b)),
        GuideKind::Equal => format!("{}{}{}", tick(a), tick(b), tick(mid)),
    };
    let label = g.label.map(|text| {
        let h = 16.0 * mm_per_px;
        let w = (text.chars().count() as f64 * 6.6 + 10.0) * mm_per_px;
        view! {
            <g class="guide-label">
                <rect x=mid.x - w / 2.0 y=mid.y - h / 2.0 width=w height=h rx=h / 2.0 />
                <text x=mid.x y=mid.y font-size=11.0 * mm_per_px text-anchor="middle"
                    dominant-baseline="central">{text}</text>
            </g>
        }
    });
    view! {
        <g class=class>
            <line x1=a.x y1=a.y x2=b.x y2=b.y vector-effect="non-scaling-stroke" />
            <path d=ticks vector-effect="non-scaling-stroke" />
            {label}
        </g>
    }
    .into_any()
}

/// Small stand-alone plant symbol for the picker (viewBox centred on 0,0).
pub fn plant_swatch(symbol: PlantSymbol, category: PlantCategory) -> impl IntoView {
    let r = 40.0;
    let c = Point::ZERO;
    let outline = canopy_path(c, r, symbol);
    let marks = canopy_marks(c, r, symbol, category);
    let class = format!(
        "swatch {} cat-{}",
        symbol_class(symbol),
        category_key(category)
    );
    view! {
        <svg viewBox="-48 -48 96 96" class=class aria-hidden="true">
            <path d=outline class="swatch-fill" />
            <path d=marks class="swatch-marks" />
            <circle cx="0" cy="0" r="3" class="swatch-dot" />
        </svg>
    }
}

pub fn category_key(c: PlantCategory) -> &'static str {
    match c {
        PlantCategory::Tree => "tree",
        PlantCategory::FruitTree => "fruit-tree",
        PlantCategory::Shrub => "shrub",
        PlantCategory::Perennial => "perennial",
        PlantCategory::Annual => "annual",
        PlantCategory::Grass => "grass",
        PlantCategory::Climber => "climber",
        PlantCategory::Vegetable => "vegetable",
        PlantCategory::Herb => "herb",
        PlantCategory::Other => "other",
    }
}
