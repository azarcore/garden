//! Flat surfaces and prisms built from closed outlines.

use garden_core::geometry::Shape;

use crate::MeshBuilder;

/// Curves are flattened to within this many millimetres.
pub(crate) const FLATTEN_MM: f64 = 20.0;
/// Triangulated area may differ from the outline's by at most this fraction.
const AREA_TOLERANCE: f64 = 0.02;

/// The outline's corner points in metres (plan x, plan y), without a repeated
/// closing point or consecutive duplicates.
pub(crate) fn outline_m(shape: &Shape) -> Vec<[f64; 2]> {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for p in shape.flatten(FLATTEN_MM) {
        let q = [p.x / 1000.0, p.y / 1000.0];
        if pts
            .last()
            .is_none_or(|l| (l[0] - q[0]).hypot(l[1] - q[1]) > 1e-6)
        {
            pts.push(q);
        }
    }
    if pts.len() > 1 {
        let (f, l) = (pts[0], pts[pts.len() - 1]);
        if (f[0] - l[0]).hypot(f[1] - l[1]) <= 1e-6 {
            pts.pop();
        }
    }
    pts
}

/// Twice the signed area of a polygon in (x, z): positive when it runs
/// counter-clockwise with x to the right and z up.
pub(crate) fn signed_area2(pts: &[[f64; 2]]) -> f64 {
    (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// Triangles (as index triples into `pts`) filling a simple polygon, or None
/// if it can't be filled faithfully (too few points, no area, self-crossing).
pub(crate) fn triangulate(pts: &[[f64; 2]], expected_area_m2: f64) -> Option<Vec<[usize; 3]>> {
    if pts.len() < 3 || !expected_area_m2.is_finite() || expected_area_m2 <= 1e-6 {
        return None;
    }
    let flat: Vec<f64> = pts.iter().flat_map(|p| [p[0], p[1]]).collect();
    let idx = earcutr::earcut(&flat, &[], 2).ok()?;
    if idx.is_empty() || idx.len() % 3 != 0 {
        return None;
    }
    let tris: Vec<[usize; 3]> = idx.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
    let area: f64 = tris
        .iter()
        .map(|t| signed_area2(&[pts[t[0]], pts[t[1]], pts[t[2]]]).abs() / 2.0)
        .sum();
    ((area - expected_area_m2).abs() <= AREA_TOLERANCE * expected_area_m2).then_some(tris)
}

/// Flat, upward-facing triangles for a closed outline at `y` metres; None if
/// it can't be triangulated.
pub fn flat_polygon(shape: &Shape, y: f64) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    let mut b = MeshBuilder::default();
    add_top(&mut b, shape, y)?;
    Some((b.positions, b.indices))
}

/// Add the outline's top face at `y`; returns the outline points on success.
pub(crate) fn add_top(b: &mut MeshBuilder, shape: &Shape, y: f64) -> Option<Vec<[f64; 2]>> {
    // only closed, well-formed outlines are filled (as in 2D)
    if !shape.closed || !shape.is_finite() {
        return None;
    }
    let pts = outline_m(shape);
    // compare with the flattened outline actually being filled: flattening
    // shaves a few percent off small curves, which mustn't count as a failure
    let outline_area = signed_area2(&pts).abs() / 2.0;
    let tris = triangulate(&pts, outline_area)?;
    let up = [0.0, 1.0, 0.0];
    for t in tris {
        let [a, c, d] = t.map(|i| [pts[i][0], y, pts[i][1]]);
        b.triangle(a, c, d, up);
    }
    Some(pts)
}

/// A closed outline extruded from `bottom` to `top` metres: top face plus
/// side walls facing outwards. None if the outline can't be filled.
pub(crate) fn add_prism(b: &mut MeshBuilder, shape: &Shape, bottom: f64, top: f64) -> Option<()> {
    let pts = add_top(b, shape, top)?;
    let ccw = signed_area2(&pts) > 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        let (dx, dz) = (q[0] - p[0], q[1] - p[1]);
        let len = dx.hypot(dz);
        if len < 1e-9 {
            continue;
        }
        let n = if ccw {
            [dz / len, 0.0, -dx / len]
        } else {
            [-dz / len, 0.0, dx / len]
        };
        b.quad(
            [p[0], bottom, p[1]],
            [q[0], bottom, q[1]],
            [q[0], top, q[1]],
            [p[0], top, p[1]],
            n,
        );
    }
    Some(())
}
