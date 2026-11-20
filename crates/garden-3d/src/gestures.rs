//! Touch gestures for the 3D view, as plain maths: which way the fingers moved
//! between two frames, and when two taps make a double tap.

/// A finger on the screen, in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finger {
    pub id: u64,
    pub x: f32,
    pub y: f32,
}

/// What the fingers did between two frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    Still,
    /// one finger moved by (dx, dy) px
    Orbit {
        dx: f32,
        dy: f32,
    },
    /// two fingers: multiply the radius by `zoom`; their midpoint moved by (dx, dy) px
    Pinch {
        zoom: f32,
        dx: f32,
        dy: f32,
    },
}

/// What the fingers did between two frames. If fingers were added or lifted
/// (the ids differ), or there are three or more, or nothing moved: Still.
pub fn gesture(prev: &[Finger], now: &[Finger]) -> Gesture {
    // the same fingers in both frames, matched by id
    let pairs: Option<Vec<(Finger, Finger)>> = now
        .iter()
        .map(|n| prev.iter().find(|p| p.id == n.id).map(|p| (*p, *n)))
        .collect();
    let Some(pairs) = pairs.filter(|p| p.len() == prev.len()) else {
        return Gesture::Still;
    };
    match pairs.as_slice() {
        [(p, n)] => {
            let (dx, dy) = (n.x - p.x, n.y - p.y);
            if dx == 0.0 && dy == 0.0 {
                Gesture::Still
            } else {
                Gesture::Orbit { dx, dy }
            }
        }
        [(p1, n1), (p2, n2)] => {
            let before = distance(p1, p2);
            let after = distance(n1, n2);
            let zoom = if before < 1.0 || after < 1.0 {
                1.0
            } else {
                before / after
            };
            let dx = (n1.x + n2.x - p1.x - p2.x) / 2.0;
            let dy = (n1.y + n2.y - p1.y - p2.y) / 2.0;
            if zoom == 1.0 && dx == 0.0 && dy == 0.0 {
                Gesture::Still
            } else {
                Gesture::Pinch { zoom, dx, dy }
            }
        }
        _ => Gesture::Still,
    }
}

fn distance(a: &Finger, b: &Finger) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Longest press that still counts as a tap.
const TAP_MS: f64 = 250.0;
/// Furthest a finger may move during a tap.
const TAP_PX: f32 = 10.0;
/// Longest gap between the two taps' releases.
const DOUBLE_MS: f64 = 300.0;
/// Furthest apart the two taps may be.
const DOUBLE_PX: f32 = 30.0;

/// Spots a double tap from presses and releases (times in ms, positions in px).
#[derive(Default)]
pub struct Taps {
    /// the finger down now, if it may still be a tap: (id, when, x, y)
    down: Option<(u64, f64, f32, f32)>,
    /// fingers down that rule out a tap (a second finger)
    others: usize,
    /// the last tap's release: (when, x, y)
    last: Option<(f64, f32, f32)>,
}

impl Taps {
    pub fn press(&mut self, id: u64, t_ms: f64, x: f32, y: f32) {
        if self.down.is_some() || self.others > 0 {
            // more than one finger: not a tap, and nothing to pair with
            if self.down.take().is_some() {
                self.others += 1;
            }
            self.last = None;
            self.others += 1;
        } else {
            self.down = Some((id, t_ms, x, y));
        }
    }

    /// true when this release completes a double tap (then starts afresh)
    pub fn release(&mut self, id: u64, t_ms: f64, x: f32, y: f32) -> bool {
        let Some((_, t0, x0, y0)) = self.down.filter(|d| d.0 == id) else {
            self.others = self.others.saturating_sub(1);
            return false;
        };
        self.down = None;
        let tap = t_ms - t0 <= TAP_MS && (x - x0).hypot(y - y0) <= TAP_PX;
        if !tap {
            self.last = None;
            return false;
        }
        match self.last.take() {
            Some((t1, x1, y1)) if t_ms - t1 <= DOUBLE_MS && (x - x1).hypot(y - y1) <= DOUBLE_PX => {
                true
            }
            _ => {
                self.last = Some((t_ms, x, y));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(id: u64, x: f32, y: f32) -> Finger {
        Finger { id, x, y }
    }

    #[test]
    fn one_finger_orbits() {
        assert_eq!(
            gesture(&[f(1, 10., 10.)], &[f(1, 13., 6.)]),
            Gesture::Orbit { dx: 3., dy: -4. }
        );
    }

    #[test]
    fn spreading_two_fingers_zooms_in() {
        assert_eq!(
            gesture(
                &[f(1, 0., 0.), f(2, 100., 0.)],
                &[f(1, -50., 0.), f(2, 150., 0.)]
            ),
            Gesture::Pinch {
                zoom: 0.5,
                dx: 0.,
                dy: 0.
            }
        );
    }

    #[test]
    fn two_fingers_moving_together_pan() {
        assert_eq!(
            gesture(
                &[f(1, 0., 0.), f(2, 100., 0.)],
                &[f(1, 10., 20.), f(2, 110., 20.)]
            ),
            Gesture::Pinch {
                zoom: 1.,
                dx: 10.,
                dy: 20.
            }
        );
    }

    #[test]
    fn adding_or_lifting_a_finger_never_jumps() {
        assert_eq!(
            gesture(&[f(1, 0., 0.)], &[f(1, 50., 0.), f(2, 90., 90.)]),
            Gesture::Still
        );
        assert_eq!(
            gesture(&[f(1, 0., 0.), f(2, 100., 0.)], &[f(1, 60., 0.)]),
            Gesture::Still
        );
        assert_eq!(gesture(&[f(1, 0., 0.)], &[f(7, 40., 0.)]), Gesture::Still);
    }

    #[test]
    fn odd_input_is_still_or_finite() {
        assert_eq!(gesture(&[], &[]), Gesture::Still);
        assert_eq!(gesture(&[f(1, 5., 5.)], &[f(1, 5., 5.)]), Gesture::Still);
        let three = [f(1, 0., 0.), f(2, 1., 1.), f(3, 2., 2.)];
        assert_eq!(
            gesture(&three, &three.map(|g| Finger { x: g.x + 9., ..g })),
            Gesture::Still
        );
        // two fingers on one spot, then apart: no NaN / infinity
        let Gesture::Pinch { zoom, .. } = gesture(
            &[f(1, 0., 0.), f(2, 0., 0.)],
            &[f(1, 0., 0.), f(2, 80., 0.)],
        ) else {
            panic!("expected a pinch")
        };
        assert!(zoom.is_finite() && zoom > 0.);
    }

    #[test]
    fn double_tap() {
        let mut t = Taps::default();
        t.press(1, 0., 100., 100.);
        assert!(!t.release(1, 120., 101., 100.));
        t.press(2, 250., 104., 98.);
        assert!(t.release(2, 360., 104., 98.));
        // a third tap starts afresh
        t.press(3, 450., 104., 98.);
        assert!(!t.release(3, 500., 104., 98.));
    }

    #[test]
    fn not_a_double_tap() {
        // too slow between taps
        let mut t = Taps::default();
        t.press(1, 0., 0., 0.);
        t.release(1, 100., 0., 0.);
        t.press(2, 500., 0., 0.);
        assert!(!t.release(2, 560., 0., 0.));
        // a long press isn't a tap
        let mut t = Taps::default();
        t.press(1, 0., 0., 0.);
        t.release(1, 400., 0., 0.);
        t.press(2, 450., 0., 0.);
        assert!(!t.release(2, 500., 0., 0.));
        // a drag isn't a tap
        let mut t = Taps::default();
        t.press(1, 0., 0., 0.);
        t.release(1, 100., 40., 0.);
        t.press(2, 150., 40., 0.);
        assert!(!t.release(2, 200., 40., 0.));
        // far apart
        let mut t = Taps::default();
        t.press(1, 0., 0., 0.);
        t.release(1, 100., 0., 0.);
        t.press(2, 150., 200., 0.);
        assert!(!t.release(2, 200., 200., 0.));
        // a second finger during the tap
        let mut t = Taps::default();
        t.press(1, 0., 0., 0.);
        t.press(2, 20., 50., 0.);
        t.release(2, 60., 50., 0.);
        t.release(1, 80., 0., 0.);
        t.press(3, 150., 0., 0.);
        assert!(!t.release(3, 200., 0., 0.));
        // lifting one of two fingers leaves the other down: still not a tap
        let mut t = Taps::default();
        t.press(1, 0., 0., 0.);
        t.press(2, 10., 50., 0.);
        t.release(2, 30., 50., 0.);
        t.press(3, 40., 0., 0.);
        t.release(3, 60., 0., 0.);
        t.press(4, 100., 0., 0.);
        assert!(!t.release(4, 120., 0., 0.));
    }
}
