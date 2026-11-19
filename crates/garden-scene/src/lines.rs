//! Paths (flat strips) and fences, walls and hedges (boxes along a line).

use std::f64::consts::TAU;

use garden_core::geometry::Shape;

use crate::MeshBuilder;
use crate::surfaces::FLATTEN_MM;

/// Segments in a round cap or joint.
const CAP_SEGMENTS: usize = 12;
const JOINT_SIDES: usize = 8;

/// The line's points in metres, without consecutive duplicates; a closed
/// shape repeats its first point at the end.
pub(crate) fn line_m(shape: &Shape) -> Vec<[f64; 2]> {
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
    if shape.closed && pts.len() > 2 {
        let (f, l) = (pts[0], pts[pts.len() - 1]);
        if (f[0] - l[0]).hypot(f[1] - l[1]) > 1e-6 {
            pts.push(f);
        }
    }
    pts
}

/// Unit direction and half-width side vector of a segment, if it has length.
fn frame(p: [f64; 2], q: [f64; 2], half: f64) -> Option<([f64; 2], [f64; 2])> {
    let (dx, dz) = (q[0] - p[0], q[1] - p[1]);
    let len = dx.hypot(dz);
    (len > 1e-9).then(|| ([dx / len, dz / len], [-dz / len * half, dx / len * half]))
}

/// A flat strip `width` metres wide along the line at height `y`, with round
/// ends and joints so corners have no gaps. Nothing for a line without length.
pub(crate) fn add_strip(b: &mut MeshBuilder, pts: &[[f64; 2]], width: f64, y: f64) {
    let half = width / 2.0;
    let up = [0.0, 1.0, 0.0];
    let mut any = false;
    for w in pts.windows(2) {
        let (p, q) = (w[0], w[1]);
        let Some((_, s)) = frame(p, q, half) else {
            continue;
        };
        any = true;
        b.quad(
            [p[0] + s[0], y, p[1] + s[1]],
            [q[0] + s[0], y, q[1] + s[1]],
            [q[0] - s[0], y, q[1] - s[1]],
            [p[0] - s[0], y, p[1] - s[1]],
            up,
        );
    }
    if !any {
        return;
    }
    for p in pts {
        for i in 0..CAP_SEGMENTS {
            let (a0, a1) = (
                TAU * i as f64 / CAP_SEGMENTS as f64,
                TAU * (i + 1) as f64 / CAP_SEGMENTS as f64,
            );
            b.triangle(
                [p[0], y, p[1]],
                [p[0] + half * a0.cos(), y, p[1] + half * a0.sin()],
                [p[0] + half * a1.cos(), y, p[1] + half * a1.sin()],
                up,
            );
        }
    }
}

/// A box `thickness` wide and `height` tall standing on the ground along each
/// segment; with `joints`, an eight-sided post at every inner point so corners close.
pub(crate) fn add_boxes(
    b: &mut MeshBuilder,
    pts: &[[f64; 2]],
    thickness: f64,
    height: f64,
    joints: bool,
) {
    let half = thickness / 2.0;
    let h = height;
    for w in pts.windows(2) {
        let (p, q) = (w[0], w[1]);
        let Some((d, s)) = frame(p, q, half) else {
            continue;
        };
        let corner = |c: [f64; 2], side: f64, y: f64| [c[0] + side * s[0], y, c[1] + side * s[1]];
        let n_side = [s[0] / half, 0.0, s[1] / half];
        // top
        b.quad(
            corner(p, 1.0, h),
            corner(q, 1.0, h),
            corner(q, -1.0, h),
            corner(p, -1.0, h),
            [0.0, 1.0, 0.0],
        );
        // long sides
        b.quad(
            corner(p, 1.0, 0.0),
            corner(q, 1.0, 0.0),
            corner(q, 1.0, h),
            corner(p, 1.0, h),
            n_side,
        );
        b.quad(
            corner(p, -1.0, 0.0),
            corner(q, -1.0, 0.0),
            corner(q, -1.0, h),
            corner(p, -1.0, h),
            n_side.map(|v| -v),
        );
        // ends
        b.quad(
            corner(p, 1.0, 0.0),
            corner(p, -1.0, 0.0),
            corner(p, -1.0, h),
            corner(p, 1.0, h),
            [-d[0], 0.0, -d[1]],
        );
        b.quad(
            corner(q, 1.0, 0.0),
            corner(q, -1.0, 0.0),
            corner(q, -1.0, h),
            corner(q, 1.0, h),
            [d[0], 0.0, d[1]],
        );
    }
    if joints && pts.len() > 2 {
        for p in &pts[1..pts.len() - 1] {
            add_post(b, *p, half, h);
        }
    }
}

/// An eight-sided post of radius `r` from the ground to `h`.
fn add_post(b: &mut MeshBuilder, c: [f64; 2], r: f64, h: f64) {
    let at = |i: usize| {
        let a = TAU * i as f64 / JOINT_SIDES as f64;
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    };
    for i in 0..JOINT_SIDES {
        let (p, q) = (at(i), at(i + 1));
        let mid = TAU * (i as f64 + 0.5) / JOINT_SIDES as f64;
        b.quad(
            [p[0], 0.0, p[1]],
            [q[0], 0.0, q[1]],
            [q[0], h, q[1]],
            [p[0], h, p[1]],
            [mid.cos(), 0.0, mid.sin()],
        );
        b.triangle(
            [c[0], h, c[1]],
            [p[0], h, p[1]],
            [q[0], h, q[1]],
            [0.0, 1.0, 0.0],
        );
    }
}
