//! JourneyAtlas: maps for Horizon, written in Rust: a minimap, a fullscreen map (J), day / night /
//! cave maps saved per world, waypoints ("balises", B, and one at each death) and an entity radar.
//!
//! This crate holds everything that does not touch the game: tiles, shading, layout, drawing
//! (as a list of operations) and the screens' logic. `src/bridge.rs` is compiled inside the
//! client (a `[[native.module]]`), reads the world and player from it, and replays the drawing.

pub mod config;
pub mod draw;
pub mod fullscreen;
pub mod geometry;
pub mod i18n;
pub mod i18n_ui;
pub mod png;
pub mod render;
pub mod shading;
pub mod store;
pub mod tiles;
pub mod tutorial;
pub mod waypoints;

use config::Config;
use fullscreen::{Action, ContextMenu, Fullscreen, MenuItem, Panel};
use mod_api::{
    self, ClientDisconnectingEvent, Flow, ClientHost, Hooks, InputKeyEvent, KeyRegistration, Registrar,
    RegistrationContext, RenderFramePreEvent, RenderGuiPostEvent, ScreenCharTypedPreEvent,
    ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseButtonReleasedPreEvent,
    ScreenMouseDraggedPreEvent, ScreenMouseScrolledPreEvent, ScreenRenderPostEvent,
};
use render::ImageKey;
use shading::MapType;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use store::{MapStore, safe_token};
use waypoints::{Kind, Waypoint};

pub const MOD_ID: &str = "journeyatlas";

/// The key mappings, the usual defaults.
pub const KEYS: &[(&str, &str)] = &[
    ("map_toggle", "key.keyboard.j"),
    ("create_waypoint", "key.keyboard.b"),
    ("zoom_in", "key.keyboard.equal"),
    ("zoom_out", "key.keyboard.minus"),
    ("minimap_type", "key.keyboard.left.bracket"),
    ("minimap_preset", "key.keyboard.backslash"),
    ("minimap_toggle", "key.keyboard.right.bracket"),
];

/// Something only the game side can do (it needs `&mut Game`).
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    OpenMap,
    CloseMap,
    /// `/tp` the player to this block.
    Teleport(i32, i32, i32),
    /// Scan every loaded chunk again.
    Rescan,
    /// Save what the fullscreen map shows as a PNG.
    SaveImage,
}

pub struct State {
    pub cfg: Config,
    pub store: MapStore,
    pub waypoints: Vec<Waypoint>,
    pub fs: Fullscreen,
    /// `<instance>/journeyatlas`.
    pub root: Option<PathBuf>,
    pub world: String,
    /// What the uploaded image shows, and the store revision it was built from.
    pub image: Option<ImageKey>,
    pub image_revision: u64,
    pub image_frame: u64,
    pub requests: Vec<Request>,
    /// The map type shown now (the chosen one, or the automatic one).
    pub shown_type: MapType,
    pub was_dead: bool,
    /// Frame the fullscreen map opened on: the key press that opened it must not close it.
    pub opened_at: u64,
    /// Operator rights, from the game each frame.
    pub can_teleport: bool,
    /// The dimension the player is in, from the game each frame.
    pub current_dim: String,
    /// A message for the fullscreen map, and the frame it was posted on.
    pub toast: Option<(String, u64)>,
    /// The first-visit tutorial of this world.
    pub tutorial: tutorial::Tutorial,
    /// Whether the fullscreen map is open, from the game each frame.
    pub map_open: bool,
    pub frame: u64,
    pub config_dirty: bool,
    pub waypoints_dirty: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            cfg: Config::default(),
            store: MapStore::default(),
            waypoints: Vec::new(),
            fs: Fullscreen::default(),
            root: None,
            world: String::new(),
            image: None,
            image_revision: 0,
            image_frame: 0,
            requests: Vec::new(),
            shown_type: MapType::Day,
            was_dead: false,
            opened_at: 0,
            can_teleport: false,
            current_dim: String::new(),
            toast: None,
            tutorial: tutorial::Tutorial::default(),
            map_open: false,
            frame: 0,
            config_dirty: false,
            waypoints_dirty: false,
        }
    }
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

pub fn state() -> MutexGuard<'static, State> {
    STATE
        .get_or_init(|| Mutex::new(State::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

impl State {
    /// The first frame with a game folder: reads the options.
    pub fn set_root(&mut self, root: PathBuf) {
        if self.root.as_ref() == Some(&root) {
            return;
        }
        if let Ok(text) = std::fs::read_to_string(root.join("config.txt")) {
            self.cfg = Config::decode(&text);
        }
        self.shown_type = self.cfg.map_type;
        self.root = Some(root);
    }

    fn world_dir(&self) -> Option<PathBuf> {
        Some(self.root.as_ref()?.join("worlds").join(safe_token(&self.world)))
    }

    /// Switches to another world (or none, with an empty key): saves the old one, loads the new.
    pub fn enter_world(&mut self, world: &str) {
        if self.world == world {
            return;
        }
        self.save_all();
        self.world = world.to_owned();
        self.image = None;
        self.fs = Fullscreen::default();
        if world.is_empty() {
            self.store.close();
            self.waypoints.clear();
            self.tutorial = tutorial::Tutorial::default();
            return;
        }
        let Some(dir) = self.world_dir() else { return };
        self.store.open(dir.join("map"));
        let saved = std::fs::read_to_string(dir.join("tutorial.txt")).ok();
        self.tutorial = tutorial::Tutorial::load(saved.as_deref());
        self.tutorial.since = self.frame;
        self.waypoints = std::fs::read_to_string(dir.join("waypoints.txt"))
            .map(|t| waypoints::decode(&t))
            .unwrap_or_default();
    }

    pub fn save_all(&mut self) {
        self.store.flush();
        if let Some(root) = &self.root
            && self.config_dirty
        {
            let _ = std::fs::create_dir_all(root);
            if std::fs::write(root.join("config.txt"), self.cfg.encode()).is_ok() {
                self.config_dirty = false;
            }
        }
        if self.tutorial.dirty
            && !self.world.is_empty()
            && let Some(dir) = self.world_dir()
        {
            let _ = std::fs::create_dir_all(&dir);
            if std::fs::write(dir.join("tutorial.txt"), self.tutorial.save()).is_ok() {
                self.tutorial.dirty = false;
            }
        }
        if self.waypoints_dirty
            && !self.world.is_empty()
            && let Some(dir) = self.world_dir()
        {
            let _ = std::fs::create_dir_all(&dir);
            if std::fs::write(dir.join("waypoints.txt"), waypoints::encode(&self.waypoints)).is_ok() {
                self.waypoints_dirty = false;
            }
        }
    }

    pub fn add_waypoint(&mut self, name: Option<String>, x: i32, y: i32, z: i32, dim: &str, kind: Kind) {
        if kind == Kind::Death {
            // Only the last death is kept.
            self.waypoints.retain(|w| w.kind != Kind::Death);
        }
        let n = self.waypoints.iter().filter(|w| w.kind == Kind::Normal).count();
        let name = name.unwrap_or_else(|| match kind {
            Kind::Death => i18n::t("wp_death"),
            Kind::Normal => i18n::tf("wp_default", &[&(n + 1)]),
        });
        let color = if kind == Kind::Death { 0xFF0000 } else { waypoints::color_for(n) };
        if kind == Kind::Normal {
            let e = if self.map_open { tutorial::Event::WaypointOnMap } else { tutorial::Event::WaypointInWorld };
            self.tutorial.event(e, self.frame);
        }
        self.waypoints.push(Waypoint { name, x, y, z, dim: dim.to_owned(), color, enabled: true, kind });
        self.waypoints_dirty = true;
    }

    /// A mapped key went down while playing (no screen open).
    pub fn on_key(&mut self, id: &str) -> Option<KeyEffect> {
        match id {
            "map_toggle" => self.requests.push(Request::OpenMap),
            "create_waypoint" => return Some(KeyEffect::CreateWaypoint),
            "zoom_in" | "zoom_out" => {
                let p = self.cfg.preset_mut();
                p.zoom = if id == "zoom_in" {
                    (p.zoom + 1).min(config::ZOOMS.len() - 1)
                } else {
                    p.zoom.saturating_sub(1)
                };
                self.config_dirty = true;
            }
            "minimap_type" => {
                self.cfg.auto_map_type = false;
                self.cfg.map_type = self.cfg.map_type.next();
                self.config_dirty = true;
            }
            "minimap_preset" => {
                self.cfg.preset = 1 - self.cfg.preset.min(1);
                self.config_dirty = true;
            }
            "minimap_toggle" => {
                self.cfg.minimap = !self.cfg.minimap;
                self.config_dirty = true;
            }
            _ => {}
        }
        None
    }

    /// A click on the fullscreen map. `world_at` is the block under the cursor.
    pub fn fs_press(&mut self, x: f32, y: f32, button: i32, gui_w: f32, gui_h: f32, dim: &str, ground_y: i32) {
        if self.fs.rename.is_some() && button == 0 {
            // A click anywhere ends the renaming, keeping what was typed.
            self.finish_rename(true);
        }
        if let Some(menu) = self.fs.menu.take() {
            if button == 0
                && let Some((_, item)) = menu.items.iter().find(|(r, _)| r.contains(x, y))
            {
                match *item {
                    MenuItem::Create => {
                        self.add_waypoint(None, menu.wx, ground_y, menu.wz, dim, Kind::Normal);
                        let i = self.waypoints.len() - 1;
                        self.fs.rename = Some((i, self.waypoints[i].name.clone()));
                    }
                    MenuItem::Teleport => self.requests.push(Request::Teleport(menu.wx, ground_y, menu.wz)),
                    MenuItem::Rename(i) => self.apply(Action::WaypointRename(i), gui_w, gui_h),
                    MenuItem::Delete(i) => self.apply(Action::WaypointDelete(i), gui_w, gui_h),
                }
            }
            // Any click closes the menu.
            return;
        }
        if let Some(action) = self.fs.hit(x, y) {
            if let (Action::Slider(id), 0) = (&action, button) {
                if let Some(r) = self.fs.buttons.iter().rev().find(|(r, a)| *a == action && r.contains(x, y)).map(|(r, _)| *r) {
                    let track = draw::Rect { x: r.x + 3.0, y: r.y, w: r.w - 6.0, h: r.h };
                    self.fs.slider = Some((id, track));
                    self.slider_to(x);
                }
                return;
            }
            if button == 0 {
                self.apply(action, gui_w, gui_h);
            }
            return;
        }
        if self.fs.over_ui(x, y, gui_w) {
            return;
        }
        match button {
            0 => self.fs.dragging = true,
            1 => {
                let (wx, wz) = self.fs.view(gui_w, gui_h).to_world(x, y);
                let target = self.fs.waypoint_at(&self.waypoints, dim, x, y, gui_w, gui_h);
                let (wx, wz) = match target.and_then(|i| self.waypoints.get(i)) {
                    Some(w) => (w.x, w.z),
                    None => (wx.floor() as i32, wz.floor() as i32),
                };
                self.fs.menu = Some(ContextMenu { x, y, wx, wz, target, items: Vec::new() });
            }
            _ => {}
        }
    }

    pub fn apply(&mut self, action: Action, gui_w: f32, gui_h: f32) {
        match action {
            Action::MapType(t) => {
                self.cfg.map_type = t;
                self.cfg.auto_map_type = false;
                self.config_dirty = true;
            }
            Action::AutoType => {
                self.cfg.auto_map_type = !self.cfg.auto_map_type;
                self.config_dirty = true;
            }
            Action::Follow => self.fs.follow = true,
            Action::Grid => {
                self.cfg.grid = !self.cfg.grid;
                self.config_dirty = true;
            }
            Action::ZoomIn => self.fs.zoom(2.0, Some((gui_w / 2.0, gui_h / 2.0, gui_w, gui_h))),
            Action::ZoomOut => self.fs.zoom(-2.0, Some((gui_w / 2.0, gui_h / 2.0, gui_w, gui_h))),
            Action::Panel(p) => self.fs.panel = if self.fs.panel == p { Panel::None } else { p },
            Action::Close => self.requests.push(Request::CloseMap),
            Action::WaypointToggle(i) => {
                if let Some(w) = self.waypoints.get_mut(i) {
                    w.enabled = !w.enabled;
                    self.waypoints_dirty = true;
                }
            }
            Action::WaypointDelete(i) => {
                if i < self.waypoints.len() {
                    self.waypoints.remove(i);
                    self.waypoints_dirty = true;
                    self.fs.rename = None;
                }
            }
            Action::WaypointRename(i) => {
                if let Some(w) = self.waypoints.get(i) {
                    self.fs.rename = Some((i, w.name.clone()));
                }
            }
            Action::WaypointColor(i) => {
                if let Some(w) = self.waypoints.get_mut(i) {
                    let at = waypoints::PALETTE.iter().position(|c| *c == w.color).map_or(0, |p| p + 1);
                    w.color = waypoints::color_for(at);
                    self.waypoints_dirty = true;
                }
            }
            Action::WaypointCenter(i) => {
                if let Some(w) = self.waypoints.get(i) {
                    self.fs.wx = f64::from(w.x);
                    self.fs.wz = f64::from(w.z);
                    self.fs.follow = false;
                }
            }
            Action::WaypointsAllToggle => {
                let on = !self.waypoints.iter().all(|w| w.enabled);
                for w in &mut self.waypoints {
                    w.enabled = on;
                }
                self.waypoints_dirty = true;
            }
            Action::Slider(_) => {}
            Action::Keys => self.fs.show_keys = !self.fs.show_keys,
            Action::Reset => {
                self.requests.push(Request::Rescan);
                self.notify(i18n::t("toast_reset"));
            }
            Action::DeleteMap => self.fs.confirm_delete = true,
            Action::ConfirmDelete(yes) => {
                self.fs.confirm_delete = false;
                if yes {
                    let dim = self.current_dim.clone();
                    self.store.delete_dim(&dim);
                    self.image = None;
                    self.requests.push(Request::Rescan);
                    self.notify(i18n::t("toast_deleted"));
                }
            }
            Action::SaveImage => self.requests.push(Request::SaveImage),
            Action::Mapping => {
                self.cfg.mapping = !self.cfg.mapping;
                self.config_dirty = true;
                self.notify(i18n::t(if self.cfg.mapping { "toast_resumed" } else { "toast_paused" }));
            }
            Action::Tutorial(b) => match b {
                tutorial::CardButton::Next => self.tutorial.next(self.frame),
                tutorial::CardButton::Skip => self.tutorial.skip(self.frame),
            },
            Action::Option("tutorial") => {
                self.tutorial.restart(self.frame);
                // Already on the map: go straight to the map steps.
                self.tutorial.event(tutorial::Event::MapOpened, self.frame);
                self.fs.panel = Panel::None;
            }
            Action::Option(id) => {
                fullscreen::apply_option(&mut self.cfg, id);
                self.config_dirty = true;
            }
        }
    }

    pub fn notify(&mut self, text: String) {
        self.toast = Some((text, self.frame));
    }

    /// The message to show now, for about three seconds after it was posted.
    pub fn toast_now(&self) -> Option<&str> {
        self.toast.as_ref().filter(|(_, at)| self.frame.saturating_sub(*at) < 180).map(|(t, _)| t.as_str())
    }

    /// Moves the dragged slider to the cursor's x.
    pub fn slider_to(&mut self, x: f32) {
        if let Some((id, track)) = self.fs.slider {
            fullscreen::set_slider(&mut self.cfg, id, (x - track.x) / track.w.max(1.0));
            self.config_dirty = true;
        }
    }

    pub fn finish_rename(&mut self, keep: bool) {
        if let Some((i, text)) = self.fs.rename.take()
            && keep
            && let Some(w) = self.waypoints.get_mut(i)
        {
            let t = text.trim();
            if !t.is_empty() {
                w.name = t.to_owned();
                self.waypoints_dirty = true;
            }
        }
    }

    /// A key over the fullscreen map. Returns true when the map took it.
    pub fn fs_key(&mut self, key: &str, map_key: Option<&str>) -> bool {
        if self.fs.confirm_delete && key == "key.keyboard.escape" {
            self.fs.confirm_delete = false;
            return true;
        }
        if self.fs.menu.is_some() && key == "key.keyboard.escape" {
            self.fs.menu = None;
            return true;
        }
        if self.fs.rename.is_some() {
            match key {
                "key.keyboard.enter" | "key.keyboard.keypad.enter" => self.finish_rename(true),
                "key.keyboard.escape" => self.finish_rename(false),
                "key.keyboard.backspace" => {
                    if let Some((_, t)) = &mut self.fs.rename {
                        t.pop();
                    }
                }
                _ => {}
            }
            // Everything else is typing.
            return true;
        }
        let step = 32.0 / f64::from(self.fs.scale.max(0.25));
        match key {
            "key.keyboard.up" | "key.keyboard.w" => self.fs.wz -= step,
            "key.keyboard.down" | "key.keyboard.s" => self.fs.wz += step,
            "key.keyboard.left" | "key.keyboard.a" => self.fs.wx -= step,
            "key.keyboard.right" | "key.keyboard.d" => self.fs.wx += step,
            _ if Some(key) == map_key => {
                if self.frame.saturating_sub(self.opened_at) > 10 {
                    self.requests.push(Request::CloseMap);
                }
                return true;
            }
            _ => return false,
        }
        self.fs.follow = false;
        true
    }

    pub fn fs_char(&mut self, c: char) -> bool {
        if let Some((_, t)) = &mut self.fs.rename {
            if !c.is_control() && t.chars().count() < 32 {
                t.push(c);
            }
            return true;
        }
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEffect {
    CreateWaypoint,
}

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        for (id, key) in KEYS {
            registrar.register_key_mapping(KeyRegistration::new(id, key).label(&format!("key.journeyatlas.{id}")));
        }
    }

    // The game fires its screen, HUD, frame and key hooks only when a mod listens to them.
    // The work happens in the bridge's mixins (src/bridge.rs); these empty listeners turn the
    // hooks on.
    fn screen_render_post(&self, _: &dyn ClientHost, _: &mut ScreenRenderPostEvent<'_>) {}
    fn screen_mouse_button_pressed_pre(&self, _: &dyn ClientHost, _: &mut ScreenMouseButtonPressedPreEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn screen_mouse_button_released_pre(&self, _: &dyn ClientHost, _: &mut ScreenMouseButtonReleasedPreEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn screen_mouse_dragged_pre(&self, _: &dyn ClientHost, _: &mut ScreenMouseDraggedPreEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn screen_mouse_scrolled_pre(&self, _: &dyn ClientHost, _: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn screen_key_pressed_pre(&self, _: &dyn ClientHost, _: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn screen_char_typed_pre(&self, _: &dyn ClientHost, _: &mut ScreenCharTypedPreEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn render_gui_post(&self, _: &dyn ClientHost, _: &mut RenderGuiPostEvent<'_>) {}
    fn render_frame_pre(&self, _: &mut dyn ClientHost, _: &mut RenderFramePreEvent) {}
    fn input_key(&self, _: &mut dyn ClientHost, _: &mut InputKeyEvent<'_>) {}
    fn client_disconnecting(&self, _: &dyn ClientHost, _: &mut ClientDisconnectingEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_flow() {
        let mut s = State::default();
        s.add_waypoint(None, 1, 64, 2, "d", Kind::Normal);
        s.apply(Action::WaypointRename(0), 400.0, 300.0);
        assert!(s.fs_key("key.keyboard.backspace", None));
        for c in "Base".chars() {
            s.fs_char(c);
        }
        s.fs_key("key.keyboard.enter", None);
        assert!(s.waypoints[0].name.ends_with("Base"));
    }

    #[test]
    fn one_death_waypoint() {
        let mut s = State::default();
        s.add_waypoint(None, 1, 64, 2, "d", Kind::Death);
        s.add_waypoint(None, 9, 64, 9, "d", Kind::Death);
        assert_eq!(s.waypoints.len(), 1);
        assert_eq!(s.waypoints[0].name, i18n::t("wp_death"));
        assert_eq!(s.waypoints[0].x, 9);
    }

    #[test]
    fn right_click_menu_creates() {
        let mut s = State::default();
        s.fs.follow = false;
        s.fs_press(100.0, 100.0, 1, 400.0, 300.0, "d", 70);
        assert!(s.fs.menu.as_ref().unwrap().target.is_none());
        // Draw once so the menu lays out its items.
        let sc = crate::render::Scene { x: 0.0, y: 64.0, z: 0.0, yaw: 0.0, dim: "d", biome: "", waypoints: &[], entities: &[], extra: &[] };
        let cfg = s.cfg.clone();
        let inp = fullscreen::FullInput { cfg: &cfg, scene: &sc, key: None, map_type: shading::MapType::Day, cursor: (0.0, 0.0), gui_w: 400.0, gui_h: 300.0, under_cursor: None, can_teleport: true, player_info: String::new(), keys: &[], toast: None, tutorial: None };
        let mut c = draw::Canvas::default();
        fullscreen::draw(&mut s.fs, &mut c, &draw::Approx, &inp);
        let (r, _) = s.fs.menu.as_ref().unwrap().items.iter().find(|(_, i)| *i == MenuItem::Create).copied().unwrap();
        s.fs_press(r.x + 2.0, r.y + 2.0, 0, 400.0, 300.0, "d", 70);
        assert_eq!(s.waypoints.len(), 1);
        assert!(s.fs.rename.is_some());
    }

    #[test]
    fn slider_drag_sets_size() {
        let mut s = State::default();
        s.fs.panel = Panel::Options;
        let sc = crate::render::Scene { x: 0.0, y: 64.0, z: 0.0, yaw: 0.0, dim: "d", biome: "", waypoints: &[], entities: &[], extra: &[] };
        let cfg = s.cfg.clone();
        let inp = fullscreen::FullInput { cfg: &cfg, scene: &sc, key: None, map_type: shading::MapType::Day, cursor: (0.0, 0.0), gui_w: 640.0, gui_h: 480.0, under_cursor: None, can_teleport: false, player_info: String::new(), keys: &[], toast: None, tutorial: None };
        let mut c = draw::Canvas::default();
        fullscreen::draw(&mut s.fs, &mut c, &draw::Approx, &inp);
        let (r, _) = *s.fs.buttons.iter().find(|(_, a)| *a == Action::Slider("size")).unwrap();
        s.fs_press(r.x + r.w - 3.0, r.y + 5.0, 0, 640.0, 480.0, "d", 64);
        assert_eq!(s.cfg.preset().size, 256.0);
        s.slider_to(r.x + 3.0);
        assert_eq!(s.cfg.preset().size, 64.0);
    }

    #[test]
    fn delete_asks_first() {
        let mut s = State::default();
        s.current_dim = "d".into();
        s.store.set("d", tiles::Layer::Surface, 1, 1, tiles::Cell { flags: tiles::Cell::EXPLORED, ..Default::default() });
        s.apply(Action::DeleteMap, 400.0, 300.0);
        assert!(s.fs.confirm_delete);
        assert!(s.store.tiles.get("d", tiles::Layer::Surface, 1, 1).is_some());
        s.apply(Action::ConfirmDelete(true), 400.0, 300.0);
        assert!(s.store.tiles.get("d", tiles::Layer::Surface, 1, 1).is_none());
        assert!(s.toast_now().is_some());
    }

    #[test]
    fn keys() {
        let mut s = State::default();
        let z = s.cfg.preset().zoom;
        s.on_key("zoom_in");
        assert_eq!(s.cfg.preset().zoom, z + 1);
        s.on_key("map_toggle");
        assert_eq!(s.requests, vec![Request::OpenMap]);
    }
}
