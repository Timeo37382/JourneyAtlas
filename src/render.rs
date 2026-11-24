//! The map image (one RGBA picture uploaded to the game's mod image slot) and the minimap drawing.

use crate::config::{Config, Shape};
use crate::draw::{Canvas, Measure, Op, tex};
use crate::geometry::{Frame, View, clamp_to_edge, inside, minimap_frame};
use crate::shading::{MapType, shade};
use crate::store::MapStore;
use crate::tiles::Layer;
use crate::i18n::{t, tf};
use crate::waypoints::{Kind, Waypoint};

/// The game's mod image slot is this large.
pub const MAX_TEXELS: i32 = 1024;

/// Which part of the world the uploaded image shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageKey {
    pub dim: String,
    pub layer: Layer,
    pub ty: MapType,
    /// World x/z of texel (0, 0).
    pub x0: i32,
    pub z0: i32,
    /// Blocks per texel.
    pub step: i32,
    pub w: i32,
    pub h: i32,
}

impl ImageKey {
    /// An image covering `[x0, x0 + span_x) x [z0, z0 + span_z)`, snapped so small moves reuse it.
    pub fn covering(dim: &str, layer: Layer, ty: MapType, cx: f64, cz: f64, half_x: f64, half_z: f64) -> Self {
        Self::covering_at(dim, layer, ty, cx, cz, half_x, half_z, 1.0)
    }

    /// As `covering`, with `pixels_per_block` on screen: zoomed out, one texel covers several
    /// blocks so the image never has more texels than the screen has pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn covering_at(dim: &str, layer: Layer, ty: MapType, cx: f64, cz: f64, half_x: f64, half_z: f64, pixels_per_block: f32) -> Self {
        let span = (half_x.max(half_z) * 2.0).max(16.0);
        let by_size = ((span / f64::from(MAX_TEXELS - 64)).ceil() as i32).max(1);
        let by_zoom = (1.0 / pixels_per_block.max(0.01)).floor() as i32;
        let step = (by_size.max(by_zoom).max(1) as u32).next_power_of_two() as i32;
        let snap = 16 * step;
        let w = (((half_x * 2.0) as i32 + 2 * snap) / step).clamp(1, MAX_TEXELS);
        let h = (((half_z * 2.0) as i32 + 2 * snap) / step).clamp(1, MAX_TEXELS);
        let x0 = ((cx as i32 - (w * step) / 2).div_euclid(snap)) * snap;
        let z0 = ((cz as i32 - (h * step) / 2).div_euclid(snap)) * snap;
        ImageKey { dim: dim.to_owned(), layer, ty, x0, z0, step, w, h }
    }

    /// Whether the image still covers a view of this size around this centre, at the same detail.
    pub fn covers(&self, other: &ImageKey) -> bool {
        self.dim == other.dim
            && self.layer == other.layer
            && self.ty == other.ty
            && self.step == other.step
            && other.x0 >= self.x0
            && other.z0 >= self.z0
            && other.x0 + other.w * other.step <= self.x0 + self.w * self.step
            && other.z0 + other.h * other.step <= self.z0 + self.h * self.step
    }

    pub fn texel(&self, x: f64, z: f64) -> [f32; 2] {
        [
            ((x - f64::from(self.x0)) / f64::from(self.step)) as f32,
            ((z - f64::from(self.z0)) / f64::from(self.step)) as f32,
        ]
    }
}

/// Builds the RGBA pixels for `key`. Regions are looked up once each, then read directly.
pub fn build_image(store: &mut MapStore, key: &ImageKey) -> Vec<u8> {
    use crate::tiles::{REGION_SIZE, Region};
    let (w, h) = (key.w as usize, key.h as usize);
    let mut out = vec![0u8; w * h * 4];
    // One block of margin west and north for the relief.
    let x_min = key.x0 - key.step;
    let z_min = key.z0 - key.step;
    let x_max = key.x0 + key.w * key.step;
    let z_max = key.z0 + key.h * key.step;
    let (rx0, rz0) = (x_min.div_euclid(REGION_SIZE), z_min.div_euclid(REGION_SIZE));
    let (rx1, rz1) = (x_max.div_euclid(REGION_SIZE), z_max.div_euclid(REGION_SIZE));
    for rz in rz0..=rz1 {
        for rx in rx0..=rx1 {
            store.ensure(&key.dim, key.layer, rx, rz);
        }
    }
    let cols = (rx1 - rx0 + 1) as usize;
    let mut regions: Vec<Option<&Region>> = Vec::with_capacity(cols * (rz1 - rz0 + 1) as usize);
    for rz in rz0..=rz1 {
        for rx in rx0..=rx1 {
            regions.push(store.tiles.regions.get(&(key.dim.clone(), key.layer, rx, rz)));
        }
    }
    let cell = |x: i32, z: i32| -> Option<crate::tiles::Cell> {
        let rx = x.div_euclid(REGION_SIZE) - rx0;
        let rz = z.div_euclid(REGION_SIZE) - rz0;
        regions
            .get(rz as usize * cols + rx as usize)
            .copied()
            .flatten()
            .map(|r| r.get(x, z))
            .filter(|c| c.explored())
    };
    for tz in 0..key.h {
        let z = key.z0 + tz * key.step;
        for tx in 0..key.w {
            let x = key.x0 + tx * key.step;
            let Some(c) = cell(x, z) else { continue };
            let west = cell(x - key.step, z).map(|c| c.y);
            let north = cell(x, z - key.step).map(|c| c.y);
            let argb = shade(c, west, north, key.ty);
            let i = (tz as usize * w + tx as usize) * 4;
            out[i] = (argb >> 16) as u8;
            out[i + 1] = (argb >> 8) as u8;
            out[i + 2] = argb as u8;
            out[i + 3] = 0xFF;
        }
    }
    out
}

/// One waypoint as seen in the world: where it lands on screen and how far it is.
pub struct Marker {
    pub sx: f32,
    pub sy: f32,
    pub name: String,
    pub dist: f64,
    pub color: u32,
    pub death: bool,
    /// The beacon beam: screen points going up from the waypoint, with their width in pixels.
    pub beam: Vec<([f32; 2], f32)>,
}

/// In-world waypoints: the icon always, the name and distance when you look
/// towards it.
pub fn draw_world_waypoints(c: &mut Canvas, m: &dyn Measure, markers: &[Marker], gui_w: f32, gui_h: f32) {
    let (cx, cy) = (gui_w / 2.0, gui_h / 2.0);
    // Beams first, under every icon: a bright core inside a softer glow, fading upwards.
    for mk in markers {
        for pair in mk.beam.windows(2) {
            let ((a, wa), (b, wb)) = (pair[0], pair[1]);
            let fade = |alpha: f32, k: usize| -> u32 {
                let t = 1.0 - k as f32 / mk.beam.len().max(1) as f32;
                ((alpha * t) as u32) << 24 | mk.color
            };
            let k = mk.beam.iter().position(|p| p.0 == a).unwrap_or(0);
            for (scale, alpha) in [(1.0_f32, 70.0_f32), (0.4, 150.0)] {
                let (ha, hb) = (wa * scale / 2.0, wb * scale / 2.0);
                c.quad([[a[0] - ha, a[1]], [a[0] + ha, a[1]], [b[0] + hb, b[1]], [b[0] - hb, b[1]]], fade(alpha, k));
            }
        }
    }
    for mk in markers {
        let col = 0xFF00_0000 | mk.color;
        let size = 12.0;
        let t = if mk.death { tex::WP_DEATH } else { tex::WP_DIAMOND };
        c.sprite(t, mk.sx - size / 2.0, mk.sy - size / 2.0, size, size, col);
        let near_centre = (mk.sx - cx).abs() < gui_w * 0.12 && (mk.sy - cy).abs() < gui_h * 0.2;
        if near_centre {
            let dist = if mk.dist >= 1000.0 { format!("{:.1} km", mk.dist / 1000.0) } else { format!("{} m", mk.dist.round() as i64) };
            let _ = tf;
            for (i, line) in [mk.name.as_str(), dist.as_str()].into_iter().enumerate() {
                let w = m.text_width(line);
                let y = mk.sy - size / 2.0 - 22.0 + i as f32 * 10.0;
                c.fill(mk.sx - w / 2.0 - 2.0, y - 1.0, w + 4.0, 10.0, 0x9000_0000);
                c.text((mk.sx - w / 2.0).round(), y, line, if i == 0 { col } else { 0xFFFF_FFFF });
            }
        }
    }
}

/// A mob's face: its texture's atlas name and the face's corners in that texture (0..1).
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    pub sprite: String,
    pub uv: [f32; 4],
}

/// A creature near the player. `kind`: 0 hostile, 1 passive, 2 player.
#[derive(Clone, Debug, PartialEq)]
pub struct Ent {
    pub x: f64,
    pub z: f64,
    pub kind: u8,
    pub name: String,
    pub head: Option<Head>,
}

/// One creature on a map: its head framed by a thin border (red for monsters), or a dot.
pub fn draw_entity(c: &mut Canvas, e: &Ent, sx: f32, sy: f32, size: f32, heads: bool) {
    match (&e.head, heads) {
        (Some(h), true) => {
            let border = match e.kind {
                1 => 0xFF20_2020,
                k => entity_color(k),
            };
            c.fill(sx - size / 2.0 - 1.0, sy - size / 2.0 - 1.0, size + 2.0, size + 2.0, border);
            c.ops.push(Op::Head { sprite: h.sprite.clone(), uv: h.uv, dst: [sx - size / 2.0, sy - size / 2.0, size, size] });
        }
        _ => {
            let d = if e.kind == 2 { size * 1.2 } else { size * 0.85 };
            c.sprite(tex::DOT, sx - d / 2.0, sy - d / 2.0, d, d, entity_color(e.kind));
        }
    }
}

/// What is around the player, for the minimap.
pub struct Scene<'a> {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Degrees, Minecraft's: 0 faces south (+z), 90 west.
    pub yaw: f32,
    pub dim: &'a str,
    pub biome: &'a str,
    pub waypoints: &'a [Waypoint],
    /// (x, z, kind) of nearby creatures: 0 hostile, 1 passive, 2 player.
    pub entities: &'a [Ent],
    /// Extra info lines under the minimap (time, weather).
    pub extra: &'a [String],
}

/// Heading arrow angle on screen (0 = up, clockwise), from Minecraft's yaw.
pub fn heading(yaw: f32) -> f32 {
    (yaw + 180.0).rem_euclid(360.0).to_radians()
}

/// Rotation for the player marker, whose tip points up in the texture.
pub fn marker_angle(yaw: f32) -> f32 {
    heading(yaw)
}

pub fn entity_color(kind: u8) -> u32 {
    match kind {
        0 => 0xFFFF_3030,
        2 => 0xFF30_A0FF,
        3 => 0xFF30_D040,
        4 => 0xFFB0_70FF,
        _ => 0xFFE0_E0E0,
    }
}

/// The view the minimap uses, so the bridge can size the image it needs.
pub fn minimap_view(cfg: &Config, sc: &Scene<'_>, gui_w: f32, gui_h: f32) -> (Frame, View) {
    let p = cfg.preset();
    let frame = minimap_frame(p, gui_w, gui_h);
    let (cx, cy) = frame.center();
    let angle = if p.rotate { -heading(sc.yaw) } else { 0.0 };
    let view = View { cx, cy, wx: sc.x, wz: sc.z, scale: crate::config::ZOOMS[p.zoom], angle };
    (frame, view)
}

/// The polygon the map image fills: the square, or a circle as 64 segments.
fn outline_points(frame: &Frame, shape: Shape) -> Vec<[f32; 2]> {
    match shape {
        Shape::Square => vec![
            [frame.x, frame.y],
            [frame.x + frame.size, frame.y],
            [frame.x + frame.size, frame.y + frame.size],
            [frame.x, frame.y + frame.size],
        ],
        Shape::Circle => {
            let (cx, cy) = frame.center();
            let r = frame.size / 2.0;
            (0..64)
                .map(|i| {
                    let a = i as f32 / 64.0 * std::f32::consts::TAU;
                    [cx + r * a.cos(), cy + r * a.sin()]
                })
                .collect()
        }
    }
}

/// Fills a convex polygon with the map image as a fan of quads.
pub fn image_polygon(c: &mut Canvas, pts: &[[f32; 2]], view: &View, key: &ImageKey, alpha: u8) {
    let tex = |p: [f32; 2]| {
        let (x, z) = view.to_world(p[0], p[1]);
        key.texel(x, z)
    };
    if pts.len() == 4 {
        c.image([pts[0], pts[1], pts[2], pts[3]], [tex(pts[0]), tex(pts[1]), tex(pts[2]), tex(pts[3])], alpha);
        return;
    }
    let centre = [view.cx, view.cy];
    let tc = tex(centre);
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        c.image([centre, a, b, centre], [tc, tex(a), tex(b), tc], alpha);
    }
}

fn compass(c: &mut Canvas, m: &dyn Measure, frame: &Frame, shape: Shape, view: &View) {
    let point = if shape == Shape::Circle { tex::CI_COMPASS } else { tex::SQ_COMPASS };
    for (key, dx, dz) in [("dir_n", 0.0, -1.0), ("dir_e", 1.0, 0.0), ("dir_s", 0.0, 1.0), ("dir_w", -1.0, 0.0)] {
        let label = t(key);
        let label = label.as_str();
        let (s, co) = view.angle.sin_cos();
        let (vx, vy) = (dx * co - dz * s, dx * s + dz * co);
        let (cx, cy) = frame.center();
        let far = (cx + vx * frame.size, cy + vy * frame.size);
        let (x, y) = clamp_to_edge(frame, shape, far.0, far.1, 0.0);
        let size = 14.0;
        c.sprite(point, x - size / 2.0, y - size / 2.0, size, size, 0xFFFF_FFFF);
        let w = m.text_width(label);
        c.text((x - w / 2.0).round(), (y - 3.5).round(), label, if key == "dir_n" { 0xFFFF_5555 } else { 0xFFFF_FFFF });
        let _ = &label;
    }
}

/// The minimap with its markers and the info lines under it.
pub fn draw_minimap(c: &mut Canvas, m: &dyn Measure, cfg: &Config, sc: &Scene<'_>, key: Option<&ImageKey>, gui_w: f32, gui_h: f32) {
    let p = *cfg.preset();
    let (frame, view) = minimap_view(cfg, sc, gui_w, gui_h);
    let pts = outline_points(&frame, p.shape);

    // Background, map, then the theme's frame over it.
    match p.shape {
        Shape::Square => c.fill(frame.x, frame.y, frame.size, frame.size, ((cfg.map_opacity * 255.0) as u32) << 24),
        Shape::Circle => {
            let (cx, cy) = frame.center();
            c.disc(cx, cy, frame.size / 2.0, ((cfg.map_opacity * 255.0) as u32) << 24);
        }
    }
    if let Some(key) = key {
        image_polygon(c, &pts, &view, key, (cfg.map_opacity.clamp(0.2, 1.0) * 255.0) as u8);
    }
    match p.shape {
        Shape::Square => {
            let (x, y, s, b) = (frame.x, frame.y, frame.size, 8.0);
            c.sprite(tex::SQ_TOP, x, y - b, s, b, 0xFFFF_FFFF);
            c.sprite(tex::SQ_BOTTOM, x, y + s, s, b, 0xFFFF_FFFF);
            c.sprite(tex::SQ_LEFT, x - b, y, b, s, 0xFFFF_FFFF);
            c.sprite(tex::SQ_RIGHT, x + s, y, b, s, 0xFFFF_FFFF);
            c.sprite(tex::SQ_TL, x - b, y - b, b, b, 0xFFFF_FFFF);
            c.sprite(tex::SQ_TR, x + s, y - b, b, b, 0xFFFF_FFFF);
            c.sprite(tex::SQ_BL, x - b, y + s, b, b, 0xFFFF_FFFF);
            c.sprite(tex::SQ_BR, x + s, y + s, b, b, 0xFFFF_FFFF);
        }
        Shape::Circle => {
            // The rim texture is 256 px for a 256 px map with its border inside.
            c.sprite(tex::CI_RIM, frame.x, frame.y, frame.size, frame.size, 0xFFFF_FFFF);
        }
    }

    // Entities.
    for e in sc.entities {
        if !cfg.shows(e.kind) {
            continue;
        }
        let (sx, sy) = view.to_screen(e.x, e.z);
        if inside(&frame, p.shape, sx, sy) {
            draw_entity(c, e, sx, sy, 8.0 * cfg.mob_scale, cfg.mob_heads);
        }
    }

    // Waypoints: on the map, or pinned to the edge when far.
    if cfg.show_waypoints {
        for w in sc.waypoints.iter().filter(|w| w.enabled && w.dim == sc.dim) {
            let (sx, sy) = view.to_screen(f64::from(w.x) + 0.5, f64::from(w.z) + 0.5);
            let col = 0xFF00_0000 | w.color;
            if inside(&frame, p.shape, sx, sy) {
                let t = if w.kind == Kind::Death { tex::WP_DEATH } else { tex::WP_DIAMOND };
                c.sprite(t, sx - 5.0, sy - 5.0, 10.0, 10.0, col);
            } else {
                // The off-map marker, pointing towards the waypoint.
                let (ex, ey) = clamp_to_edge(&frame, p.shape, sx, sy, 5.0);
                let (cx, cy) = frame.center();
                let angle = (sy - cy).atan2(sx - cx) + std::f32::consts::FRAC_PI_2;
                c.sprite_rotated(tex::OFFSCREEN, ex, ey, 10.0, angle, col);
            }
        }
    }

    // Player.
    let arrow = if p.rotate { 0.0 } else { marker_angle(sc.yaw) };
    c.sprite_rotated(tex::PLAYER, view.cx, view.cy, 16.0, arrow, 0xFFFF_FFFF);

    if p.show_compass {
        compass(c, m, &frame, p.shape, &view);
    }

    // Info lines.
    let mut y = frame.y + frame.size + 8.0;
    let (cx, _) = frame.center();
    let mut line = |c: &mut Canvas, s: &str| {
        let w = m.text_width(s);
        c.fill(cx - w / 2.0 - 3.0, y - 2.0, w + 6.0, 11.0, 0xB000_0000);
        c.text((cx - w / 2.0).round(), y, s, 0xFFFF_FFFF);
        y += 10.0;
    };
    if p.show_coords {
        line(c, &format!("{}, {}, {}", sc.x.floor() as i64, sc.y.floor() as i64, sc.z.floor() as i64));
    }
    if p.show_biome && !sc.biome.is_empty() {
        line(c, sc.biome);
    }
    for l in sc.extra {
        line(c, l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::Approx;
    use crate::tiles::Cell;

    #[test]
    fn image_is_built_from_tiles() {
        let mut s = MapStore::default();
        s.set("d", Layer::Surface, 5, 5, Cell { rgb: [200, 0, 0], y: 64, flags: Cell::EXPLORED, ..Default::default() });
        let key = ImageKey::covering("d", Layer::Surface, MapType::Day, 0.0, 0.0, 40.0, 40.0);
        let img = build_image(&mut s, &key);
        let t = key.texel(5.0, 5.0);
        let i = (t[1] as usize * key.w as usize + t[0] as usize) * 4;
        assert_eq!(img[i + 3], 0xFF);
        assert!(img[i] > 150);
    }

    #[test]
    fn small_moves_reuse_the_image() {
        let a = ImageKey::covering("d", Layer::Surface, MapType::Day, 0.0, 0.0, 60.0, 60.0);
        let b = ImageKey::covering("d", Layer::Surface, MapType::Day, 3.0, -2.0, 60.0, 60.0);
        let need = ImageKey { w: 120, h: 120, x0: -57, z0: -62, ..b.clone() };
        assert!(a.covers(&need));
    }

    #[test]
    fn minimap_draws() {
        let cfg = Config::default();
        let sc = Scene { x: 0.0, y: 64.0, z: 0.0, yaw: 0.0, dim: "d", biome: "Plaines", waypoints: &[], entities: &[], extra: &[] };
        let key = ImageKey::covering("d", Layer::Surface, MapType::Day, 0.0, 0.0, 100.0, 100.0);
        let mut c = Canvas::default();
        draw_minimap(&mut c, &Approx, &cfg, &sc, Some(&key), 640.0, 360.0);
        assert!(c.ops.iter().any(|o| matches!(o, crate::draw::Op::Image { .. })));
    }
}
