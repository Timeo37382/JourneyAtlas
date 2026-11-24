//! Minimap placement and world <-> screen transforms.

use crate::config::{Corner, Preset, Shape};

pub const MARGIN: f32 = 6.0;

/// The minimap's square on screen, in GUI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub size: f32,
}

impl Frame {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.size * 0.5, self.y + self.size * 0.5)
    }
}

pub fn minimap_frame(p: &Preset, screen_w: f32, screen_h: f32) -> Frame {
    let size = p.size.min(screen_w * 0.45).min(screen_h * 0.45).max(32.0);
    // Room under the map for the info lines.
    let info = 22.0;
    let (x, y) = match p.corner {
        Corner::TopRight => (screen_w - size - MARGIN, MARGIN),
        Corner::TopLeft => (MARGIN, MARGIN),
        Corner::BottomRight => (screen_w - size - MARGIN, screen_h - size - MARGIN - info),
        Corner::BottomLeft => (MARGIN, screen_h - size - MARGIN - info),
        Corner::TopCenter => ((screen_w - size) / 2.0, MARGIN),
    };
    Frame { x, y, size }
}

/// Maps world x/z to screen around a centre, with an optional rotation (radians, player yaw
/// for a rotating minimap) and a scale in pixels per block.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub cx: f32,
    pub cy: f32,
    pub wx: f64,
    pub wz: f64,
    pub scale: f32,
    pub angle: f32,
}

impl View {
    pub fn to_screen(&self, x: f64, z: f64) -> (f32, f32) {
        let dx = ((x - self.wx) as f32) * self.scale;
        let dz = ((z - self.wz) as f32) * self.scale;
        let (s, c) = self.angle.sin_cos();
        (self.cx + dx * c - dz * s, self.cy + dx * s + dz * c)
    }

    pub fn to_world(&self, sx: f32, sy: f32) -> (f64, f64) {
        let dx = sx - self.cx;
        let dy = sy - self.cy;
        let (s, c) = (-self.angle).sin_cos();
        let rx = dx * c - dy * s;
        let rz = dx * s + dy * c;
        (self.wx + f64::from(rx / self.scale), self.wz + f64::from(rz / self.scale))
    }
}

/// Is a screen point inside the minimap's visible shape?
pub fn inside(frame: &Frame, shape: Shape, x: f32, y: f32) -> bool {
    match shape {
        Shape::Square => x >= frame.x && y >= frame.y && x < frame.x + frame.size && y < frame.y + frame.size,
        Shape::Circle => {
            let (cx, cy) = frame.center();
            let r = frame.size * 0.5;
            (x - cx).powi(2) + (y - cy).powi(2) <= r * r
        }
    }
}

/// Pulls a point that is outside the shape back onto its edge (used for off-map waypoints).
pub fn clamp_to_edge(frame: &Frame, shape: Shape, x: f32, y: f32, inset: f32) -> (f32, f32) {
    let (cx, cy) = frame.center();
    let r = frame.size * 0.5 - inset;
    match shape {
        Shape::Circle => {
            let (dx, dy) = (x - cx, y - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d <= r || d == 0.0 { (x, y) } else { (cx + dx / d * r, cy + dy / d * r) }
        }
        Shape::Square => (x.clamp(cx - r, cx + r), y.clamp(cy - r, cy + r)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_world_round_trip() {
        let v = View { cx: 100.0, cy: 80.0, wx: 12.5, wz: -40.0, scale: 2.0, angle: 0.7 };
        let (sx, sy) = v.to_screen(30.0, -10.0);
        let (x, z) = v.to_world(sx, sy);
        assert!((x - 30.0).abs() < 1e-3 && (z + 10.0).abs() < 1e-3);
    }
}
