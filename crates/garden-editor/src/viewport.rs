//! Mapping between world millimetres and screen pixels.

use kurbo::{Point, Rect, Vec2};

/// What part of the garden is on screen. `scale` is screen px per mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub center: Point,
    pub scale: f64,
    pub width: f64,
    pub height: f64,
}

impl Viewport {
    pub const MIN_SCALE: f64 = 0.005;
    pub const MAX_SCALE: f64 = 5.0;
    pub const DEFAULT_SCALE: f64 = 0.05;

    pub fn new(width: f64, height: f64) -> Viewport {
        Viewport {
            center: Point::ZERO,
            scale: Self::DEFAULT_SCALE,
            width,
            height,
        }
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
            self.width = width;
            self.height = height;
        }
    }

    fn half(&self) -> Vec2 {
        Vec2::new(self.width / 2.0, self.height / 2.0)
    }

    pub fn world_to_screen(&self, p: Point) -> Point {
        ((p - self.center) * self.scale + self.half()).to_point()
    }

    pub fn screen_to_world(&self, s: Point) -> Point {
        self.center + (s.to_vec2() - self.half()) / self.scale
    }

    /// Zoom by `factor`, keeping the world point under `screen` in place.
    pub fn zoom_at(&mut self, screen: Point, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.screen_to_world(screen);
        self.scale = (self.scale * factor).clamp(Self::MIN_SCALE, Self::MAX_SCALE);
        // keep `anchor` under `screen`
        self.center = anchor - (screen.to_vec2() - self.half()) / self.scale;
    }

    /// Move the view so content follows a pointer drag of `screen_delta` px.
    pub fn pan_by(&mut self, screen_delta: Vec2) {
        self.center -= screen_delta / self.scale;
    }

    /// Show the whole of `world` with `margin_px` around it.
    /// A zero-area rect only recentres.
    pub fn fit(&mut self, world: Rect, margin_px: f64) {
        self.center = world.center();
        let (w, h) = (world.width(), world.height());
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let avail_w = (self.width - 2.0 * margin_px).max(1.0);
        let avail_h = (self.height - 2.0 * margin_px).max(1.0);
        self.scale = (avail_w / w)
            .min(avail_h / h)
            .clamp(Self::MIN_SCALE, Self::MAX_SCALE);
    }

    pub fn px_to_mm(&self, px: f64) -> f64 {
        px / self.scale
    }

    /// SVG transform taking world mm to screen px.
    pub fn svg_transform(&self) -> String {
        let t = self.half() - self.center.to_vec2() * self.scale;
        format!("matrix({} 0 0 {} {} {})", self.scale, self.scale, t.x, t.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_world_to_screen_and_back() {
        let v = Viewport::new(1000., 500.);
        assert_eq!(v.world_to_screen(Point::ZERO), Point::new(500., 250.));
        assert_eq!(
            v.world_to_screen(Point::new(2000., 1000.)),
            Point::new(600., 300.)
        );
        let p = Point::new(1234.5, -678.);
        let back = v.screen_to_world(v.world_to_screen(p));
        assert!((back - p).hypot() < 1e-9);
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let mut v = Viewport::new(1000., 500.);
        v.zoom_at(Point::new(600., 300.), 2.0);
        assert!((v.scale - 0.1).abs() < 1e-12);
        assert!(
            (v.world_to_screen(Point::new(2000., 1000.)) - Point::new(600., 300.)).hypot() < 1e-9
        );
        v.zoom_at(Point::new(0., 0.), 1e9);
        assert_eq!(v.scale, Viewport::MAX_SCALE);
        v.zoom_at(Point::new(0., 0.), 0.0);
        assert_eq!(v.scale, Viewport::MAX_SCALE); // non-positive factor ignored
    }

    #[test]
    fn pan_moves_content_with_pointer() {
        let mut v = Viewport::new(1000., 500.);
        v.pan_by(Vec2::new(100., 0.));
        assert_eq!(v.world_to_screen(Point::ZERO), Point::new(600., 250.));
    }

    #[test]
    fn fit_shows_whole_rect() {
        let mut v = Viewport::new(1000., 500.);
        v.fit(Rect::new(0., 0., 20_000., 10_000.), 50.);
        assert!((v.scale - 0.04).abs() < 1e-12);
        assert_eq!(v.center, Point::new(10_000., 5000.));
        v.fit(Rect::new(7., 7., 7., 7.), 50.);
        assert_eq!(v.center, Point::new(7., 7.));
        assert!((v.scale - 0.04).abs() < 1e-12);
    }

    #[test]
    fn svg_transform_and_px_to_mm() {
        let v = Viewport::new(1000., 500.);
        assert_eq!(v.svg_transform(), "matrix(0.05 0 0 0.05 500 250)");
        assert!((v.px_to_mm(8.) - 160.).abs() < 1e-9);
    }
}
