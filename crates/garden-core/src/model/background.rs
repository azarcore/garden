use kurbo::Vec2;
use serde::{Deserialize, Serialize};

use crate::geometry::Point;

/// A photo, aerial image or survey plan shown under the design for tracing.
/// `image_ref` is an opaque key; the image bytes are stored elsewhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackgroundImage {
    pub image_ref: String,
    pub mime: String,
    pub width_px: u32,
    pub height_px: u32,
    /// World position (mm) of the image's top-left pixel.
    pub origin: Point,
    pub mm_per_px: f64,
    pub rotation: f64,
    pub opacity: f32,
    pub locked: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CalibrationError {
    #[error("pick two different points")]
    SamePoint,
    #[error("distance must be a positive length")]
    InvalidDistance,
}

impl BackgroundImage {
    pub fn is_finite(&self) -> bool {
        self.origin.x.is_finite()
            && self.origin.y.is_finite()
            && self.mm_per_px.is_finite()
            && self.rotation.is_finite()
            && self.opacity.is_finite()
    }

    /// Two image pixel positions `a_px` and `b_px` are `real_distance_mm` apart: set the scale.
    pub fn calibrate(
        &mut self,
        a_px: Point,
        b_px: Point,
        real_distance_mm: f64,
    ) -> Result<(), CalibrationError> {
        let px = a_px.distance(b_px);
        if !px.is_finite() || px < 1e-9 {
            return Err(CalibrationError::SamePoint);
        }
        if !real_distance_mm.is_finite() || real_distance_mm <= 0.0 {
            return Err(CalibrationError::InvalidDistance);
        }
        self.mm_per_px = real_distance_mm / px;
        Ok(())
    }

    /// Inverse of `px_to_world`.
    pub fn world_to_px(&self, world: Point) -> Point {
        let v = world - self.origin;
        let (s, c) = self.rotation.sin_cos();
        // rotate back by -rotation, then undo the scale
        let unrotated = Vec2::new(v.x * c + v.y * s, -v.x * s + v.y * c);
        (unrotated / self.mm_per_px).to_point()
    }

    /// Set the scale so pixels `a_px`..`b_px` are `real_distance_mm` apart,
    /// keeping the world position of `a_px` unchanged (the image grows or
    /// shrinks about that point). On error nothing changes.
    pub fn calibrate_about(
        &mut self,
        a_px: Point,
        b_px: Point,
        real_distance_mm: f64,
    ) -> Result<(), CalibrationError> {
        let anchor = self.px_to_world(a_px);
        let mut next = self.clone();
        next.calibrate(a_px, b_px, real_distance_mm)?;
        next.origin += anchor - next.px_to_world(a_px);
        *self = next;
        Ok(())
    }

    /// Width and height of the whole image in world mm.
    pub fn world_size(&self) -> (f64, f64) {
        (
            f64::from(self.width_px) * self.mm_per_px,
            f64::from(self.height_px) * self.mm_per_px,
        )
    }

    pub fn px_to_world(&self, px: Point) -> Point {
        let v = px.to_vec2() * self.mm_per_px;
        let (s, c) = self.rotation.sin_cos();
        self.origin + Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_calibration() {
        let mut b = BackgroundImage {
            image_ref: "k".into(),
            mime: "image/png".into(),
            width_px: 1000,
            height_px: 800,
            origin: Point::ZERO,
            mm_per_px: 1.,
            rotation: 0.,
            opacity: 0.5,
            locked: false,
        };
        b.calibrate(Point::new(100., 100.), Point::new(400., 500.), 10_000.)
            .unwrap(); // 500 px apart
        assert!((b.mm_per_px - 20.).abs() < 1e-9);
        assert_eq!(b.px_to_world(Point::new(10., 0.)), Point::new(200., 0.));
        assert_eq!(
            b.calibrate(Point::ZERO, Point::ZERO, 10.),
            Err(CalibrationError::SamePoint)
        );
        assert_eq!(
            b.calibrate(Point::ZERO, Point::new(1., 0.), -5.),
            Err(CalibrationError::InvalidDistance)
        );
    }

    fn bg() -> BackgroundImage {
        BackgroundImage {
            image_ref: "k".into(),
            mime: "image/png".into(),
            width_px: 1000,
            height_px: 800,
            origin: Point::new(500., 300.),
            mm_per_px: 10.,
            rotation: 0.3,
            opacity: 0.5,
            locked: false,
        }
    }

    #[test]
    fn world_and_pixels_round_trip() {
        let b = bg();
        let px = Point::new(123., 456.);
        assert!((b.world_to_px(b.px_to_world(px)) - px).hypot() < 1e-9);
    }

    #[test]
    fn calibrating_keeps_the_first_point_still() {
        let mut b = bg();
        let a = Point::new(100., 100.);
        let before = b.px_to_world(a);
        b.calibrate_about(a, Point::new(400., 500.), 10_000.)
            .unwrap(); // 500 px apart
        assert!((b.mm_per_px - 20.).abs() < 1e-9);
        assert!((b.px_to_world(a) - before).hypot() < 1e-6);
        assert_eq!(
            b.calibrate_about(a, a, 1000.),
            Err(CalibrationError::SamePoint)
        );
        assert_eq!(
            b.calibrate_about(a, Point::new(0., 0.), 0.),
            Err(CalibrationError::InvalidDistance)
        );
        assert!((b.mm_per_px - 20.).abs() < 1e-9); // failed calls change nothing
    }

    #[test]
    fn world_size_scales_pixels() {
        assert_eq!(bg().world_size(), (10_000., 8_000.));
    }
}
