//! The sun map: how many hours of direct sun each spot of the garden gets.
//!
//! Everything that can cast a shadow becomes a simple solid, shaped as the
//! 3D view draws it: structures and wall/fence/hedge segments are upright
//! prisms, plants are round or conical crowns. A spot on the ground is in
//! shadow when the line from it towards the sun passes through any of them.
//! All lengths in millimetres; plan coordinates (y grows down the plan).

use kurbo::{Point, Rect, Vec2};
use loma_core::date::Date;
use loma_core::model::{Document, GeoPoint, LinearKind, ObjectKind, PlantCategory, PlantSymbol};
use loma_core::sun::sun_position;

/// Shadows longer than this (very low sun) are cut short.
const MAX_SHADOW: f64 = 100_000.0;

/// Something that casts a shadow.
#[derive(Clone, Debug, PartialEq)]
pub enum Caster {
    /// An upright prism standing on the ground.
    Prism { footprint: Vec<Point>, height: f64 },
    /// A plant's crown: a spheroid, or a cone with its point at `top`.
    Crown {
        centre: Point,
        radius: f64,
        bottom: f64,
        top: f64,
        cone: bool,
    },
}

/// Where the sun is: a bearing on the plan (degrees clockwise from the top
/// of the plan) and its height above the horizon in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunDir {
    pub bearing: f64,
    pub altitude: f64,
}

impl SunDir {
    /// The unit direction on the plan towards the sun (y down the plan).
    fn toward(self) -> Vec2 {
        let b = self.bearing.to_radians();
        Vec2::new(b.sin(), -b.cos())
    }

    /// Ground distance per unit of height (cot of the altitude), capped.
    fn reach(self, height: f64) -> f64 {
        let t = self.altitude.to_radians().tan();
        if t <= 0.0 {
            MAX_SHADOW
        } else {
            (height / t).min(MAX_SHADOW)
        }
    }
}

/// A box for one segment of a wall, fence or hedge.
fn segment_box(a: Point, b: Point, thickness: f64) -> Option<Vec<Point>> {
    let d = b - a;
    let len = d.hypot();
    if len < 1e-6 {
        return None;
    }
    let n = Vec2::new(-d.y, d.x) * (thickness.max(1.0) / 2.0 / len);
    Some(vec![a + n, b + n, b - n, a - n])
}

/// Everything in the design that can cast a shadow, shaped as in 3D.
pub fn casters(doc: &Document) -> Vec<Caster> {
    let mut out = Vec::new();
    for o in &doc.objects {
        match &o.kind {
            ObjectKind::Structure {
                footprint, height, ..
            } => {
                let pts = footprint.flatten(10.0);
                if pts.len() >= 3 {
                    out.push(Caster::Prism {
                        footprint: pts,
                        height: height.max(1.0),
                    });
                }
            }
            ObjectKind::Linear {
                kind: LinearKind::Fence | LinearKind::Wall | LinearKind::Hedge,
                shape,
                thickness,
                height,
            } => {
                let pts = shape.flatten(10.0);
                for w in pts.windows(2) {
                    if let Some(footprint) = segment_box(w[0], w[1], *thickness) {
                        out.push(Caster::Prism {
                            footprint,
                            height: height.max(1.0),
                        });
                    }
                }
            }
            ObjectKind::Plant {
                position,
                spread,
                height,
                symbol,
                category,
                ..
            } => {
                let (r, h) = (spread.max(1.0) / 2.0, height.max(1.0));
                let crown = |bottom: f64, top: f64, cone: bool| Caster::Crown {
                    centre: *position,
                    radius: r,
                    bottom,
                    top,
                    cone,
                };
                out.push(match category {
                    PlantCategory::Tree | PlantCategory::FruitTree => {
                        if *symbol == PlantSymbol::Conifer {
                            crown(0.2 * h, h, true)
                        } else {
                            crown(0.3 * h, h, false)
                        }
                    }
                    PlantCategory::Grass => crown(0.0, h, true),
                    PlantCategory::Shrub | PlantCategory::Climber | PlantCategory::Other => {
                        crown(0.0, h, false)
                    }
                    // a mound: centre at 0.35 h, reaching 0.65 h above and below it
                    PlantCategory::Perennial
                    | PlantCategory::Annual
                    | PlantCategory::Herb
                    | PlantCategory::Vegetable => crown(-0.3 * h, h, false),
                });
            }
            _ => {}
        }
    }
    out
}

/// Whether `p` is inside the polygon (even-odd).
fn inside(poly: &[Point], p: Point) -> bool {
    let mut odd = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Whether segments ab and cd cross or touch.
fn segments_meet(a: Point, b: Point, c: Point, d: Point) -> bool {
    let cross = |o: Point, p: Point, q: Point| (p - o).cross(q - o);
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    (d1 * d2 <= 0.0) && (d3 * d4 <= 0.0) && !(d1 == 0.0 && d2 == 0.0 && d3 == 0.0 && d4 == 0.0)
}

/// The smallest value of the quadratic a·t² + b·t + c over [t0, t1].
fn quadratic_min(a: f64, b: f64, c: f64, t0: f64, t1: f64) -> f64 {
    let f = |t: f64| (a * t + b) * t + c;
    let mut best = f(t0).min(f(t1));
    if a > 0.0 {
        let t = (-b / (2.0 * a)).clamp(t0, t1);
        best = best.min(f(t));
    }
    best
}

/// Whether the ground point `p` is in the shadow of `c` with the sun at `sun`.
pub fn in_shadow(c: &Caster, p: Point, sun: SunDir) -> bool {
    if sun.altitude <= 0.0 {
        return true; // no sun at all
    }
    let u = sun.toward();
    match c {
        Caster::Prism { footprint, height } => {
            // the ray towards the sun is below the top until this ground distance
            let end = p + u * sun.reach(*height);
            inside(footprint, p)
                || inside(footprint, end)
                || (0..footprint.len()).any(|i| {
                    let j = (i + 1) % footprint.len();
                    segments_meet(p, end, footprint[i], footprint[j])
                })
        }
        Caster::Crown {
            centre,
            radius,
            bottom,
            top,
            cone,
        } => {
            // the ray: horizontal q + t·w (from the crown's axis), height t·s
            let a = sun.altitude.to_radians();
            let (w, s) = (u * a.cos(), a.sin());
            let q = p - *centre;
            let (t0, t1) = (bottom.max(0.0) / s, top / s);
            if t1 < t0 {
                return false;
            }
            let (qq, qw, ww) = (q.dot(q), q.dot(w), w.dot(w));
            if *cone {
                // inside when |q + t·w| ≤ k·(top − t·s), k = radius per height
                let k = radius / (top - bottom).max(1.0);
                let k2 = k * k;
                let qa = ww - k2 * s * s;
                let qb = 2.0 * qw + 2.0 * k2 * top * s;
                let qc = qq - k2 * top * top;
                quadratic_min(qa, qb, qc, t0, t1) <= 0.0
            } else {
                // a spheroid between bottom and top: |q + t·w|²/r² + (t·s − m)²/b² ≤ 1
                let (m, b) = ((bottom + top) / 2.0, (top - bottom) / 2.0);
                let (r2, b2) = (radius * radius, (b * b).max(1.0));
                let qa = ww / r2 + s * s / b2;
                let qb = 2.0 * qw / r2 - 2.0 * s * m / b2;
                let qc = qq / r2 + m * m / b2 - 1.0;
                quadratic_min(qa, qb, qc, 0.0, t1) <= 0.0
            }
        }
    }
}

/// A box on the ground holding the whole shadow of `c`.
pub fn caster_shadow_bounds(c: &Caster, sun: SunDir) -> Rect {
    let away = -sun.toward();
    let (base, height) = match c {
        Caster::Prism { footprint, height } => {
            let mut r = Rect::from_points(footprint[0], footprint[0]);
            for p in footprint {
                r = r.union_pt(*p);
            }
            (r, *height)
        }
        Caster::Crown {
            centre,
            radius,
            top,
            ..
        } => (
            Rect::from_center_size(*centre, (2.0 * radius, 2.0 * radius)),
            *top,
        ),
    };
    let shifted = base + away * sun.reach(height);
    base.union(shifted)
}

// ---- hours of sun on a grid ----

/// Grid squares (mm), and how far past the garden the grid reaches.
const CELL: f64 = 250.0;
const MARGIN: f64 = 2000.0;
/// At most this many squares a side (bigger gardens get bigger squares).
const MAX_CELLS: usize = 400;
/// The sun is looked at every 15 minutes (in the middle of each quarter).
const STEP_HOURS: f32 = 0.25;
const STEPS_PER_DAY: u32 = 96;

/// Hours of sun per grid square.
#[derive(Clone, Debug, PartialEq)]
pub struct SunGrid {
    /// The top-left corner of the first square.
    pub origin: Point,
    /// Square size (mm).
    pub cell: f64,
    pub cols: usize,
    pub rows: usize,
    /// Row by row, from the top of the plan.
    pub hours: Vec<f32>,
}

impl SunGrid {
    /// The centre of square (col, row).
    pub fn centre(&self, col: usize, row: usize) -> Point {
        self.origin
            + Vec2::new(
                (col as f64 + 0.5) * self.cell,
                (row as f64 + 0.5) * self.cell,
            )
    }

    /// The whole grid on the plan.
    pub fn rect(&self) -> Rect {
        Rect::from_origin_size(
            self.origin,
            (self.cols as f64 * self.cell, self.rows as f64 * self.cell),
        )
    }

    /// Hours of sun at `p`, or `None` off the grid.
    pub fn hours_at(&self, p: Point) -> Option<f32> {
        let d = p - self.origin;
        if d.x < 0.0 || d.y < 0.0 {
            return None;
        }
        let (col, row) = ((d.x / self.cell) as usize, (d.y / self.cell) as usize);
        (col < self.cols && row < self.rows).then(|| self.hours[row * self.cols + col])
    }
}

/// An empty grid over the garden: the plot boundary if there is one, else
/// everything drawn, with room around. `None` for an empty design.
pub fn grid_for(doc: &Document) -> Option<SunGrid> {
    let plot = doc.objects.iter().find_map(|o| match &o.kind {
        ObjectKind::PlotBoundary { shape } => shape
            .is_finite()
            .then(|| crate::objects::object_bounds(&o.kind)),
        _ => None,
    });
    let bounds = plot.or_else(|| {
        doc.objects
            .iter()
            .map(|o| crate::objects::object_bounds(&o.kind))
            .reduce(|a, b| a.union(b))
    })?;
    let area = bounds.inflate(MARGIN, MARGIN);
    let longest = area.width().max(area.height());
    let cell = CELL.max(longest / MAX_CELLS as f64);
    let cols = (area.width() / cell).ceil().max(1.0) as usize;
    let rows = (area.height() / cell).ceil().max(1.0) as usize;
    Some(SunGrid {
        origin: area.origin(),
        cell,
        cols,
        rows,
        hours: vec![0.0; cols * rows],
    })
}

/// How sunny a spot is, as plant labels put it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// 6 hours or more.
    Full,
    /// 3 to 6 hours.
    Part,
    /// Under 3 hours.
    Shade,
}

pub fn band(hours: f32) -> Band {
    if hours >= 6.0 {
        Band::Full
    } else if hours >= 3.0 {
        Band::Part
    } else {
        Band::Shade
    }
}

/// Which days the map is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Span {
    /// The growing season of `year`, averaged.
    Season { year: i32 },
    /// One day.
    Day(Date),
}

/// The days to add up: one, or the 1st and 15th of each growing month
/// (April to September in the north, October to March in the south).
pub fn days(span: Span, lat: f64) -> Vec<Date> {
    match span {
        Span::Day(d) => vec![d],
        Span::Season { year } => {
            let months: &[u32] = if lat >= 0.0 {
                &[4, 5, 6, 7, 8, 9]
            } else {
                &[1, 2, 3, 10, 11, 12]
            };
            months
                .iter()
                .flat_map(|m| [1, 15].map(|d| Date::from_ymd(year, *m, d)))
                .flatten()
                .collect()
        }
    }
}

/// Add `weight` × the hours of direct sun on `day` to every square.
pub fn day_hours(
    casters: &[Caster],
    at: GeoPoint,
    north: f64,
    day: Date,
    grid: &mut SunGrid,
    weight: f32,
) {
    let epoch = Date::from_ymd(1970, 1, 1).expect("a real date");
    // from local solar midnight, so the day isn't split around noon
    let start = day.days_since(epoch) as f64 * 86_400_000.0 - at.lon / 15.0 * 3_600_000.0;
    let mut shade = vec![false; grid.hours.len()];
    for step in 0..STEPS_PER_DAY {
        let t = start + (step as f64 + 0.5) * (STEP_HOURS as f64) * 3_600_000.0;
        let sky = sun_position(at, t);
        if sky.altitude <= 0.0 {
            continue;
        }
        let sun = SunDir {
            bearing: crate::compass::sun_bearing_on_plan(north, sky.azimuth),
            altitude: sky.altitude,
        };
        shade.fill(false);
        for c in casters {
            let b = caster_shadow_bounds(c, sun).intersect(grid.rect());
            if b.width() <= 0.0 || b.height() <= 0.0 {
                continue;
            }
            let col0 = ((b.x0 - grid.origin.x) / grid.cell).floor().max(0.0) as usize;
            let row0 = ((b.y0 - grid.origin.y) / grid.cell).floor().max(0.0) as usize;
            let col1 = (((b.x1 - grid.origin.x) / grid.cell).ceil() as usize).min(grid.cols);
            let row1 = (((b.y1 - grid.origin.y) / grid.cell).ceil() as usize).min(grid.rows);
            for row in row0..row1 {
                for col in col0..col1 {
                    let i = row * grid.cols + col;
                    if !shade[i] && in_shadow(c, grid.centre(col, row), sun) {
                        shade[i] = true;
                    }
                }
            }
        }
        for (h, shaded) in grid.hours.iter_mut().zip(&shade) {
            if !shaded {
                *h += STEP_HOURS * weight;
            }
        }
    }
}

/// Working out a sun map a day at a time, so a page can stay responsive.
pub struct SunMapJob {
    casters: Vec<Caster>,
    at: GeoPoint,
    north: f64,
    days: Vec<Date>,
    done: usize,
    grid: SunGrid,
}

impl SunMapJob {
    /// `None` when the design has no location or nothing in it.
    pub fn new(doc: &Document, span: Span) -> Option<SunMapJob> {
        let at = doc.location?;
        Some(SunMapJob {
            casters: casters(doc),
            at,
            north: doc.north,
            days: days(span, at.lat),
            done: 0,
            grid: grid_for(doc)?,
        })
    }

    /// Work out one more day; true once every day is done.
    pub fn step(&mut self) -> bool {
        if let Some(day) = self.days.get(self.done).copied() {
            let weight = 1.0 / self.days.len() as f32;
            day_hours(
                &self.casters,
                self.at,
                self.north,
                day,
                &mut self.grid,
                weight,
            );
            self.done += 1;
        }
        self.done >= self.days.len()
    }

    /// Days done, and days in all.
    pub fn progress(&self) -> (usize, usize) {
        (self.done, self.days.len())
    }

    pub fn grid(&self) -> &SunGrid {
        &self.grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::doc_with;
    use loma_core::geometry::Shape;
    use loma_core::model::{AreaType, StructureKind};

    fn wall() -> ObjectKind {
        ObjectKind::Linear {
            kind: LinearKind::Wall,
            shape: Shape::polyline(&[Point::new(0., 0.), Point::new(4000., 0.)], false),
            thickness: 200.,
            height: 2000.,
        }
    }

    fn sun(bearing: f64, altitude: f64) -> SunDir {
        SunDir { bearing, altitude }
    }

    fn shaded(kinds: Vec<ObjectKind>, p: (f64, f64), s: SunDir) -> bool {
        let doc = doc_with(kinds);
        casters(&doc)
            .iter()
            .any(|c| in_shadow(c, Point::new(p.0, p.1), s))
    }

    #[test]
    fn a_wall_shades_the_side_away_from_the_sun() {
        // sun towards plan-down: the shadow falls towards plan-up
        let s = sun(180.0, 45.0);
        assert!(shaded(vec![wall()], (2000., -1500.), s));
        assert!(!shaded(vec![wall()], (2000., -2500.), s));
        assert!(!shaded(vec![wall()], (2000., 1000.), s), "the sunny side");
    }

    #[test]
    fn a_low_sun_makes_a_long_shadow() {
        // cot 26.565° = 2: a 2 m wall shades 4 m (plus half its thickness)
        let s = sun(180.0, 26.565_051);
        assert!(shaded(vec![wall()], (2000., -3900.), s));
        assert!(!shaded(vec![wall()], (2000., -4200.), s));
    }

    #[test]
    fn inside_a_building_is_always_shaded() {
        let shed = ObjectKind::Structure {
            kind: StructureKind::Shed,
            footprint: Shape::rect(Point::new(0., 0.), 2000., 2000.),
            height: 3000.,
        };
        for alt in [5.0, 45.0, 89.0] {
            assert!(
                shaded(vec![shed.clone()], (1000., 1000.), sun(123.0, alt)),
                "{alt}"
            );
        }
    }

    #[test]
    fn a_shrub_shades_the_ground_beside_it() {
        let shrub = ObjectKind::Plant {
            position: Point::new(0., 0.),
            spread: 2000.,
            height: 2000.,
            symbol: PlantSymbol::Round,
            category: PlantCategory::Shrub,
            catalog_id: None,
        };
        // sun to plan-right, 45° up: the shadow falls to plan-left
        let s = sun(90.0, 45.0);
        assert!(shaded(vec![shrub.clone()], (-1000., 0.), s));
        assert!(!shaded(vec![shrub.clone()], (-3500., 0.), s));
        assert!(!shaded(vec![shrub], (1500., 0.), s));
    }

    #[test]
    fn a_conifer_casts_a_pointed_shadow() {
        let fir = ObjectKind::Plant {
            position: Point::new(0., 0.),
            spread: 2000.,
            height: 6000.,
            symbol: PlantSymbol::Conifer,
            category: PlantCategory::Tree,
            catalog_id: None,
        };
        // sun to plan-down, 45° up: shadow up the plan, as long as the tree is tall
        let s = sun(180.0, 45.0);
        assert!(
            shaded(vec![fir.clone()], (0., -3000.), s),
            "under the crown's shadow"
        );
        assert!(
            shaded(vec![fir.clone()], (0., -5800.), s),
            "near the tip's shadow"
        );
        assert!(!shaded(vec![fir.clone()], (0., -6300.), s), "past the tip");
        assert!(
            !shaded(vec![fir], (900., -5800.), s),
            "the tip's shadow is narrow"
        );
    }

    #[test]
    fn only_solid_things_cast_shadows() {
        let lawn = ObjectKind::Area {
            area_type: AreaType::Lawn,
            shape: Shape::rect(Point::new(0., 0.), 5000., 5000.),
        };
        let fence = ObjectKind::Linear {
            kind: LinearKind::Fence,
            shape: Shape::polyline(
                &[
                    Point::new(0., 0.),
                    Point::new(3000., 0.),
                    Point::new(3000., 3000.),
                ],
                false,
            ),
            thickness: 50.,
            height: 1800.,
        };
        let doc = doc_with(vec![lawn, fence]);
        let cs = casters(&doc);
        assert_eq!(
            cs.len(),
            2,
            "one prism per fence segment, none for the lawn"
        );
        assert!(cs.iter().all(|c| matches!(c, Caster::Prism { .. })));
    }

    #[test]
    fn shadow_bounds_cover_the_shadow() {
        let doc = doc_with(vec![wall()]);
        let c = &casters(&doc)[0];
        let b = caster_shadow_bounds(c, sun(180.0, 45.0));
        assert!(b.contains(Point::new(2000., -1500.)), "{b:?}");
        assert!(b.y0 <= -2000.0 && b.y1 >= 100.0, "{b:?}");
    }

    // ---- hours on a grid ----

    const GREENWICH: GeoPoint = GeoPoint {
        lat: 51.48,
        lon: 0.0,
    };
    const SVALBARD: GeoPoint = GeoPoint {
        lat: 78.22,
        lon: 15.65,
    };

    fn lawn() -> ObjectKind {
        ObjectKind::Area {
            area_type: AreaType::Lawn,
            shape: Shape::rect(Point::new(0., 0.), 4000., 3000.),
        }
    }

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn day_map(kinds: Vec<ObjectKind>, at: GeoPoint, north: f64, day: Date) -> SunGrid {
        let doc = doc_with(kinds);
        let mut grid = grid_for(&doc).unwrap();
        day_hours(&casters(&doc), at, north, day, &mut grid, 1.0);
        grid
    }

    #[test]
    fn open_ground_gets_the_whole_day() {
        let grid = day_map(vec![lawn()], GREENWICH, 0.0, date(2024, 6, 21));
        // Greenwich midsummer: about 16 h 38 min from sunrise to sunset
        for h in &grid.hours {
            assert!((h - 16.6).abs() < 0.35, "{h}");
        }
    }

    #[test]
    fn a_wall_leaves_its_north_side_in_shade_and_turns_with_north() {
        let at = |g: &SunGrid, y: f64| g.hours_at(Point::new(2000., y)).unwrap();
        let kinds = vec![lawn(), wall()];
        let winter = date(2024, 12, 21);
        // north up the plan: the sun is down the plan, so up the plan is shaded
        let g = day_map(kinds.clone(), GREENWICH, 0.0, winter);
        assert!(
            at(&g, 1000.) > at(&g, -1000.) + 3.0,
            "{} vs {}",
            at(&g, 1000.),
            at(&g, -1000.)
        );
        // north turned round: the other side is the sunny one
        let g = day_map(kinds, GREENWICH, 180.0, winter);
        assert!(
            at(&g, -1000.) > at(&g, 1000.) + 3.0,
            "{} vs {}",
            at(&g, -1000.),
            at(&g, 1000.)
        );
    }

    #[test]
    fn polar_night_and_day() {
        let night = day_map(vec![lawn()], SVALBARD, 0.0, date(2024, 12, 21));
        assert!(night.hours.iter().all(|h| *h == 0.0));
        let day = day_map(vec![lawn()], SVALBARD, 0.0, date(2024, 6, 21));
        assert!(
            day.hours.iter().all(|h| (*h - 24.0).abs() < 1e-3),
            "{:?}",
            &day.hours[..3]
        );
    }

    #[test]
    fn the_growing_season_depends_on_the_hemisphere() {
        let north = days(Span::Season { year: 2025 }, 51.5);
        assert_eq!(north.len(), 12);
        assert_eq!(north[0], date(2025, 4, 1));
        assert_eq!(north[11], date(2025, 9, 15));
        let south = days(Span::Season { year: 2025 }, -33.9);
        assert_eq!(south.len(), 12);
        assert!(south.contains(&date(2025, 1, 1)) && south.contains(&date(2025, 3, 15)));
        assert!(south.contains(&date(2025, 10, 1)) && south.contains(&date(2025, 12, 15)));
        assert!(!south.contains(&date(2025, 6, 1)));
        assert_eq!(
            days(Span::Day(date(2025, 6, 21)), 0.0),
            vec![date(2025, 6, 21)]
        );
    }

    #[test]
    fn bands_follow_plant_labels() {
        assert_eq!(band(6.0), Band::Full);
        assert_eq!(band(5.9), Band::Part);
        assert_eq!(band(3.0), Band::Part);
        assert_eq!(band(2.9), Band::Shade);
    }

    #[test]
    fn the_grid_covers_the_garden_with_room_around() {
        let doc = doc_with(vec![lawn()]);
        let g = grid_for(&doc).unwrap();
        assert_eq!(g.cell, 250.0);
        assert_eq!(g.origin, Point::new(-2000., -2000.));
        assert_eq!((g.cols, g.rows), (32, 28));
        assert_eq!(g.hours_at(Point::new(-5000., 0.)), None);
        assert!(grid_for(&doc_with(vec![])).is_none());
    }

    #[test]
    fn a_season_job_runs_one_day_per_step() {
        let mut doc = doc_with(vec![lawn(), wall()]);
        assert!(
            SunMapJob::new(&doc, Span::Season { year: 2025 }).is_none(),
            "no location"
        );
        doc.location = Some(GREENWICH);
        let mut job = SunMapJob::new(&doc, Span::Season { year: 2025 }).unwrap();
        let mut steps = 0;
        while !job.step() {
            steps += 1;
        }
        assert_eq!(steps + 1, 12);
        assert_eq!(job.progress(), (12, 12));
        // the same as adding up the twelve days by hand
        let mut by_hand = grid_for(&doc).unwrap();
        for d in days(Span::Season { year: 2025 }, GREENWICH.lat) {
            day_hours(&casters(&doc), GREENWICH, 0.0, d, &mut by_hand, 1.0 / 12.0);
        }
        for (a, b) in job.grid().hours.iter().zip(&by_hand.hours) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    #[ignore]
    fn timing() {
        // a 20 × 15 m garden with 30 things casting shadows, for a season
        let mut kinds = vec![ObjectKind::Area {
            area_type: AreaType::Lawn,
            shape: Shape::rect(Point::new(0., 0.), 20_000., 15_000.),
        }];
        for i in 0..30 {
            let (x, y) = (
                (i % 6) as f64 * 3000. + 1000.,
                (i / 6) as f64 * 3000. + 1000.,
            );
            kinds.push(if i % 3 == 0 {
                ObjectKind::Linear {
                    kind: LinearKind::Hedge,
                    shape: Shape::polyline(&[Point::new(x, y), Point::new(x + 2000., y)], false),
                    thickness: 600.,
                    height: 1800.,
                }
            } else {
                ObjectKind::Plant {
                    position: Point::new(x, y),
                    spread: 2500.,
                    height: 4000.,
                    symbol: PlantSymbol::Round,
                    category: PlantCategory::Tree,
                    catalog_id: None,
                }
            });
        }
        let mut doc = doc_with(kinds);
        doc.location = Some(GREENWICH);
        let start = std::time::Instant::now();
        let mut job = SunMapJob::new(&doc, Span::Season { year: 2025 }).unwrap();
        while !job.step() {}
        println!(
            "season sun map: {:?} for {} cells",
            start.elapsed(),
            job.grid().hours.len()
        );
    }
}
