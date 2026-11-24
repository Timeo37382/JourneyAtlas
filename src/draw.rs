//! What the mod draws, as a list of simple operations the game-side bridge replays on the HUD
//! batch. Keeping the drawing here keeps it testable without the game.

/// One drawing operation, in GUI pixels. Colours are `0xAARRGGBB`.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Fill { x: f32, y: f32, w: f32, h: f32, argb: u32 },
    /// A solid convex quad, corners in order.
    Quad { p: [[f32; 2]; 4], argb: u32 },
    /// A quad of the map image: `t` are texel positions in the uploaded image.
    Image { p: [[f32; 2]; 4], t: [[f32; 2]; 4], alpha: u8 },
    Text { x: f32, y: f32, text: String, argb: u32, shadow: bool },
    /// Part `src` (texture pixels) of a texture shipped in `assets/journeyatlas/textures/gui/`,
    /// stretched over `dst`, multiplied by `tint`.
    Sprite { name: &'static str, src: [f32; 4], dst: [f32; 4], tint: u32 },
    /// Part `uv` (0..1 of the texture: u0, v0, u1, v1) of an atlas sprite named at run time,
    /// such as a mob's texture, stretched over `dst`.
    Head { sprite: String, uv: [f32; 4], dst: [f32; 4] },
    /// The same texture on an arbitrary (rotated) quad: `uv` in 0..1 of the texture.
    SpriteQuad { name: &'static str, p: [[f32; 2]; 4], uv: [[f32; 2]; 4], tint: u32 },
}

/// JourneyAtlas's own textures (`assets/journeyatlas/textures/gui/`), with their pixel sizes.
pub mod tex {
    pub type T = (&'static str, f32, f32);

    const fn icon(name: &'static str) -> T {
        (name, 16.0, 16.0)
    }

    pub const SQ_TOP: T = ("journeyatlas:gui/frame/top", 1.0, 8.0);
    pub const SQ_BOTTOM: T = ("journeyatlas:gui/frame/bottom", 1.0, 8.0);
    pub const SQ_LEFT: T = ("journeyatlas:gui/frame/left", 8.0, 1.0);
    pub const SQ_RIGHT: T = ("journeyatlas:gui/frame/right", 8.0, 1.0);
    pub const SQ_TL: T = ("journeyatlas:gui/frame/topleft", 8.0, 8.0);
    pub const SQ_TR: T = ("journeyatlas:gui/frame/topright", 8.0, 8.0);
    pub const SQ_BL: T = ("journeyatlas:gui/frame/bottomleft", 8.0, 8.0);
    pub const SQ_BR: T = ("journeyatlas:gui/frame/bottomright", 8.0, 8.0);
    pub const SQ_COMPASS: T = ("journeyatlas:gui/frame/compass", 16.0, 16.0);
    pub const CI_RIM: T = ("journeyatlas:gui/frame/rim", 256.0, 256.0);
    pub const CI_COMPASS: T = SQ_COMPASS;

    pub const PLAYER: T = ("journeyatlas:gui/markers/player", 32.0, 32.0);
    pub const DOT: T = ("journeyatlas:gui/markers/dot", 16.0, 16.0);
    pub const OFFSCREEN: T = ("journeyatlas:gui/markers/offscreen", 16.0, 16.0);
    pub const WP_DIAMOND: T = ("journeyatlas:gui/markers/waypoint", 16.0, 16.0);
    pub const WP_DEATH: T = ("journeyatlas:gui/markers/death", 16.0, 16.0);

    pub const DAY: T = icon("journeyatlas:gui/icons/day");
    pub const NIGHT: T = icon("journeyatlas:gui/icons/night");
    pub const TOPO: T = icon("journeyatlas:gui/icons/topo");
    pub const LAYERS: T = icon("journeyatlas:gui/icons/layers");
    pub const VILLAGERS: T = icon("journeyatlas:gui/icons/villagers");
    pub const MONSTERS: T = icon("journeyatlas:gui/icons/monsters");
    pub const ANIMALS: T = icon("journeyatlas:gui/icons/animals");
    pub const PETS: T = icon("journeyatlas:gui/icons/pets");
    pub const PLAYERS: T = icon("journeyatlas:gui/icons/players");
    pub const GRID: T = icon("journeyatlas:gui/icons/grid");
    pub const KEYS: T = icon("journeyatlas:gui/icons/keys");
    pub const FOLLOW: T = icon("journeyatlas:gui/icons/follow");
    pub const ZOOM_IN: T = icon("journeyatlas:gui/icons/zoomin");
    pub const ZOOM_OUT: T = icon("journeyatlas:gui/icons/zoomout");
    pub const CLOSE: T = icon("journeyatlas:gui/icons/close");
    pub const WAYPOINTS: T = icon("journeyatlas:gui/icons/waypoints");
    pub const OPTIONS: T = icon("journeyatlas:gui/icons/options");
    pub const ABOUT: T = icon("journeyatlas:gui/icons/about");
    pub const RESET: T = icon("journeyatlas:gui/icons/reset");
    pub const DELETE: T = icon("journeyatlas:gui/icons/delete");
    pub const SAVEMAP: T = icon("journeyatlas:gui/icons/savemap");
    pub const DISABLE: T = icon("journeyatlas:gui/icons/disable");
}

/// Measures text; the bridge answers with the game font.
pub trait Measure {
    fn text_width(&self, text: &str) -> f32;
}

/// A fixed-width guess, for tests.
pub struct Approx;
impl Measure for Approx {
    fn text_width(&self, text: &str) -> f32 {
        text.chars().count() as f32 * 6.0
    }
}

#[derive(Default)]
pub struct Canvas {
    pub ops: Vec<Op>,
}

impl Canvas {
    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        if w > 0.0 && h > 0.0 && argb >> 24 != 0 {
            self.ops.push(Op::Fill { x, y, w, h, argb });
        }
    }

    pub fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, argb: u32) {
        self.fill(x, y, w, t, argb);
        self.fill(x, y + h - t, w, t, argb);
        self.fill(x, y + t, t, h - 2.0 * t, argb);
        self.fill(x + w - t, y + t, t, h - 2.0 * t, argb);
    }

    pub fn quad(&mut self, p: [[f32; 2]; 4], argb: u32) {
        self.ops.push(Op::Quad { p, argb });
    }

    pub fn tri(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 2], argb: u32) {
        self.quad([a, b, c, c], argb);
    }

    pub fn image(&mut self, p: [[f32; 2]; 4], t: [[f32; 2]; 4], alpha: u8) {
        self.ops.push(Op::Image { p, t, alpha });
    }

    pub fn text(&mut self, x: f32, y: f32, text: impl Into<String>, argb: u32) {
        self.ops.push(Op::Text { x, y, text: text.into(), argb, shadow: true });
    }

    pub fn text_centered(&mut self, m: &dyn Measure, cx: f32, y: f32, text: &str, argb: u32) {
        let w = m.text_width(text);
        self.text((cx - w / 2.0).round(), y, text, argb);
    }

    /// A filled disc as a fan of thin quads.
    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, argb: u32) {
        let n = if r > 6.0 { 20 } else { 10 };
        for i in 0..n {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
            self.tri([cx, cy], [cx + r * a0.cos(), cy + r * a0.sin()], [cx + r * a1.cos(), cy + r * a1.sin()], argb);
        }
    }

    /// A ring of thickness `t`.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, t: f32, argb: u32) {
        let n = 64;
        for i in 0..n {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
            let (c0, s0) = (a0.cos(), a0.sin());
            let (c1, s1) = (a1.cos(), a1.sin());
            self.quad(
                [
                    [cx + r * c0, cy + r * s0],
                    [cx + r * c1, cy + r * s1],
                    [cx + (r - t) * c1, cy + (r - t) * s1],
                    [cx + (r - t) * c0, cy + (r - t) * s0],
                ],
                argb,
            );
        }
    }

    /// A player arrow pointing at `angle` (radians, 0 = up, clockwise).
    pub fn arrow(&mut self, cx: f32, cy: f32, angle: f32, size: f32, fill: u32) {
        let (s, c) = angle.sin_cos();
        let p = |dx: f32, dy: f32| [cx + dx * c - dy * s, cy + dx * s + dy * c];
        // Dark outline first, then the coloured arrow.
        let o = size + 1.5;
        self.quad([p(0.0, -o), p(o * 0.75, o), p(0.0, o * 0.45), p(-o * 0.75, o)], 0xFF00_0000);
        self.quad(
            [p(0.0, -size), p(size * 0.7, size * 0.85), p(0.0, size * 0.4), p(-size * 0.7, size * 0.85)],
            fill,
        );
    }

    /// A diamond marker (waypoints).
    pub fn diamond(&mut self, cx: f32, cy: f32, r: f32, argb: u32) {
        self.quad([[cx, cy - r - 1.0], [cx + r + 1.0, cy], [cx, cy + r + 1.0], [cx - r - 1.0, cy]], 0xFF00_0000);
        self.quad([[cx, cy - r], [cx + r, cy], [cx, cy + r], [cx - r, cy]], argb);
    }

    pub fn sprite(&mut self, t: tex::T, x: f32, y: f32, w: f32, h: f32, tint: u32) {
        self.ops.push(Op::Sprite { name: t.0, src: [0.0, 0.0, t.1, t.2], dst: [x, y, w, h], tint });
    }

    /// A texture centred on a point and turned by `angle` (radians, clockwise).
    pub fn sprite_rotated(&mut self, t: tex::T, cx: f32, cy: f32, size: f32, angle: f32, tint: u32) {
        let (s, c) = angle.sin_cos();
        let h = size / 2.0;
        let p = |dx: f32, dy: f32| [cx + dx * c - dy * s, cy + dx * s + dy * c];
        self.ops.push(Op::SpriteQuad {
            name: t.0,
            p: [p(-h, -h), p(h, -h), p(h, h), p(-h, h)],
            uv: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            tint,
        });
    }





    /// A button: returns its rectangle for hit tests.
    pub fn button(&mut self, m: &dyn Measure, x: f32, y: f32, label: &str, active: bool, hover: bool) -> Rect {
        let w = m.text_width(label) + 10.0;
        let h = 14.0;
        let bg = match (active, hover) {
            (true, _) => 0xE02F_6FD8,
            (false, true) => 0xD038_3C44,
            _ => 0xB022_252B,
        };
        self.fill(x, y, w, h, bg);
        self.text(x + 5.0, y + 3.0, label, 0xFFFF_FFFF);
        Rect { x, y, w, h }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}
