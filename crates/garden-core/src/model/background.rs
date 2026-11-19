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
}
