//! Printing to scale: which paper orientation and scale a plan fits.

/// Paper sizes we print on (portrait width × height in mm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    A4,
    Letter,
}

impl Paper {
    pub fn size_mm(self) -> (f64, f64) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::Letter => (215.9, 279.4),
        }
    }
}

/// Blank border kept around the printed plan.
pub const PRINT_MARGIN_MM: f64 = 12.0;
/// Height kept free for the title line above and the scale bar below the plan.
pub const PRINT_FURNITURE_MM: f64 = 20.0;

/// Scales offered (1:n), most detailed first.
pub const STANDARD_SCALES: [u32; 8] = [20, 25, 50, 100, 200, 250, 500, 1000];

/// How a plan lands on the page: 1:`scale`, orientation, and printed size in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub scale: f64,
    pub landscape: bool,
    pub width_mm: f64,
    pub height_mm: f64,
}

/// Area left for the plan itself (width, height) for an orientation.
fn printable(paper: Paper, landscape: bool) -> (f64, f64) {
    let (w, h) = paper.size_mm();
    let (w, h) = if landscape { (h, w) } else { (w, h) };
    (
        w - 2.0 * PRINT_MARGIN_MM,
        h - 2.0 * PRINT_MARGIN_MM - PRINT_FURNITURE_MM,
    )
}

/// Lay out `world` (mm) at 1:`scale` on `paper` inside the margins, picking the
/// orientation that fits (if both fit, landscape for plans wider than tall).
pub fn layout_at(world: kurbo::Rect, paper: Paper, scale: u32) -> Option<Layout> {
    let s = f64::from(scale.max(1));
    let (w, h) = (world.width() / s, world.height() / s);
    let fits = |landscape| {
        let (aw, ah) = printable(paper, landscape);
        w <= aw + 1e-9 && h <= ah + 1e-9
    };
    let landscape = match (fits(false), fits(true)) {
        (true, true) => w > h,
        (false, true) => true,
        (true, false) => false,
        (false, false) => return None,
    };
    Some(Layout {
        scale: s,
        landscape,
        width_mm: w,
        height_mm: h,
    })
}

/// The most detailed standard scale that fits, if any.
pub fn best_standard_scale(world: kurbo::Rect, paper: Paper) -> Option<u32> {
    STANDARD_SCALES
        .into_iter()
        .find(|s| layout_at(world, paper, *s).is_some())
}

/// The exact scale that fills the page ("fit to page").
pub fn fit_to_page(world: kurbo::Rect, paper: Paper) -> Layout {
    let (ww, wh) = (world.width().max(1.0), world.height().max(1.0));
    let best = [false, true]
        .into_iter()
        .map(|landscape| {
            let (aw, ah) = printable(paper, landscape);
            (landscape, (aw / ww).min(ah / wh))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((false, 1.0));
    let (landscape, k) = best;
    Layout {
        scale: 1.0 / k,
        landscape,
        width_mm: ww * k,
        height_mm: wh * k,
    }
}

/// How much neighbouring pages overlap on paper (mm).
pub const TILE_OVERLAP_MM: f64 = 10.0;
/// The most plan pages one print may take.
pub const MAX_TILES: usize = 60;

/// One sheet of a multi-page print.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub col: usize,
    pub row: usize,
    /// Column letter and row number, e.g. "B2".
    pub label: String,
    /// The world area printed on this sheet (mm), overlap included.
    pub world: kurbo::Rect,
}

/// A plan split into sheets at one scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Tiling {
    pub scale: f64,
    pub landscape: bool,
    pub cols: usize,
    pub rows: usize,
    /// Left to right, top to bottom.
    pub tiles: Vec<Tile>,
    /// The printable area each sheet uses, on paper (mm).
    pub page_mm: (f64, f64),
}

/// A, B, … Z, AA, AB, …
pub fn column_label(col: usize) -> String {
    let mut n = col + 1;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Sheets needed along one axis: the first covers `page`, each next one adds `page - overlap`.
fn along(length: f64, page: f64, overlap: f64) -> usize {
    let step = page - overlap;
    let rest = (length - page - 1e-6).max(0.0);
    1 + (rest / step).ceil() as usize
}

/// (cols, rows, printable page on paper) for one orientation.
fn grid(
    world: kurbo::Rect,
    paper: Paper,
    scale: f64,
    landscape: bool,
) -> (usize, usize, (f64, f64)) {
    let page = printable(paper, landscape);
    let overlap = TILE_OVERLAP_MM * scale;
    (
        along(world.width(), page.0 * scale, overlap),
        along(world.height(), page.1 * scale, overlap),
        page,
    )
}

/// Pages needed in one orientation.
fn count(world: kurbo::Rect, paper: Paper, scale: f64, landscape: bool) -> usize {
    let (c, r, _) = grid(world, paper, scale, landscape);
    c.saturating_mul(r)
}

/// Split `world` into sheets at 1:`scale` in the orientation that needs fewer
/// of them. None if the bounds aren't usable or it would take more than
/// `MAX_TILES` sheets.
pub fn tile(world: kurbo::Rect, paper: Paper, scale: u32) -> Option<Tiling> {
    let finite = [world.x0, world.y0, world.x1, world.y1]
        .iter()
        .all(|v| v.is_finite());
    if !finite {
        return None;
    }
    let world = world.abs();
    let s = f64::from(scale.max(1));
    let (portrait, landscape) = (count(world, paper, s, false), count(world, paper, s, true));
    let use_landscape = match landscape.cmp(&portrait) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => world.width() > world.height(),
    };
    let (cols, rows, page_mm) = grid(world, paper, s, use_landscape);
    if cols.saturating_mul(rows) > MAX_TILES {
        return None;
    }
    let (pw, ph) = (page_mm.0 * s, page_mm.1 * s);
    let (step_w, step_h) = (pw - TILE_OVERLAP_MM * s, ph - TILE_OVERLAP_MM * s);
    let mut tiles = Vec::with_capacity(cols * rows);
    for row in 0..rows {
        for col in 0..cols {
            let x0 = world.x0 + col as f64 * step_w;
            let y0 = world.y0 + row as f64 * step_h;
            tiles.push(Tile {
                col,
                row,
                label: format!("{}{}", column_label(col), row + 1),
                world: kurbo::Rect::new(x0, y0, x0 + pw, y0 + ph),
            });
        }
    }
    Some(Tiling {
        scale: s,
        landscape: use_landscape,
        cols,
        rows,
        tiles,
        page_mm,
    })
}

/// Labels of a sheet's neighbours: (left, right, up, down).
pub fn neighbours(t: &Tile, tiling: &Tiling) -> [Option<String>; 4] {
    let label = |col: Option<usize>, row: Option<usize>| -> Option<String> {
        let (c, r) = (col?, row?);
        (c < tiling.cols && r < tiling.rows).then(|| format!("{}{}", column_label(c), r + 1))
    };
    [
        label(t.col.checked_sub(1), Some(t.row)),
        label(Some(t.col + 1), Some(t.row)),
        label(Some(t.col), t.row.checked_sub(1)),
        label(Some(t.col), Some(t.row + 1)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(w: f64, h: f64) -> kurbo::Rect {
        kurbo::Rect::new(0., 0., w, h)
    }

    #[test]
    fn picks_orientation_and_checks_fit() {
        let l = layout_at(r(20_000., 14_000.), Paper::A4, 100).unwrap(); // 200 × 140 mm
        assert!(l.landscape);
        assert!((l.width_mm - 200.).abs() < 1e-9 && (l.height_mm - 140.).abs() < 1e-9);
        assert!(layout_at(r(20_000., 14_000.), Paper::A4, 50).is_none()); // 400 × 280 mm doesn't fit
        assert!(
            !layout_at(r(5_000., 9_000.), Paper::A4, 50)
                .unwrap()
                .landscape
        ); // tall plan, portrait
    }

    #[test]
    fn best_scale_is_the_most_detailed_that_fits() {
        assert_eq!(
            best_standard_scale(r(20_000., 14_000.), Paper::A4),
            Some(100)
        );
        assert_eq!(best_standard_scale(r(3_000., 2_000.), Paper::A4), Some(20));
        assert_eq!(best_standard_scale(r(500_000., 400_000.), Paper::A4), None);
    }

    #[test]
    fn fit_to_page_fills_the_printable_area() {
        let l = fit_to_page(r(20_000., 14_000.), Paper::A4);
        assert!(l.landscape);
        let fills_width = (l.width_mm - (297. - 2. * PRINT_MARGIN_MM)).abs() < 1e-6;
        let fills_height =
            (l.height_mm - (210. - 2. * PRINT_MARGIN_MM - PRINT_FURNITURE_MM)).abs() < 1e-6;
        assert!(fills_width || fills_height);
        assert!(
            l.width_mm <= 297. - 2. * PRINT_MARGIN_MM + 1e-6
                && l.height_mm <= 210. - 2. * PRINT_MARGIN_MM + 1e-6
        );
    }

    #[test]
    fn leaves_room_for_title_and_scale_bar() {
        // 17 × 25.5 m at 1:100 is 170 × 255 mm: inside the bare margins, but not
        // once the title line and scale bar are on the page too.
        assert!(layout_at(r(17_000., 25_500.), Paper::A4, 100).is_none());
        assert_eq!(
            best_standard_scale(r(17_000., 25_500.), Paper::A4),
            Some(200)
        );
        let l = fit_to_page(r(12_000., 20_000.), Paper::A4);
        assert!(l.height_mm <= 297. - 2. * PRINT_MARGIN_MM - PRINT_FURNITURE_MM + 1e-6);
    }

    #[test]
    fn a_plan_that_fits_is_one_tile() {
        let t = tile(r(20_000., 14_000.), Paper::A4, 100).unwrap();
        assert_eq!((t.cols, t.rows), (1, 1));
        assert_eq!(t.tiles[0].label, "A1");
        assert!(t.landscape);
    }

    #[test]
    fn big_plans_split_with_overlap_and_cover_everything() {
        let w = r(40_000., 28_000.); // 800 × 560 mm at 1:50
        let t = tile(w, Paper::A4, 50).unwrap();
        // A4 printable: landscape 273 × 166 mm → 4 × 4 = 16; portrait 186 × 253 mm → 5 × 3 = 15
        assert!(!t.landscape);
        assert_eq!((t.cols, t.rows), (5, 3));
        assert_eq!(t.tiles.len(), t.cols * t.rows);
        // neighbours overlap by exactly 10 mm on paper = 500 mm in the world
        let (a1, b1) = (&t.tiles[0], &t.tiles[1]);
        assert_eq!((a1.label.as_str(), b1.label.as_str()), ("A1", "B1"));
        assert!((a1.world.x1 - b1.world.x0 - 500.).abs() < 1e-6);
        // every point of the plan is on some tile
        for x in [0., 13_333., 26_666., 39_999.9] {
            for y in [0., 9_999., 27_999.9] {
                let p = kurbo::Point::new(x, y);
                assert!(
                    t.tiles.iter().any(|tl| tl.world.contains(p)),
                    "{x},{y} not printed"
                );
            }
        }
        // each page's world size ÷ scale is exactly the printable page
        for tl in &t.tiles {
            assert!((tl.world.width() / t.scale - t.page_mm.0).abs() < 1e-6);
            assert!((tl.world.height() / t.scale - t.page_mm.1).abs() < 1e-6);
        }
    }

    #[test]
    fn orientation_minimises_pages() {
        // a 10 × 60 m strip at 1:50: portrait 2 × 5 = 10 pages, landscape 1 × 8 = 8 pages
        let w = r(10_000., 60_000.);
        let t = tile(w, Paper::A4, 50).unwrap();
        assert_eq!(
            t.tiles.len(),
            count(w, Paper::A4, 50., true).min(count(w, Paper::A4, 50., false))
        );
        assert!(t.landscape);
        assert_eq!((t.cols, t.rows), (1, 8));
    }

    #[test]
    fn labels_and_neighbours() {
        assert_eq!(column_label(0), "A");
        assert_eq!(column_label(25), "Z");
        assert_eq!(column_label(26), "AA");
        assert_eq!(column_label(27), "AB");
        let t = tile(r(40_000., 28_000.), Paper::A4, 50).unwrap();
        let b2 = t.tiles.iter().find(|x| x.label == "B2").unwrap();
        assert_eq!(
            neighbours(b2, &t),
            [
                Some("A2".into()),
                Some("C2".into()),
                Some("B1".into()),
                Some("B3".into())
            ]
        );
        assert_eq!(neighbours(&t.tiles[0], &t)[0], None);
        assert_eq!(neighbours(&t.tiles[0], &t)[2], None);
    }

    #[test]
    fn refuses_absurd_tilings() {
        assert!(tile(r(500_000., 400_000.), Paper::A4, 20).is_none()); // thousands of pages
        assert!(tile(kurbo::Rect::new(0., 0., f64::NAN, 10.), Paper::A4, 50).is_none());
        let tiny = tile(r(0., 0.), Paper::A4, 20).unwrap();
        assert_eq!(tiny.tiles.len(), 1);
        let long = tile(r(200_000., 1_000.), Paper::Letter, 100).unwrap();
        assert_eq!(long.rows, 1);
        assert!(long.tiles.len() <= MAX_TILES);
    }
}
