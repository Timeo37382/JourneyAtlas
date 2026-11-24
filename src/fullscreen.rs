//! The fullscreen map (J): pan by dragging, zoom with the wheel, floating toolbars for the map
//! type, the filters and the tools, a waypoint manager, the options and a help panel.

use crate::config::{Config, Corner, Shape, ZOOMS};
use crate::draw::{Canvas, Measure, Rect, tex};
use crate::geometry::View;
use crate::render::{ImageKey, Scene, draw_entity, image_polygon, marker_angle};
use crate::shading::MapType;
use crate::i18n::{t, tf};
use crate::waypoints::{Kind, Waypoint};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    None,
    Waypoints,
    Options,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    MapType(MapType),
    AutoType,
    Follow,
    Panel(Panel),
    Close,
    Grid,
    ZoomIn,
    ZoomOut,
    WaypointToggle(usize),
    WaypointDelete(usize),
    WaypointRename(usize),
    WaypointCenter(usize),
    WaypointColor(usize),
    WaypointsAllToggle,
    Option(&'static str),
    Slider(&'static str),
    Keys,
    Reset,
    DeleteMap,
    ConfirmDelete(bool),
    SaveImage,
    Mapping,
    Tutorial(crate::tutorial::CardButton),
}

/// What a right-click on the map offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Teleport,
    Create,
    Rename(usize),
    Delete(usize),
}

pub struct ContextMenu {
    pub x: f32,
    pub y: f32,
    /// The block clicked.
    pub wx: i32,
    pub wz: i32,
    /// The waypoint clicked, if any.
    pub target: Option<usize>,
    pub items: Vec<(Rect, MenuItem)>,
}

pub struct Fullscreen {
    pub wx: f64,
    pub wz: f64,
    /// Pixels per block.
    pub scale: f32,
    pub follow: bool,
    pub panel: Panel,
    /// Waypoint being renamed and the text typed so far.
    pub rename: Option<(usize, String)>,
    pub buttons: Vec<(Rect, Action)>,
    pub scroll: usize,
    pub dragging: bool,
    pub menu: Option<ContextMenu>,
    /// The slider being dragged and its track.
    pub slider: Option<(&'static str, Rect)>,
    /// The keyboard shortcut legend (bottom right).
    pub show_keys: bool,
    /// The tooltip of the button under the cursor: key, and where.
    pub hover_tip: Option<(&'static str, f32, f32)>,
    /// Asking to confirm the map's deletion.
    pub confirm_delete: bool,
}

impl Default for Fullscreen {
    fn default() -> Self {
        Self { wx: 0.0, wz: 0.0, scale: 2.0, follow: true, panel: Panel::None, rename: None, buttons: Vec::new(), scroll: 0, dragging: false, menu: None, slider: None, show_keys: false, hover_tip: None, confirm_delete: false }
    }
}

pub const MIN_SCALE: f32 = 0.0625;
pub const MAX_SCALE: f32 = 16.0;

impl Fullscreen {
    pub fn view(&self, gui_w: f32, gui_h: f32) -> View {
        View { cx: gui_w / 2.0, cy: gui_h / 2.0, wx: self.wx, wz: self.wz, scale: self.scale, angle: 0.0 }
    }

    pub fn zoom(&mut self, steps: f64, at: Option<(f32, f32, f32, f32)>) {
        let before = self.scale;
        let f = 2f32.powf(steps as f32 * 0.5);
        self.scale = (self.scale * f).clamp(MIN_SCALE, MAX_SCALE);
        // Keep the block under the cursor in place.
        if let Some((mx, my, w, h)) = at {
            let v = View { scale: before, ..self.view(w, h) };
            let (bx, bz) = v.to_world(mx, my);
            let v2 = View { scale: self.scale, ..v };
            let (ax, az) = v2.to_world(mx, my);
            self.wx += bx - ax;
            self.wz += bz - az;
            if (bx - ax).abs() > 0.01 || (bz - az).abs() > 0.01 {
                self.follow = false;
            }
        }
    }

    pub fn pan_pixels(&mut self, dx: f32, dy: f32) {
        self.wx -= f64::from(dx / self.scale);
        self.wz -= f64::from(dy / self.scale);
        self.follow = false;
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Action> {
        self.buttons.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| a.clone())
    }

    /// Whether a point is over a toolbar or the open panel (not the map).
    pub fn over_ui(&self, x: f32, y: f32, gui_w: f32) -> bool {
        self.confirm_delete
            || self.buttons.iter().any(|(r, _)| r.contains(x, y))
            || (self.panel != Panel::None && x >= panel_x(gui_w))
    }

    /// The waypoint drawn under a screen point, if any.
    pub fn waypoint_at(&self, waypoints: &[Waypoint], dim: &str, x: f32, y: f32, gui_w: f32, gui_h: f32) -> Option<usize> {
        let v = self.view(gui_w, gui_h);
        waypoints
            .iter()
            .enumerate()
            .filter(|(_, w)| w.enabled && w.dim == dim)
            .map(|(i, w)| {
                let (sx, sy) = v.to_screen(f64::from(w.x) + 0.5, f64::from(w.z) + 0.5);
                (i, (sx - x).powi(2) + (sy - y).powi(2))
            })
            .filter(|(_, d)| *d <= 64.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

pub struct FullInput<'a> {
    pub cfg: &'a Config,
    pub scene: &'a Scene<'a>,
    pub key: Option<&'a ImageKey>,
    pub map_type: MapType,
    pub cursor: (f32, f32),
    pub gui_w: f32,
    pub gui_h: f32,
    /// Biome / height under the cursor, if known.
    pub under_cursor: Option<String>,
    /// Whether the player may teleport (operator).
    pub can_teleport: bool,
    /// The line under the top bar: where the player is.
    pub player_info: String,
    /// (key, what it does) for the shortcut legend.
    pub keys: &'a [(String, String)],
    /// A short message shown for a few seconds.
    pub toast: Option<&'a str>,
    /// The tutorial step to show, if one is running.
    pub tutorial: Option<u8>,
}

pub fn draw(fs: &mut Fullscreen, c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    let (w, h) = (inp.gui_w, inp.gui_h);
    let sc = inp.scene;
    if fs.follow {
        fs.wx = sc.x;
        fs.wz = sc.z;
    }
    let view = fs.view(w, h);
    fs.buttons.clear();

    c.fill(0.0, 0.0, w, h, 0xFF0A_0A0A);
    if let Some(key) = inp.key {
        image_polygon(c, &[[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], &view, key, 255);
    }

    // Grid: chunks when zoomed in, regions (512 blocks) further out.
    if inp.cfg.grid {
        let (step, col) = if fs.scale >= 1.0 { (16.0, 0x3000_0000) } else { (512.0, 0x6000_0000) };
        let (x0, z0) = view.to_world(0.0, 0.0);
        let mut gx = (x0 / step).floor() * step;
        loop {
            let (sx, _) = view.to_screen(gx, z0);
            if sx >= w {
                break;
            }
            c.fill(sx, 0.0, 1.0, h, col);
            gx += step;
        }
        let mut gz = (z0 / step).floor() * step;
        loop {
            let (_, sy) = view.to_screen(x0, gz);
            if sy >= h {
                break;
            }
            c.fill(0.0, sy, w, 1.0, col);
            gz += step;
        }
    }

    // Entities.
    for e in sc.entities {
        if !inp.cfg.shows(e.kind) {
            continue;
        }
        let (sx, sy) = view.to_screen(e.x, e.z);
        if sx < 0.0 || sy < 0.0 || sx > w || sy > h {
            continue;
        }
        let size = (fs.scale * 1.5).clamp(6.0, 10.0) * inp.cfg.mob_scale / 0.75;
        draw_entity(c, e, sx, sy, size, inp.cfg.mob_heads);
        if e.kind == 2 || inp.cfg.show_entity_names {
            c.text_centered(m, sx, sy + size / 2.0 + 2.0, &e.name, 0xFFFF_FFFF);
        }
    }

    // Waypoints with labels.
    if inp.cfg.show_waypoints {
        for wp in sc.waypoints.iter().filter(|w| w.enabled && w.dim == sc.dim) {
            let (sx, sy) = view.to_screen(f64::from(wp.x) + 0.5, f64::from(wp.z) + 0.5);
            if sx < -20.0 || sy < -20.0 || sx > w + 20.0 || sy > h + 20.0 {
                continue;
            }
            let t = if wp.kind == Kind::Death { tex::WP_DEATH } else { tex::WP_DIAMOND };
            c.sprite(t, sx - 6.0, sy - 6.0, 12.0, 12.0, 0xFF00_0000 | wp.color);
            let label = wp.name.clone();
            let tw = m.text_width(&label);
            c.fill(sx - tw / 2.0 - 2.0, sy + 6.0, tw + 4.0, 10.0, 0xA000_0000);
            c.text((sx - tw / 2.0).round(), sy + 7.0, label, 0xFF00_0000 | wp.color);
        }
    }

    // Player.
    let (px, py) = view.to_screen(sc.x, sc.z);
    c.sprite_rotated(tex::PLAYER, px, py, 20.0, marker_angle(sc.yaw), 0xFFFF_FFFF);

    toolbar(fs, c, m, inp);

    match fs.panel {
        Panel::Waypoints => waypoint_panel(fs, c, m, inp),
        Panel::Options => options_panel(fs, c, m, inp),
        Panel::Help => help_panel(c, m, inp),
        Panel::None => {}
    }
    if fs.show_keys && fs.panel == Panel::None {
        key_legend(c, m, inp);
    }

    // Where the player is, under the top bar; the block under the cursor, over the bottom bar.
    label(c, m, w / 2.0, TOP + BTN + 2.0 * PAD + 3.0, &inp.player_info);
    let (cx, cz) = view.to_world(inp.cursor.0, inp.cursor.1);
    let mut status = format!("x: {}, z: {}", cx.floor() as i64, cz.floor() as i64);
    if let Some(u) = &inp.under_cursor {
        status.push_str("  ");
        status.push_str(u);
    }
    let zoom = if fs.scale >= 1.0 { format!("x{}", fs.scale) } else { format!("1:{}", (1.0 / fs.scale).round()) };
    status.push_str(&format!("   {zoom}"));
    label(c, m, w / 2.0, h - BTN - 2.0 * PAD - 16.0, &status);
    if let Some(msg) = inp.toast {
        let tw = m.text_width(msg);
        let y = TOP + BTN + 2.0 * PAD + 18.0;
        c.fill(w / 2.0 - tw / 2.0 - 8.0, y, tw + 16.0, 16.0, 0xE02F_6FD8);
        c.text((w / 2.0 - tw / 2.0).round(), y + 4.0, msg, 0xFFFF_FFFF);
    }

    if let Some(step) = inp.tutorial {
        // Over the map, under the top bar, clear of the panel.
        let cx = if fs.panel == Panel::None { w / 2.0 } else { (panel_x(w) - 8.0) / 2.0 + 4.0 };
        let y = TOP + BTN + 2.0 * PAD + 34.0;
        for (r, b) in crate::tutorial::card(c, m, step, cx, y, true, true, inp.cursor) {
            fs.buttons.push((r, Action::Tutorial(b)));
        }
    }
    context_menu(fs, c, m, inp);
    rename_dialog(fs, c, m, inp);
    delete_dialog(fs, c, m, inp);
    tooltip(fs, c, m, inp);
}

/// A small dark label centred on `cx`.
fn label(c: &mut Canvas, m: &dyn Measure, cx: f32, y: f32, text: &str) {
    if text.is_empty() {
        return;
    }
    let tw = m.text_width(text);
    c.fill(cx - tw / 2.0 - 4.0, y, tw + 8.0, 11.0, 0xC010_1626);
    c.text((cx - tw / 2.0).round(), y + 2.0, text, 0xFFE8_ECF4);
}

const BTN: f32 = 16.0;
const GAP: f32 = 3.0;
const PAD: f32 = 3.0;
const TOP: f32 = 4.0;

/// One button of a floating bar: its icon, whether it is on, what it does, its tooltip key.
type Btn = (tex::T, bool, Action, &'static str);

/// Every button, with its tooltip, for hover and the help panel.
fn bar_button(fs: &mut Fullscreen, c: &mut Canvas, x: f32, y: f32, b: &Btn, cursor: (f32, f32)) {
    let (icon, on, action, tip) = b;
    let r = Rect { x, y, w: BTN, h: BTN };
    let hover = r.contains(cursor.0, cursor.1);
    let bg = match (*on, hover) {
        (true, _) => 0xFF4C_6EF5,
        (false, true) => 0xFF3A_4FB0,
        (false, false) => 0xFF24_3170,
    };
    c.fill(x + 1.0, y, BTN - 2.0, BTN, bg);
    c.fill(x, y + 1.0, BTN, BTN - 2.0, bg);
    if *on {
        c.fill(x + 1.0, y + BTN - 1.0, BTN - 2.0, 1.0, 0xFFFF_FFFF);
    }
    let pad = 2.0;
    c.sprite(*icon, x + pad, y + pad, BTN - 2.0 * pad, BTN - 2.0 * pad, if *on || hover { 0xFFFF_FFFF } else { 0xFFC8_D2F0 });
    fs.buttons.push((r, action.clone()));
    if hover {
        fs.hover_tip = Some((tip, x + BTN / 2.0, y));
    }
}

/// A floating bar: a dark rounded plate behind a row (or column) of buttons. Returns its size.
fn bar(fs: &mut Fullscreen, c: &mut Canvas, x: f32, y: f32, vertical: bool, items: &[Btn], cursor: (f32, f32)) -> (f32, f32) {
    let n = items.len() as f32;
    let len = n * BTN + (n - 1.0).max(0.0) * GAP + 2.0 * PAD;
    let thick = BTN + 2.0 * PAD;
    let (w, h) = if vertical { (thick, len) } else { (len, thick) };
    c.fill(x + 1.0, y, w - 2.0, h, 0xD80E_1430);
    c.fill(x, y + 1.0, w, h - 2.0, 0xD80E_1430);
    for (i, b) in items.iter().enumerate() {
        let off = PAD + i as f32 * (BTN + GAP);
        let (bx, by) = if vertical { (x + PAD, y + off) } else { (x + off, y + PAD) };
        bar_button(fs, c, bx, by, b, cursor);
    }
    (w, h)
}

fn bar_len(n: usize) -> f32 {
    n as f32 * BTN + (n as f32 - 1.0).max(0.0) * GAP + 2.0 * PAD
}

/// The reference layout: map types and filters across the top, follow / zoom on the left,
/// close at the top right, the tools along the bottom.
fn toolbar(fs: &mut Fullscreen, c: &mut Canvas, _m: &dyn Measure, inp: &FullInput<'_>) {
    let cur = inp.cursor;
    let (w, h) = (inp.gui_w, inp.gui_h);
    let cfg = inp.cfg;
    let ty = inp.map_type;
    fs.hover_tip = None;
    let types: Vec<Btn> = vec![
        (tex::DAY, ty == MapType::Day, Action::MapType(MapType::Day), "tip_day"),
        (tex::NIGHT, ty == MapType::Night, Action::MapType(MapType::Night), "tip_night"),
        (tex::TOPO, ty == MapType::Topo, Action::MapType(MapType::Topo), "tip_topo"),
        (tex::LAYERS, ty == MapType::Cave, Action::MapType(MapType::Cave), "tip_caves"),
    ];
    let filters: Vec<Btn> = vec![
        (tex::VILLAGERS, cfg.show_villagers, Action::Option("villagers"), "tip_villagers"),
        (tex::MONSTERS, cfg.show_mobs, Action::Option("mobs"), "tip_monsters"),
        (tex::ANIMALS, cfg.show_animals, Action::Option("animals"), "tip_animals"),
        (tex::PETS, cfg.show_pets, Action::Option("pets"), "tip_pets"),
        (tex::PLAYERS, cfg.show_players, Action::Option("players"), "tip_players"),
        (tex::GRID, cfg.grid, Action::Grid, "tip_grid"),
        (tex::KEYS, fs.show_keys, Action::Keys, "tip_keys"),
    ];
    let gap = 6.0;
    let total = bar_len(types.len()) + gap + bar_len(filters.len());
    let x0 = (w - total) / 2.0;
    let (tw, _) = bar(fs, c, x0, TOP, false, &types, cur);
    bar(fs, c, x0 + tw + gap, TOP, false, &filters, cur);

    bar(fs, c, w - BTN - 2.0 * PAD - 4.0, TOP, false, &[(tex::CLOSE, false, Action::Close, "tip_close")], cur);

    let left: Vec<Btn> = vec![
        (tex::FOLLOW, fs.follow, Action::Follow, "tip_follow"),
        (tex::ZOOM_IN, false, Action::ZoomIn, "tip_zoom_in"),
        (tex::ZOOM_OUT, false, Action::ZoomOut, "tip_zoom_out"),
    ];
    bar(fs, c, 4.0, h / 2.0 - bar_len(left.len()) / 2.0, true, &left, cur);

    let tools: Vec<Btn> = vec![
        (tex::WAYPOINTS, fs.panel == Panel::Waypoints, Action::Panel(Panel::Waypoints), "tip_waypoints"),
        (tex::OPTIONS, fs.panel == Panel::Options, Action::Panel(Panel::Options), "tip_options"),
        (tex::ABOUT, fs.panel == Panel::Help, Action::Panel(Panel::Help), "tip_help"),
        (tex::RESET, false, Action::Reset, "tip_reset"),
        (tex::DELETE, fs.confirm_delete, Action::DeleteMap, "tip_delete"),
        (tex::SAVEMAP, false, Action::SaveImage, "tip_save"),
        (tex::DISABLE, !cfg.mapping, Action::Mapping, "tip_mapping"),
    ];
    let bw = bar_len(tools.len());
    bar(fs, c, (w - bw) / 2.0, h - BTN - 2.0 * PAD - 4.0, false, &tools, cur);
}

fn tooltip(fs: &Fullscreen, c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    let Some((key, x, y)) = fs.hover_tip else { return };
    let text = t(key);
    let tw = m.text_width(&text);
    let tx = (x - tw / 2.0 - 4.0).clamp(2.0, inp.gui_w - tw - 10.0);
    // Below the button when it is in the top half, above it otherwise.
    let ty = if y < inp.gui_h / 2.0 { y + BTN + 4.0 } else { y - 15.0 };
    c.fill(tx, ty, tw + 8.0, 12.0, 0xF00A_0E1E);
    c.outline(tx, ty, tw + 8.0, 12.0, 1.0, 0xFF4C_6EF5);
    c.text(tx + 4.0, ty + 2.0, text, 0xFFFF_FFFF);
}

fn key_legend(c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    let rows = inp.keys;
    if rows.is_empty() {
        return;
    }
    let kw = rows.iter().map(|(k, _)| m.text_width(k)).fold(0.0, f32::max);
    let dw = rows.iter().map(|(_, d)| m.text_width(d)).fold(0.0, f32::max);
    let (bw, bh) = (kw + dw + 18.0, rows.len() as f32 * 10.0 + 18.0);
    let (x, y) = (inp.gui_w - bw - 6.0, inp.gui_h - bh - BTN - 2.0 * PAD - 10.0);
    c.fill(x, y, bw, bh, 0xD80E_1430);
    c.text(x + 6.0, y + 4.0, t("keys_title"), 0xFF8F_B4FF);
    for (i, (k, d)) in rows.iter().enumerate() {
        let ry = y + 15.0 + i as f32 * 10.0;
        c.text(x + 6.0 + kw - m.text_width(k), ry, k.as_str(), 0xFFFF_E080);
        c.text(x + 12.0 + kw, ry, d.as_str(), 0xFFE8_ECF4);
    }
}

fn help_panel(c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    let (x, mut y, ph) = panel_frame(c, inp, &t("help_title"), m);
    let bottom = y + ph;
    let rows: [(tex::T, &str); 22] = [
        (tex::DAY, "tip_day"), (tex::NIGHT, "tip_night"), (tex::TOPO, "tip_topo"), (tex::LAYERS, "tip_caves"),
        (tex::VILLAGERS, "tip_villagers"), (tex::MONSTERS, "tip_monsters"), (tex::ANIMALS, "tip_animals"),
        (tex::PETS, "tip_pets"), (tex::PLAYERS, "tip_players"), (tex::GRID, "tip_grid"), (tex::KEYS, "tip_keys"),
        (tex::FOLLOW, "tip_follow"), (tex::ZOOM_IN, "tip_zoom_in"), (tex::ZOOM_OUT, "tip_zoom_out"),
        (tex::CLOSE, "tip_close"), (tex::WAYPOINTS, "tip_waypoints"), (tex::OPTIONS, "tip_options"),
        (tex::ABOUT, "tip_help"), (tex::RESET, "tip_reset"), (tex::DELETE, "tip_delete"), (tex::SAVEMAP, "tip_save"),
        (tex::DISABLE, "tip_mapping"),
    ];
    for (icon, key) in rows {
        if y + 12.0 > bottom {
            break;
        }
        c.fill(x + 6.0, y + 1.0, 10.0, 10.0, 0xFF24_3170);
        c.sprite(icon, x + 7.0, y + 2.0, 8.0, 8.0, 0xFFFF_FFFF);
        let mut text = t(key);
        while m.text_width(&text) > PANEL_W - 30.0 && text.chars().count() > 4 {
            text.pop();
        }
        c.text(x + 20.0, y + 2.0, text, 0xFFE8_ECF4);
        y += 12.0;
    }
}

fn delete_dialog(fs: &mut Fullscreen, c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    if !fs.confirm_delete {
        return;
    }
    let msg = t("confirm_delete");
    let w = (m.text_width(&msg) + 20.0).min(inp.gui_w - 20.0);
    let (x, y) = (inp.gui_w / 2.0 - w / 2.0, inp.gui_h / 2.0 - 25.0);
    c.fill(0.0, 0.0, inp.gui_w, inp.gui_h, 0x8000_0000);
    c.fill(x, y, w, 50.0, 0xF014_1A2E);
    c.fill(x, y, w, 1.0, 0xFFE0_4040);
    c.text_centered(m, x + w / 2.0, y + 8.0, &msg, 0xFFFF_FFFF);
    let (mx, my) = inp.cursor;
    for (i, (key, yes)) in [("yes", true), ("no", false)].into_iter().enumerate() {
        let r = Rect { x: x + w / 2.0 - 62.0 + i as f32 * 66.0, y: y + 28.0, w: 58.0, h: 14.0 };
        let hover = r.contains(mx, my);
        let bg = match (yes, hover) {
            (true, true) => 0xFFE0_4040,
            (true, false) => 0xFFB0_3030,
            (false, true) => 0xFF3A_4FB0,
            (false, false) => 0xFF24_3170,
        };
        c.fill(r.x, r.y, r.w, r.h, bg);
        c.text_centered(m, r.x + r.w / 2.0, r.y + 3.0, &t(key), 0xFFFF_FFFF);
        fs.buttons.push((r, Action::ConfirmDelete(yes)));
    }
}

const PANEL_W: f32 = 230.0;

/// The panel's left edge: along the right side, under the close button.
pub fn panel_x(gui_w: f32) -> f32 {
    gui_w - PANEL_W - 4.0
}

fn panel_frame(c: &mut Canvas, inp: &FullInput<'_>, title: &str, _m: &dyn Measure) -> (f32, f32, f32) {
    let x = panel_x(inp.gui_w);
    let y = TOP + BTN + 2.0 * PAD + 18.0;
    let h = inp.gui_h - y - BTN - 2.0 * PAD - 26.0;
    c.fill(x, y, PANEL_W, h, 0xE00E_1430);
    c.fill(x, y + 14.0, PANEL_W, 1.0, 0xFF2F_6FD8);
    c.text(x + 6.0, y + 4.0, title, 0xFFFF_FFFF);
    (x, y + 16.0, h - 16.0)
}

fn waypoint_panel(fs: &mut Fullscreen, c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    let (x, mut y, h) = panel_frame(c, inp, &t("waypoints"), m);
    let (mx, my) = inp.cursor;
    let all_on = inp.scene.waypoints.iter().all(|w| w.enabled);
    let r = c.button(m, x + 4.0, y, &if all_on { t("hide_all") } else { t("show_all") }, false, false);
    fs.buttons.push((r, Action::WaypointsAllToggle));
    y += 18.0;
    let row_h = 14.0;
    let rows = ((h - 22.0) / row_h).max(1.0) as usize;
    let list: Vec<(usize, &Waypoint)> = inp.scene.waypoints.iter().enumerate().collect();
    fs.scroll = fs.scroll.min(list.len().saturating_sub(rows));
    if list.is_empty() {
        c.text(x + 6.0, y + 2.0, t("no_waypoints"), 0xFFA0_A0A0);
    }
    for (i, wp) in list.iter().skip(fs.scroll).take(rows) {
        let i = *i;
        let hover = my >= y && my < y + row_h && mx >= x && mx < x + PANEL_W;
        if hover {
            c.fill(x + 1.0, y, PANEL_W - 2.0, row_h, 0x4040_4040);
        }
        // Colour swatch (click: next colour).
        c.fill(x + 4.0, y + 3.0, 8.0, 8.0, 0xFF00_0000 | wp.color);
        fs.buttons.push((Rect { x: x + 3.0, y, w: 10.0, h: row_h }, Action::WaypointColor(i)));
        // Name (click: centre the map; double action via rename button).
        let dist = {
            let dx = f64::from(wp.x) - inp.scene.x;
            let dz = f64::from(wp.z) - inp.scene.z;
            (dx * dx + dz * dz).sqrt()
        };
        let name = match &fs.rename {
            Some((ri, text)) if *ri == i => format!("{text}_"),
            _ => wp.name.clone(),
        };
        let col = if wp.enabled { 0xFFFF_FFFF } else { 0xFF80_8080 };
        let mut label = name;
        while m.text_width(&label) > 110.0 && label.chars().count() > 3 {
            label.pop();
        }
        c.text(x + 16.0, y + 3.0, label, col);
        fs.buttons.push((Rect { x: x + 14.0, y, w: 112.0, h: row_h }, Action::WaypointCenter(i)));
        let d = if wp.dim == inp.scene.dim { format!("{}m", dist.round() as i64) } else { t("other_dim") };
        c.text(x + 128.0, y + 3.0, d, 0xFFB0_B0B0);
        // Small buttons: rename, show/hide, delete.
        let bx = x + PANEL_W - 54.0;
        for (k, (lbl, action)) in [("R", Action::WaypointRename(i)), (if wp.enabled { "O" } else { "-" }, Action::WaypointToggle(i)), ("X", Action::WaypointDelete(i))]
            .into_iter()
            .enumerate()
        {
            let r = Rect { x: bx + k as f32 * 17.0, y: y + 1.0, w: 15.0, h: 12.0 };
            let hov = r.contains(mx, my);
            c.fill(r.x, r.y, r.w, r.h, if hov { 0xFF50_5050 } else { 0xFF30_3030 });
            c.text_centered(m, r.x + r.w / 2.0, r.y + 2.0, lbl, if lbl == "X" { 0xFFFF_6060 } else { 0xFFFF_FFFF });
            fs.buttons.push((r, action));
        }
        y += row_h;
    }
    if fs.rename.is_some() {
        c.text(x + 6.0, inp.gui_h - 44.0, t("enter_cancel"), 0xFFFF_FF80);
    }
}

enum Row {
    Header(String),
    Toggle(String, bool, &'static str),
    Cycle(String, String, &'static str),
    /// Label, value shown, position 0..1, id.
    Slider(String, String, f32, &'static str),
}

impl Row {
    fn height(&self) -> f32 {
        match self {
            Row::Header(_) => 16.0,
            Row::Slider(..) => 24.0,
            _ => 14.0,
        }
    }
}

const ACCENT: u32 = 0xFF2F_6FD8;

fn options_panel(fs: &mut Fullscreen, c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    let (x, mut y, ph) = panel_frame(c, inp, &t("options"), m);
    let bottom = y + ph - 4.0;
    let cfg = inp.cfg;
    let p = cfg.preset();
    let shape = t(if p.shape == Shape::Circle { "shape_round" } else { "shape_square" });
    let pct = |v: f32| format!("{}%", (v * 100.0).round() as i32);
    let rows = vec![
        Row::Header(t("sec_minimap")),
        Row::Toggle(t("opt_show_minimap"), cfg.minimap, "minimap"),
        Row::Cycle(t("opt_preset"), format!("{}", cfg.preset + 1), "preset"),
        Row::Cycle(t("opt_shape"), shape, "shape"),
        Row::Cycle(t("opt_position"), corner_label(p.corner), "corner"),
        Row::Slider(t("opt_size"), format!("{}", p.size as i32), (p.size - 64.0) / 192.0, "size"),
        Row::Slider(t("opt_zoom"), format!("x{}", ZOOMS[p.zoom]), p.zoom as f32 / (ZOOMS.len() - 1) as f32, "zoom"),
        Row::Slider(t("opt_opacity"), pct(cfg.map_opacity), (cfg.map_opacity - 0.2) / 0.8, "opacity"),
        Row::Toggle(t("opt_rotate"), p.rotate, "rotate"),
        Row::Toggle(t("opt_compass"), p.show_compass, "compass"),
        Row::Header(t("sec_info")),
        Row::Toggle(t("opt_coords"), p.show_coords, "coords"),
        Row::Toggle(t("opt_biome"), p.show_biome, "biome"),
        Row::Toggle(t("opt_time"), cfg.show_time, "time"),
        Row::Toggle(t("opt_weather"), cfg.show_weather, "weather"),
        Row::Header(t("sec_map")),
        Row::Toggle(t("opt_auto_type"), cfg.auto_map_type, "auto"),
        Row::Slider(t("opt_radius"), tf("radius_value", &[&cfg.scan_radius]), (cfg.scan_radius - 2) as f32 / 14.0, "radius"),
        Row::Header(t("sec_waypoints")),
        Row::Toggle(t("opt_wp_map"), cfg.show_waypoints, "waypoints"),
        Row::Toggle(t("opt_wp_world"), cfg.show_world_waypoints, "world_waypoints"),
        Row::Toggle(t("opt_wp_death"), cfg.create_death_waypoints, "death"),
        Row::Toggle(t("opt_wp_death_remove"), cfg.auto_remove_death, "death_remove"),
        Row::Header(t("sec_entities")),
        Row::Toggle(t("opt_villagers"), cfg.show_villagers, "villagers"),
        Row::Toggle(t("opt_mobs"), cfg.show_mobs, "mobs"),
        Row::Toggle(t("opt_animals"), cfg.show_animals, "animals"),
        Row::Toggle(t("opt_pets"), cfg.show_pets, "pets"),
        Row::Toggle(t("opt_heads"), cfg.mob_heads, "heads"),
        Row::Slider(t("opt_mob_scale"), pct(cfg.mob_scale), (cfg.mob_scale - 0.5) / 1.0, "mob_scale"),
        Row::Toggle(t("opt_players"), cfg.show_players, "players"),
        Row::Toggle(t("opt_names"), cfg.show_entity_names, "names"),
        Row::Header(t("tip_help")),
        Row::Cycle(t("opt_tutorial"), "1/6".to_owned(), "tutorial"),
    ];
    // Scroll by rows: keep as many as fit.
    let total: f32 = rows.iter().map(Row::height).sum();
    let room = bottom - y;
    let mut max_skip = 0;
    let mut acc = total;
    while acc > room && max_skip < rows.len() {
        acc -= rows[max_skip].height();
        max_skip += 1;
    }
    fs.scroll = fs.scroll.min(max_skip);
    let (mx, my) = inp.cursor;
    let (left, right) = (x + 8.0, x + PANEL_W - 8.0);
    for row in rows.into_iter().skip(fs.scroll) {
        let h = row.height();
        if y + h > bottom {
            break;
        }
        let r = Rect { x: x + 2.0, y, w: PANEL_W - 4.0, h };
        let hover = r.contains(mx, my);
        match row {
            Row::Header(title) => {
                let tw = m.text_width(&title);
                c.text(left, y + 5.0, title, 0xFF8F_B4FF);
                c.fill(left + tw + 4.0, y + 9.0, right - left - tw - 4.0, 1.0, 0x402F_6FD8);
            }
            Row::Toggle(label, on, id) => {
                if hover {
                    c.fill(r.x, r.y, r.w, r.h, 0x3040_4650);
                }
                c.text(left, y + 3.0, label, 0xFFE8_EAEE);
                // A switch: a pill with a knob, blue when on.
                let (sw, sh) = (18.0, 9.0);
                let (sx, sy) = (right - sw, y + 2.5);
                let track = if on { ACCENT } else { 0xFF3A_3E46 };
                c.fill(sx + 1.0, sy, sw - 2.0, sh, track);
                c.fill(sx, sy + 1.0, sw, sh - 2.0, track);
                let kx = if on { sx + sw - sh + 1.0 } else { sx + 1.0 };
                c.fill(kx, sy + 1.0, sh - 2.0, sh - 2.0, 0xFFFF_FFFF);
                fs.buttons.push((r, Action::Option(id)));
            }
            Row::Cycle(label, value, id) => {
                if hover {
                    c.fill(r.x, r.y, r.w, r.h, 0x3040_4650);
                }
                c.text(left, y + 3.0, label, 0xFFE8_EAEE);
                let v = format!("< {value} >");
                c.text(right - m.text_width(&v), y + 3.0, v, 0xFF8F_B4FF);
                fs.buttons.push((r, Action::Option(id)));
            }
            Row::Slider(label, value, t, id) => {
                c.text(left, y + 2.0, label, 0xFFE8_EAEE);
                let vw = m.text_width(&value);
                c.text(right - vw, y + 2.0, value, 0xFF8F_B4FF);
                let track = Rect { x: left, y: y + 14.0, w: right - left, h: 6.0 };
                let t = t.clamp(0.0, 1.0);
                c.fill(track.x, track.y + 2.0, track.w, 2.0, 0xFF3A_3E46);
                c.fill(track.x, track.y + 2.0, track.w * t, 2.0, ACCENT);
                let kx = track.x + track.w * t;
                let knob = if hover || fs.slider.is_some_and(|(s, _)| s == id) { 0xFFFF_FFFF } else { 0xFFDA_DEE6 };
                c.fill(kx - 2.5, track.y - 0.5, 5.0, 7.0, knob);
                // The whole track (a little taller) takes clicks and drags.
                let hit = Rect { x: track.x - 3.0, y: y + 10.0, w: track.w + 6.0, h: 13.0 };
                fs.buttons.push((hit, Action::Slider(id)));
            }
        }
        y += h;
    }
}

/// Sets a slider from a position 0..1 along its track.
pub fn set_slider(cfg: &mut Config, id: &str, t: f32) {
    let t = t.clamp(0.0, 1.0);
    match id {
        "size" => cfg.preset_mut().size = (64.0 + t * 192.0).round(),
        "zoom" => cfg.preset_mut().zoom = (t * (ZOOMS.len() - 1) as f32).round() as usize,
        "radius" => cfg.scan_radius = 2 + (t * 14.0).round() as i32,
        "opacity" => cfg.map_opacity = ((0.2 + t * 0.8) * 20.0).round() / 20.0,
        "mob_scale" => cfg.mob_scale = ((0.5 + t) * 20.0).round() / 20.0,
        _ => {}
    }
}

fn context_menu(fs: &mut Fullscreen, c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    let Some(menu) = &mut fs.menu else { return };
    let mut labels: Vec<(String, MenuItem)> = Vec::new();
    if let Some(i) = menu.target {
        labels.push((t("menu_rename"), MenuItem::Rename(i)));
        labels.push((t("menu_delete"), MenuItem::Delete(i)));
    } else {
        labels.push((t("menu_create"), MenuItem::Create));
    }
    if inp.can_teleport {
        labels.push((t("menu_tp"), MenuItem::Teleport));
    }
    let title = match menu.target.and_then(|i| inp.scene.waypoints.get(i)) {
        Some(wp) => wp.name.clone(),
        None => format!("{}, {}", menu.wx, menu.wz),
    };
    let w = labels.iter().map(|(l, _)| m.text_width(l)).fold(m.text_width(&title), f32::max) + 14.0;
    let h = labels.len() as f32 * 14.0 + 16.0;
    let x = menu.x.min(inp.gui_w - w - 2.0);
    let y = menu.y.min(inp.gui_h - h - 2.0);
    c.fill(x, y, w, h, 0xF014_161B);
    c.fill(x, y + 13.0, w, 1.0, 0xFF2F_6FD8);
    c.text(x + 5.0, y + 3.0, title, 0xFFA0_C0FF);
    menu.items.clear();
    let (mx, my) = inp.cursor;
    for (k, (label, item)) in labels.into_iter().enumerate() {
        let r = Rect { x, y: y + 15.0 + k as f32 * 14.0, w, h: 14.0 };
        if r.contains(mx, my) {
            c.fill(r.x, r.y, r.w, r.h, 0xE02F_6FD8);
        }
        let col = if matches!(item, MenuItem::Delete(_)) { 0xFFFF_7070 } else { 0xFFFF_FFFF };
        c.text(r.x + 7.0, r.y + 3.0, label, col);
        menu.items.push((r, item));
    }
}

fn rename_dialog(fs: &mut Fullscreen, c: &mut Canvas, m: &dyn Measure, inp: &FullInput<'_>) {
    // The waypoint panel renames in its own list.
    if fs.panel == Panel::Waypoints {
        return;
    }
    let Some((_, text)) = &fs.rename else { return };
    let w = 200.0_f32.min(inp.gui_w - 20.0);
    let (x, y) = (inp.gui_w / 2.0 - w / 2.0, inp.gui_h / 2.0 - 25.0);
    c.fill(x, y, w, 50.0, 0xF014_161B);
    c.fill(x, y + 13.0, w, 1.0, 0xFF2F_6FD8);
    c.text(x + 5.0, y + 3.0, t("wp_name"), 0xFFFF_FFFF);
    c.fill(x + 5.0, y + 18.0, w - 10.0, 13.0, 0xFF0A_0B0E);
    c.text(x + 8.0, y + 21.0, format!("{text}_"), 0xFFFF_FFFF);
    c.text_centered(m, x + w / 2.0, y + 37.0, &t("enter_cancel"), 0xFFA0_A0A0);
}

fn corner_label(c: Corner) -> String {
    t(match c {
        Corner::TopRight => "pos_tr",
        Corner::TopLeft => "pos_tl",
        Corner::BottomRight => "pos_br",
        Corner::BottomLeft => "pos_bl",
        Corner::TopCenter => "pos_tc",
    })
}

/// Applies an option row click to the config.
pub fn apply_option(cfg: &mut Config, id: &str) {
    match id {
        "minimap" => cfg.minimap = !cfg.minimap,
        "preset" => cfg.preset = 1 - cfg.preset.min(1),
        "auto" => cfg.auto_map_type = !cfg.auto_map_type,
        "waypoints" => cfg.show_waypoints = !cfg.show_waypoints,
        "world_waypoints" => cfg.show_world_waypoints = !cfg.show_world_waypoints,
        "death" => cfg.create_death_waypoints = !cfg.create_death_waypoints,
        "mobs" => cfg.show_mobs = !cfg.show_mobs,
        "players" => cfg.show_players = !cfg.show_players,
        "villagers" => cfg.show_villagers = !cfg.show_villagers,
        "animals" => cfg.show_animals = !cfg.show_animals,
        "pets" => cfg.show_pets = !cfg.show_pets,
        "names" => cfg.show_entity_names = !cfg.show_entity_names,
        "heads" => cfg.mob_heads = !cfg.mob_heads,
        "time" => cfg.show_time = !cfg.show_time,
        "weather" => cfg.show_weather = !cfg.show_weather,
        "death_remove" => cfg.auto_remove_death = !cfg.auto_remove_death,
        "radius" => cfg.scan_radius = if cfg.scan_radius >= 16 { 4 } else { cfg.scan_radius + 2 },
        _ => {
            let p = cfg.preset_mut();
            match id {
                "shape" => p.shape = if p.shape == Shape::Circle { Shape::Square } else { Shape::Circle },
                "corner" => {
                    p.corner = match p.corner {
                        Corner::TopRight => Corner::BottomRight,
                        Corner::BottomRight => Corner::BottomLeft,
                        Corner::BottomLeft => Corner::TopLeft,
                        Corner::TopLeft => Corner::TopCenter,
                        Corner::TopCenter => Corner::TopRight,
                    }
                }
                "size" => p.size = if p.size >= 200.0 { 64.0 } else { p.size + 24.0 },
                "zoom" => p.zoom = (p.zoom + 1) % ZOOMS.len(),
                "rotate" => p.rotate = !p.rotate,
                "coords" => p.show_coords = !p.show_coords,
                "biome" => p.show_biome = !p.show_biome,
                "compass" => p.show_compass = !p.show_compass,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::Approx;

    #[test]
    fn draws_and_hits_buttons() {
        let cfg = Config::default();
        let wps = vec![Waypoint { name: "Maison".into(), x: 10, y: 64, z: 10, dim: "d".into(), color: 0xFF0000, enabled: true, kind: Kind::Normal }];
        let sc = Scene { x: 0.0, y: 64.0, z: 0.0, yaw: 0.0, dim: "d", biome: "", waypoints: &wps, entities: &[], extra: &[] };
        let mut fs = Fullscreen { panel: Panel::Waypoints, ..Default::default() };
        let inp = FullInput { cfg: &cfg, scene: &sc, key: None, map_type: MapType::Day, cursor: (0.0, 0.0), gui_w: 640.0, gui_h: 360.0, under_cursor: None, can_teleport: false, player_info: String::new(), keys: &[], toast: None, tutorial: None };
        let mut c = Canvas::default();
        draw(&mut fs, &mut c, &Approx, &inp);
        let (r, _) = fs.buttons.iter().find(|(_, a)| *a == Action::MapType(MapType::Day)).cloned().unwrap();
        assert!(r.y < 30.0 && (r.x - 320.0).abs() < 120.0, "day button at the top centre: {r:?}");
        assert!(fs.buttons.iter().any(|(_, a)| *a == Action::SaveImage));
        assert!(fs.buttons.iter().any(|(_, a)| *a == Action::WaypointDelete(0)));
    }

    #[test]
    fn zoom_keeps_cursor_block() {
        let mut fs = Fullscreen { follow: false, ..Default::default() };
        let v = fs.view(400.0, 300.0);
        let before = v.to_world(300.0, 100.0);
        fs.zoom(2.0, Some((300.0, 100.0, 400.0, 300.0)));
        let after = fs.view(400.0, 300.0).to_world(300.0, 100.0);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
    }
}
