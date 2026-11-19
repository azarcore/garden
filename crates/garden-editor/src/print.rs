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
}
