//! The sun map: how many hours of direct sun each spot of the garden gets.
//!
//! Everything that can cast a shadow becomes a simple solid, shaped as the
//! 3D view draws it: structures and wall/fence/hedge segments are upright
//! prisms, plants are round or conical crowns. A spot on the ground is in
//! shadow when the line from it towards the sun passes through any of them.
//! All lengths in millimetres; plan coordinates (y grows down the plan).

use kurbo::{Point, Rect, Vec2};
use loma_core::model::{Document, LinearKind, ObjectKind, PlantCategory, PlantSymbol};

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
}
