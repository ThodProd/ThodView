use eframe::egui::{Pos2, Rect, Vec2, pos2};

const MAX_ZOOM: f32 = 64.0;

pub struct View {
    pub scale: f32,
    pub origin: Pos2,
    fitted: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            scale: 1.0,
            origin: Pos2::ZERO,
            fitted: true,
        }
    }
}

impl View {
    fn minimum(canvas: Rect, size: Vec2) -> f32 {
        (canvas.width() / size.x)
            .min(canvas.height() / size.y)
            .clamp(0.0001, 16.0)
    }

    pub fn fit(&mut self, canvas: Rect, size: Vec2) {
        self.scale = Self::minimum(canvas, size);
        self.origin = canvas.center() - size * self.scale * 0.5;
        self.fitted = true;
    }

    pub fn actual(&mut self, canvas: Rect, size: Vec2, pixels_per_point: f32) {
        self.scale = (1.0 / pixels_per_point).max(Self::minimum(canvas, size));
        self.origin = canvas.center() - size * self.scale * 0.5;
        self.fitted = self.scale <= Self::minimum(canvas, size);
    }

    pub fn resize(&mut self, old: Rect, canvas: Rect, size: Vec2) {
        if self.fitted || self.scale <= Self::minimum(canvas, size) {
            self.fit(canvas, size);
        } else {
            self.origin += canvas.center() - old.center();
            self.clamp(canvas, size);
        }
    }

    pub fn zoom(&mut self, canvas: Rect, size: Vec2, anchor: Pos2, factor: f32) {
        let minimum = Self::minimum(canvas, size);
        let next = (self.scale * factor).clamp(minimum, MAX_ZOOM);
        if next <= minimum * 1.000001 {
            self.fit(canvas, size);
            return;
        }
        let pixel = (anchor - self.origin) / self.scale;
        self.origin = anchor - pixel * next;
        self.scale = next;
        self.fitted = false;
        self.clamp(canvas, size);
    }

    pub fn is_zoomed(&self, canvas: Rect, size: Vec2) -> bool {
        self.scale > Self::minimum(canvas, size) * 1.000001
    }

    pub fn pan(&mut self, canvas: Rect, size: Vec2, delta: Vec2) {
        self.origin += delta;
        self.clamp(canvas, size);
    }

    fn clamp(&mut self, canvas: Rect, size: Vec2) {
        let extent = size * self.scale;
        self.origin = pos2(
            clamp_axis(self.origin.x, canvas.min.x, canvas.max.x, extent.x),
            clamp_axis(self.origin.y, canvas.min.y, canvas.max.y, extent.y),
        );
    }
}

fn clamp_axis(origin: f32, min: f32, max: f32, extent: f32) -> f32 {
    if extent <= max - min {
        min + (max - min - extent) * 0.5
    } else {
        origin.clamp(max - extent, min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    #[test]
    fn zoom_out_after_panning_returns_to_center_and_stays_there() {
        let canvas = Rect::from_min_size(pos2(10.0, 44.0), vec2(800.0, 480.0));
        let size = vec2(2000.0, 1000.0);
        let mut view = View::default();
        view.fit(canvas, size);
        let baseline = view.origin;
        view.zoom(canvas, size, pos2(110.0, 124.0), 30.0);
        view.pan(canvas, size, vec2(-500.0, -250.0));
        for _ in 0..40 {
            view.zoom(canvas, size, canvas.min, 1.0 / 1.2);
        }
        assert_eq!(view.scale, 0.4);
        assert_eq!(view.origin, baseline);
        let larger = canvas.expand(50.0);
        view.resize(canvas, larger, size);
        assert_eq!(view.origin + size * view.scale * 0.5, larger.center());
    }
}
