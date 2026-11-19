//! How the plan is drawn: a landscape architect's planting plan on vellum.
//! Plants are canopy symbols, surfaces carry textures, the plot boundary is a
//! property line. Everything here is world-space SVG (millimetres).

use std::f64::consts::TAU;
use std::fmt::Write as _;

use garden_core::geometry::Point;
use garden_core::model::{Color, PlantCategory, PlantSymbol};
use garden_editor::render::{RenderItem, RenderShape, color_css};
use garden_editor::tools::Preview;
use leptos::prelude::*;

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
        (_, PlantCategory::Tree) => 6,
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

/// Shared texture patterns, in world millimetres.
pub fn defs() -> impl IntoView {
    view! {
        <defs>
            <pattern id="tex-lawn" width="600" height="600" patternUnits="userSpaceOnUse">
                <path d="M70 120l18-48M104 126l8-42M380 410l18-48M414 416l8-42M230 560l16-44M520 180l17-46M554 186l7-40M300 260l15-40"
                    class="tex-stroke lawn-blade" />
            </pattern>
            <pattern id="tex-bed" width="480" height="480" patternUnits="userSpaceOnUse">
                <path d="M50 70l70 22M280 50l54-28M160 240l84 14M380 280l32 46M70 390l64-26M300 420l76 8M420 130l-30 50"
                    class="tex-stroke bed-mulch" />
            </pattern>
            <pattern id="tex-paving" width="900" height="900" patternUnits="userSpaceOnUse">
                <path d="M0 0H900M0 450H900M0 0V450M450 450V900" class="tex-stroke paving-joint" />
            </pattern>
            <pattern id="tex-gravel" width="420" height="420" patternUnits="userSpaceOnUse">
                <circle cx="50" cy="70" r="22" class="gravel-stone" />
                <circle cx="240" cy="40" r="16" class="gravel-stone" />
                <circle cx="350" cy="210" r="24" class="gravel-stone" />
                <circle cx="160" cy="260" r="18" class="gravel-stone" />
                <circle cx="70" cy="370" r="14" class="gravel-stone" />
                <circle cx="300" cy="350" r="20" class="gravel-stone" />
            </pattern>
            <pattern id="tex-water" width="1400" height="700" patternUnits="userSpaceOnUse">
                <path d="M100 200q140-80 280 0t280 0M760 540q140-80 280 0t280 0" class="tex-stroke water-ripple" />
            </pattern>
            <pattern id="tex-roof" width="360" height="360" patternUnits="userSpaceOnUse"
                patternTransform="rotate(45)">
                <path d="M0 0V360" class="tex-stroke roof-hatch" />
            </pattern>
        </defs>
    }
}

fn texture_for(class: &str) -> Option<&'static str> {
    Some(match class {
        "lawn" => "url(#tex-lawn)",
        "bed" => "url(#tex-bed)",
        "paving" => "url(#tex-paving)",
        "gravel" => "url(#tex-gravel)",
        "water" => "url(#tex-water)",
        "structure" => "url(#tex-roof)",
        _ => return None,
    })
}

/// One drawn object. `mm_per_px` converts screen sizes to world sizes.
pub fn item_view(it: RenderItem, mm_per_px: f64) -> AnyView {
    let fill = css(it.fill);
    let stroke = css(it.stroke);
    let class = format!("obj {}", it.class);
    let main = match it.shape.clone() {
        RenderShape::Path { d, .. } => match it.stroke_width {
            // paths, fences, walls, hedges: world-width strokes drawn twice for an edge
            Some(w) => {
                let inner = (w - 3.0 * mm_per_px).max(w * 0.4);
                view! {
                    <g class=class>
                        <path d=d.clone() fill="none" class="edge" stroke-width=w
                            stroke-linecap="round" stroke-linejoin="round" />
                        <path d=d fill="none" stroke=stroke stroke-width=inner class="body"
                            stroke-linecap="round" stroke-linejoin="round" />
                    </g>
                }
                .into_any()
            }
            None => {
                let texture = texture_for(it.class);
                view! {
                    <g class=class>
                        <path d=d.clone() fill=fill stroke=stroke stroke-width=HAIRLINE_PX
                            vector-effect="non-scaling-stroke" stroke-linejoin="round" />
                        {texture.map(|t| view! { <path d=d fill=t stroke="none" /> })}
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
            let marks = canopy_marks(center, r, symbol, category);
            let dot = (r * 0.06).max(1.5 * mm_per_px);
            view! {
                <g class=format!("{class} {}", symbol_class(symbol))>
                    <path d=outline fill=fill stroke=stroke.clone() stroke-width=HAIRLINE_PX
                        vector-effect="non-scaling-stroke" stroke-linejoin="round" />
                    <path d=marks fill="none" stroke=stroke.clone() stroke-width="0.75"
                        vector-effect="non-scaling-stroke" opacity="0.6" />
                    <circle cx=center.x cy=center.y r=dot fill=stroke />
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
        RenderShape::Path { d, .. } => view! { <path d=d class=class vector-effect="non-scaling-stroke" /> }.into_any(),
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
    view! { {shapes}{dims}{snap}{marquee} }
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
