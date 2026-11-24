// The game side of JourneyAtlas: compiled inside the client as `crate::game::journeyatlas_bridge`
// (horizon-mod.toml's [[native.module]]), so it reads the world, the player and the mobs directly
// and replays the drawing the mod crate prepares.

use super::{Game, Screen};
use ::mod_journeyatlas as jm;
use horizon_client::hud::{HudBatch, MOD_IMAGE_SPRITE, rgba};
use jm::draw::{Canvas, Measure, Op};
use jm::render::{ImageKey, Scene};
use jm::shading::MapType;
use jm::tiles::{Cell, Layer};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Per-state facts the scan needs: map colour (0 = none), and flags.
struct Palette {
    rgb: Vec<[u8; 3]>,
    flags: Vec<u8>,
}
const P_AIR: u8 = 1;
const P_WATER: u8 = 2;
const P_LAVA: u8 = 4;
const P_NOCOLOR: u8 = 8;

/// What the bridge remembers between frames (the mod crate keeps the rest).
#[derive(Default)]
struct Runtime {
    palette: Option<Palette>,
    palette_states: usize,
    /// Fingerprint of each chunk when last scanned, per layer.
    scanned: HashMap<(i32, i32, Layer), u64>,
    dim: String,
    last_save_frame: u64,
}

fn runtime() -> std::sync::MutexGuard<'static, Runtime> {
    static RT: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RT.get_or_init(|| Mutex::new(Runtime::default())).lock().unwrap_or_else(|e| e.into_inner())
}

fn build_palette(game: &Game<'_>) -> Palette {
    let facts = game.res.facts();
    let n = facts.states.len();
    let mut rgb = Vec::with_capacity(n);
    let mut flags = Vec::with_capacity(n);
    for (i, f) in facts.states.iter().enumerate() {
        let id = usize::from(f.map_color);
        let col = horizon_items::blocks::MAP_COLORS.get(id).copied().unwrap_or(0);
        rgb.push([(col >> 16) as u8, (col >> 8) as u8, col as u8]);
        let mut fl = 0;
        if facts.is_air(horizon_world::registry::BlockStateId(i as u32)) {
            fl |= P_AIR;
        }
        match f.fluid {
            horizon_client::blocks::Fluid::Water => fl |= P_WATER,
            horizon_client::blocks::Fluid::Lava => fl |= P_LAVA,
            _ => {}
        }
        if f.map_color == 0 {
            fl |= P_NOCOLOR;
        }
        flags.push(fl);
    }
    Palette { rgb, flags }
}

fn chunk_fingerprint(chunk: &horizon_client::chunk::Chunk) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in &chunk.sections {
        h ^= std::sync::Arc::as_ptr(s) as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// The surface of one chunk, like the day map: the top block with a map colour, water
/// over its floor with its depth.
fn scan_surface(chunk: &horizon_client::chunk::Chunk, pal: &Palette, min_y: i32, top_y: i32, ceiling: bool) -> Vec<(i32, i32, Cell)> {
    let mut out = Vec::with_capacity(256);
    let sections_top = min_y + chunk.sections.len() as i32 * 16 - 1;
    // In the Nether the roof is skipped: start below it.
    let start = if ceiling { (min_y + 120).min(sections_top) } else { top_y.min(sections_top) };
    for lz in 0..16 {
        for lx in 0..16 {
            let mut y = start;
            let mut water_top: Option<i32> = None;
            let mut in_roof = ceiling;
            let mut cell = Cell::default();
            while y >= min_y {
                let st = chunk.get(lx, y, lz) as usize;
                let fl = pal.flags.get(st).copied().unwrap_or(P_AIR);
                if in_roof {
                    // Under a ceiling, look for the first air below the roof first.
                    if fl & P_AIR != 0 {
                        in_roof = false;
                    }
                    y -= 1;
                    continue;
                }
                if fl & P_AIR != 0 || (fl & P_NOCOLOR != 0 && fl & (P_WATER | P_LAVA) == 0) {
                    y -= 1;
                    continue;
                }
                if fl & P_WATER != 0 {
                    water_top.get_or_insert(y);
                    y -= 1;
                    continue;
                }
                let light = chunk.light(lx, water_top.unwrap_or(y) + 1, lz) & 0x0f;
                cell = match water_top {
                    Some(wy) => {
                        let depth = (wy - y).clamp(1, 255) as u8;
                        let w = [64u8, 64, 255];
                        let bed = pal.rgb[st];
                        let k = (f32::from(depth) * 0.12).clamp(0.55, 0.92);
                        let rgb = [0, 1, 2].map(|i| (f32::from(w[i]) * k + f32::from(bed[i]) * (1.0 - k)) as u8);
                        Cell { rgb, y: wy as i16, water: depth, light, flags: Cell::EXPLORED | Cell::WATER }
                    }
                    None => Cell { rgb: pal.rgb[st], y: y as i16, water: 0, light, flags: Cell::EXPLORED },
                };
                break;
            }
            if !cell.explored()
                && let Some(wy) = water_top
            {
                cell = Cell { rgb: [48, 48, 220], y: wy as i16, water: 24, light: 0, flags: Cell::EXPLORED | Cell::WATER };
            }
            if cell.explored() {
                out.push((chunk.x * 16 + lx, chunk.z * 16 + lz, cell));
            }
        }
    }
    out
}

/// The cave map of one 16-block slice: the floor under the first air going down from the slice's top.
fn scan_cave(chunk: &horizon_client::chunk::Chunk, pal: &Palette, min_y: i32, slice: i32) -> Vec<(i32, i32, Cell)> {
    let mut out = Vec::with_capacity(256);
    let top = slice * 16 + 15;
    let bottom = (slice * 16 - 16).max(min_y);
    for lz in 0..16 {
        for lx in 0..16 {
            let mut seen_air = false;
            let mut y = top;
            while y >= bottom {
                let st = chunk.get(lx, y, lz) as usize;
                let fl = pal.flags.get(st).copied().unwrap_or(P_AIR);
                if fl & P_AIR != 0 || (fl & P_NOCOLOR != 0 && fl & (P_WATER | P_LAVA) == 0) {
                    seen_air = true;
                } else if seen_air {
                    let light = chunk.light(lx, y + 1, lz) & 0x0f;
                    let flags = Cell::EXPLORED | if fl & P_WATER != 0 { Cell::WATER } else { 0 };
                    out.push((chunk.x * 16 + lx, chunk.z * 16 + lz, Cell { rgb: pal.rgb[st], y: y as i16, water: u8::from(fl & P_WATER != 0) * 4, light, flags }));
                    break;
                }
                y -= 1;
            }
        }
    }
    out
}

fn world_key(game: &Game<'_>) -> String {
    if let Some(i) = game.integrated.as_ref().filter(|i| !i.save_folder.is_empty()) {
        return format!("sp_{}", i.save_folder);
    }
    if let Some(r) = game.remote.as_ref() {
        return format!("mp_{}", r.session.address);
    }
    String::new()
}

/// Under a roof (cave map automatically, as map mods does).
fn underground(game: &Game<'_>, x: i32, y: i32, z: i32) -> bool {
    let Some(chunk) = game.world.chunks.get_chunk(x.div_euclid(16), z.div_euclid(16)) else { return false };
    (chunk.light(x & 15, y + 1, z & 15) >> 4) < 4 && y < 60
}

fn chosen_type(st: &jm::State, game: &Game<'_>, px: i32, py: i32, pz: i32, ceiling: bool) -> MapType {
    if !st.cfg.auto_map_type {
        return st.cfg.map_type;
    }
    if ceiling || underground(game, px, py, pz) {
        return MapType::Cave;
    }
    let t = game.world.day_time.rem_euclid(24000);
    if (13000..23000).contains(&t) { MapType::Night } else { MapType::Day }
}

fn layer_for(ty: MapType, py: i32) -> Layer {
    if ty == MapType::Cave { Layer::Cave(py.div_euclid(16)) } else { Layer::Surface }
}

/// Once a frame: follow the world, scan chunks, react to keys and deaths, keep the map image.
pub fn frame(game: &mut Game<'_>) {
    jm::i18n::set_language(&horizon_client::lang::code());
    let mut st = jm::state();
    let mut rt = runtime();
    st.frame += 1;
    let data = game.instance_dir().join("journeyatlas");
    // Maps and waypoints saved under the mod's first name move to its new folder once.
    let old = game.instance_dir().join("journeymap_rs");
    if !data.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &data);
    }
    st.set_root(data);

    let key = if game.player.is_some() { world_key(game) } else { String::new() };
    st.enter_world(&key);
    // Requests from hooks that can't change the game.
    let requests: Vec<jm::Request> = std::mem::take(&mut st.requests);
    for r in requests {
        match r {
            jm::Request::OpenMap => {
                if game.mod_screen().is_none() && game.playing() {
                    st.fs.follow = true;
                    st.image = None;
                    if game.mod_screen_open(jm::MOD_ID) {
                        st.opened_at = st.frame;
                        let f = st.frame;
                        st.tutorial.event(jm::tutorial::Event::MapOpened, f);
                    }
                }
            }
            jm::Request::CloseMap => {
                game.mod_screen_close(jm::MOD_ID);
                st.image = None;
            }
            jm::Request::Rescan => rt.scanned.clear(),
            jm::Request::SaveImage => save_image(game, &mut st),
            jm::Request::Teleport(x, y, z) => {
                if st.can_teleport {
                    game.click_command(&format!("tp @s {x} {y} {z}"));
                }
            }
        }
    }
    st.can_teleport = game.ui.registries.permission >= 2;
    st.map_open = game.mod_screen() == Some(jm::MOD_ID);
    let f = st.frame;
    st.tutorial.tick(f);
    if st.tutorial.dirty && f % 60 == 0 {
        st.save_all();
    }
    st.current_dim = game.dim.id().to_owned();
    let Some(player) = game.player.as_ref() else {
        return;
    };
    let (px, py, pz) = (player.position.x, player.position.y, player.position.z);
    let (bx, by, bz) = (px.floor() as i32, py.floor() as i32, pz.floor() as i32);
    let dim = game.dim.id().to_owned();
    if rt.dim != dim {
        rt.dim = dim.clone();
        rt.scanned.clear();
        st.image = None;
    }

    // A waypoint where the player died.
    let dead = game.screen == Screen::Dead;
    if dead && !st.was_dead && st.cfg.create_death_waypoints {
        st.add_waypoint(None, bx, by, bz, &dim, jm::waypoints::Kind::Death);
    }
    st.was_dead = dead;
    // Back at the last death: the waypoint has done its job.
    if st.cfg.auto_remove_death && !dead {
        let before = st.waypoints.len();
        st.waypoints.retain(|w| {
            w.kind != jm::waypoints::Kind::Death
                || w.dim != dim
                || (w.x - bx).pow(2) + (w.z - bz).pow(2) > 6 * 6
        });
        if st.waypoints.len() != before {
            st.waypoints_dirty = true;
        }
    }

    // The block palette follows the resources.
    let states = game.res.facts().states.len();
    if rt.palette.is_none() || rt.palette_states != states {
        rt.palette = Some(build_palette(game));
        rt.palette_states = states;
        rt.scanned.clear();
    }

    let ceiling = game.world.shape.has_ceiling;
    let min_y = game.world.shape.min_y;
    let top_y = min_y + game.world.shape.height - 1;
    let ty = chosen_type(&st, game, bx, by, bz, ceiling);
    st.shown_type = ty;
    let layer = layer_for(ty, by);

    // Scan chunks near the player, nearest first, a few per frame; nearby ones again when changed.
    let (cx, cz) = (bx.div_euclid(16), bz.div_euclid(16));
    let r = st.cfg.scan_radius;
    let mut budget = if st.cfg.mapping { 6 } else { 0 };
    let pal = rt.palette.take();
    if let Some(pal) = pal.as_ref().filter(|_| budget > 0) {
        'rings: for ring in 0..=r {
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dz.abs() != ring {
                        continue;
                    }
                    let (kx, kz) = (cx + dx, cz + dz);
                    let Some(chunk) = game.world.chunks.get_chunk(kx, kz) else { continue };
                    let fp = chunk_fingerprint(chunk);
                    for l in [Layer::Surface, layer] {
                        if rt.scanned.get(&(kx, kz, l)) == Some(&fp) {
                            continue;
                        }
                        let cells = match l {
                            Layer::Surface => scan_surface(chunk, pal, min_y, top_y, ceiling),
                            Layer::Cave(s) => scan_cave(chunk, pal, min_y, s),
                        };
                        for (x, z, c) in cells {
                            st.store.set(&dim, l, x, z, c);
                        }
                        rt.scanned.insert((kx, kz, l), fp);
                        budget -= 1;
                        if budget <= 0 {
                            break 'rings;
                        }
                    }
                }
            }
        }
    }
    rt.palette = pal;

    // Save now and then; drop far regions.
    if st.frame - rt.last_save_frame > 1200 {
        rt.last_save_frame = st.frame;
        st.save_all();
        st.store.evict_far(&dim, bx, bz, 4);
    }

    // The map image for what is on screen: the fullscreen map when open, else the minimap.
    let [gw, gh] = gui_size(game);
    let fullscreen = game.mod_screen() == Some(jm::MOD_ID);
    let wanted = if fullscreen {
        if st.fs.follow {
            st.fs.wx = px;
            st.fs.wz = pz;
        }
        let half_x = f64::from(gw / 2.0 / st.fs.scale) + 8.0;
        let half_z = f64::from(gh / 2.0 / st.fs.scale) + 8.0;
        Some(ImageKey::covering_at(&dim, layer, ty, st.fs.wx, st.fs.wz, half_x, half_z, st.fs.scale))
    } else if st.cfg.minimap {
        let p = *st.cfg.preset();
        let half = f64::from(p.size * 0.75 / jm::config::ZOOMS[p.zoom]) + 8.0;
        Some(ImageKey::covering(&dim, layer, ty, px, pz, half, half))
    } else {
        None
    };
    let Some(wanted) = wanted else { return };
    let stale = match &st.image {
        None => true,
        Some(have) => {
            !have.covers(&wanted)
                || (st.store.revision != st.image_revision && st.frame - st.image_frame >= if fullscreen { 60 } else { 15 })
        }
    };
    if stale {
        let pixels = jm::render::build_image(&mut st.store, &wanted);
        if game.mod_image(wanted.w as u32, wanted.h as u32, &pixels) {
            st.image_revision = st.store.revision;
            st.image_frame = st.frame;
            st.image = Some(wanted);
        }
    }
}

/// Saves what the fullscreen map shows (or the area around the player) as a PNG under
/// `<instance>/journeyatlas/exports/`.
fn save_image(game: &Game<'_>, st: &mut jm::State) {
    let Some(player) = game.player.as_ref() else { return };
    let [gw, gh] = gui_size(game);
    let dim = game.dim.id().to_owned();
    let layer = layer_for(st.shown_type, player.position.y.floor() as i32);
    let half_x = f64::from(gw / 2.0 / st.fs.scale);
    let half_z = f64::from(gh / 2.0 / st.fs.scale);
    let key = ImageKey::covering_at(&dim, layer, st.shown_type, st.fs.wx, st.fs.wz, half_x, half_z, st.fs.scale.max(1.0));
    let pixels = jm::render::build_image(&mut st.store, &key);
    let png = jm::png::encode(key.w as u32, key.h as u32, &pixels);
    let Some(root) = st.root.clone() else { return };
    let dir = root.join("exports");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = format!("{}_{}_{stamp}.png", jm::store::safe_token(&st.world), jm::store::safe_token(&dim));
    let path = dir.join(&name);
    let ok = std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, png).is_ok();
    if ok {
        st.notify(jm::i18n::tf("toast_saved", &[&name]));
    }
}

/// "J" from "key.keyboard.j", "Left Bracket" from "key.keyboard.left.bracket".
fn key_name(key: &str) -> String {
    let k = key.rsplit_once("keyboard.").map_or(key, |(_, k)| k);
    k.split('.')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn key_rows(game: &Game<'_>) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = [
        ("map_toggle", "k_map"),
        ("create_waypoint", "k_create"),
        ("zoom_in", "k_zoom_in"),
        ("zoom_out", "k_zoom_out"),
        ("minimap_type", "k_type"),
        ("minimap_preset", "k_preset"),
        ("minimap_toggle", "k_toggle"),
    ]
    .into_iter()
    .filter_map(|(id, label)| game.mod_key(jm::MOD_ID, id).map(|(k, _)| (key_name(&k), jm::i18n::t(label))))
    .collect();
    rows.push(("WASD / Arrows".into(), jm::i18n::t("k_pan")));
    rows.push(("Right-click".into(), jm::i18n::t("k_menu")));
    rows.push(("Wheel".into(), jm::i18n::t("k_wheel")));
    rows
}

fn gui_size(game: &Game<'_>) -> [f32; 2] {
    let size = game.renderer.as_ref().map_or([1280, 720], |r| [r.config.width, r.config.height]);
    let b = HudBatch::new(size, game.settings.gui_scale_inputs());
    [b.width, b.height]
}

struct Font<'a>(&'a horizon_client::hud::HudAtlas);
impl Measure for Font<'_> {
    fn text_width(&self, text: &str) -> f32 {
        HudBatch::text_width(self.0, text)
    }
}

fn replay(game: &Game<'_>, batch: &mut HudBatch, ops: &[Op], image: Option<&ImageKey>) {
    let atlas = &game.gui.atlas;
    let slot = atlas.sprites.get(MOD_IMAGE_SPRITE).copied();
    for op in ops {
        match op {
            Op::Fill { x, y, w, h, argb } => game.native_fill(batch, [*x, *y, *w, *h], *argb),
            Op::Quad { p, argb } => game.native_quad(batch, *p, *argb),
            Op::Text { x, y, text, argb, shadow } => game.native_text(batch, text, [*x, *y], *argb, *shadow),
            Op::Sprite { name, src, dst, tint } => {
                game.native_sprite(batch, name, *src, *dst, Some(*tint));
            }
            Op::SpriteQuad { name, p, uv, tint } => {
                if let Some(r) = atlas.sprites.get(*name).copied() {
                    let uvs = uv.map(|[u, v]| [r.u0 + (r.u1 - r.u0) * u, r.v0 + (r.v1 - r.v0) * v]);
                    let [a, rr, g, b] = tint.to_be_bytes();
                    batch.quad(*p, uvs, rgba(rr, g, b, a));
                }
            }
            Op::Head { sprite, uv, dst } => {
                if let Some(r) = atlas.sprites.get(sprite.as_str()).copied() {
                    let (du, dv) = (r.u1 - r.u0, r.v1 - r.v0);
                    let (a0, b0, a1, b1) = (r.u0 + du * uv[0], r.v0 + dv * uv[1], r.u0 + du * uv[2], r.v0 + dv * uv[3]);
                    let [x, y, w, h] = *dst;
                    batch.quad([[x, y], [x + w, y], [x + w, y + h], [x, y + h]], [[a0, b0], [a1, b0], [a1, b1], [a0, b1]], u32::MAX);
                }
            }
            Op::Image { p, t, alpha } => {
                let (Some(r), Some(_)) = (slot, image) else { continue };
                // Texels of the slot to atlas UVs.
                let (du, dv) = ((r.u1 - r.u0) / r.w as f32, (r.v1 - r.v0) / r.h as f32);
                let uvs = t.map(|[tx, ty]| [r.u0 + tx * du, r.v0 + ty * dv]);
                batch.quad(*p, uvs, rgba(255, 255, 255, *alpha));
            }
        }
    }
}

fn biome_name(game: &Game<'_>, x: i32, y: i32, z: i32) -> String {
    let Some(biomes) = game.world_biomes.as_ref() else { return String::new() };
    let Some(chunk) = game.world.chunks.get_chunk(x.div_euclid(16), z.div_euclid(16)) else { return String::new() };
    let id = biomes.name(chunk.biome(x & 15, y, z & 15)).to_owned();
    let key = format!("biome.{}", id.replace(':', "."));
    horizon_client::lang::get_or(&key, &id)
}

/// The face of a model: the largest north-facing polygon of its "head" part, as texture corners.
fn head_uv(part: &horizon_client::mobs::Part) -> Option<[f32; 4]> {
    fn find<'a>(p: &'a horizon_client::mobs::Part) -> Option<&'a horizon_client::mobs::Part> {
        if p.name == "head" {
            return Some(p);
        }
        p.children.iter().find_map(find)
    }
    let head = find(part)?;
    fn consider(p: &horizon_client::mobs::Part, best: &mut Option<([f32; 4], f32)>) {
        for poly in &p.polygons {
            if poly.normal[2] > -0.9 {
                continue;
            }
            let (mut u0, mut v0, mut u1, mut v1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for v in &poly.vertices {
                u0 = u0.min(v[3]);
                v0 = v0.min(v[4]);
                u1 = u1.max(v[3]);
                v1 = v1.max(v[4]);
            }
            let area = (u1 - u0) * (v1 - v0);
            if area > 0.0 && best.is_none_or(|(_, a)| area > a) {
                *best = Some(([u0, v0, u1, v1], area));
            }
        }
    }
    let mut best: Option<([f32; 4], f32)> = None;
    consider(head, &mut best);
    if best.is_none() {
        for child in &head.children {
            consider(child, &mut best);
        }
    }
    best.map(|(uv, _)| uv)
}

fn sprite_key(texture: &str) -> String {
    let t = texture.split_once(':').map_or(texture, |(_, p)| p);
    let t = t.strip_prefix("textures/").unwrap_or(t);
    t.strip_suffix(".png").unwrap_or(t).to_owned()
}

fn entities(game: &Game<'_>, px: f64, pz: f64, range: f64) -> Vec<jm::render::Ent> {
    let own = game.own_entity_id();
    let atlas = &game.gui.atlas;
    game.mobs
        .iter()
        .filter(|m| i64::from(m.id) != i64::from(own))
        .filter(|m| (m.position.x - px).abs() < range && (m.position.z - pz).abs() < range)
        .map(|m| {
            use jm::config::kind;
            let id = m.kind.id.as_str();
            let kind = if id == "minecraft:player" {
                kind::PLAYER
            } else if matches!(id, "minecraft:villager" | "minecraft:wandering_trader") {
                kind::VILLAGER
            } else if m.kind.category == "monster" {
                kind::MONSTER
            } else if m.extra.tame {
                kind::PET
            } else {
                kind::ANIMAL
            };
            let name = m.kind.id.rsplit(':').next().unwrap_or("").replace('_', " ");
            let head = head_uv(&m.kind.model).and_then(|uv| {
                let tex = m.skin.as_deref().unwrap_or(&m.kind.texture);
                let key = sprite_key(tex);
                atlas.sprites.contains_key(key.as_str()).then_some(jm::render::Head { sprite: key, uv })
            });
            jm::render::Ent { x: m.position.x, z: m.position.z, kind, name, head }
        })
        .collect()
}

/// Waypoints projected into the view, the in-world markers.
fn world_markers(game: &Game<'_>, st: &jm::State, gw: f32, gh: f32) -> Vec<jm::render::Marker> {
    let Some(camera) = game.native_camera_frame.as_ref() else { return Vec::new() };
    let dim = game.dim.id();
    let pos = camera.position;
    st.waypoints
        .iter()
        .filter(|w| w.enabled && w.dim == dim)
        .filter_map(|w| {
            let at = [f64::from(w.x) + 0.5, f64::from(w.y) + 1.0, f64::from(w.z) + 0.5];
            let ndc = horizon_client::camera::project_point(&camera.projection, pos, camera.yaw, camera.pitch, game.posed_roll, at)?;
            if ndc[0].abs() > 1.0 || ndc[1].abs() > 1.0 {
                return None;
            }
            let (dx, dy, dz) = (at[0] - pos[0], at[1] - pos[1], at[2] - pos[2]);
            let to_screen = |p: [f32; 3]| [(p[0] + 1.0) / 2.0 * gw, (1.0 - p[1]) / 2.0 * gh];
            // The beam: from the waypoint up to the sky, as the beacon.
            let mut beam = Vec::new();
            let mut y = f64::from(w.y);
            while y < f64::from(w.y) + 256.0 {
                let p = [at[0], y, at[2]];
                let side = [at[0] + 0.35, y, at[2]];
                match (
                    horizon_client::camera::project_point(&camera.projection, pos, camera.yaw, camera.pitch, game.posed_roll, p),
                    horizon_client::camera::project_point(&camera.projection, pos, camera.yaw, camera.pitch, game.posed_roll, side),
                ) {
                    (Some(a), Some(b)) => {
                        let (sa, sb) = (to_screen(a), to_screen(b));
                        let width = ((sa[0] - sb[0]).hypot(sa[1] - sb[1]) * 2.0).clamp(1.5, 24.0);
                        beam.push((sa, width));
                        if sa[1] < -50.0 {
                            break;
                        }
                    }
                    _ => break,
                }
                y += 4.0;
            }
            Some(jm::render::Marker {
                sx: (ndc[0] + 1.0) / 2.0 * gw,
                sy: (1.0 - ndc[1]) / 2.0 * gh,
                name: w.name.clone(),
                dist: (dx * dx + dy * dy + dz * dz).sqrt(),
                color: w.color,
                death: w.kind == jm::waypoints::Kind::Death,
                beam,
            })
        })
        .collect()
}

/// Time of day and weather lines, as the info slots.
fn info_lines(game: &Game<'_>, cfg: &jm::config::Config) -> Vec<String> {
    let mut out = Vec::new();
    if cfg.show_time {
        let t = game.world.day_time.rem_euclid(24000);
        let hours = (t / 1000 + 6) % 24;
        let minutes = (t % 1000) * 60 / 1000;
        out.push(jm::i18n::tf("time", &[&format!("{hours:02}:{minutes:02}")]));
    }
    if cfg.show_weather {
        let w = &game.world.weather;
        let key = if w.thunder_level.1 > 0.5 {
            "weather_thunder"
        } else if w.rain_level.1 > 0.2 {
            "weather_rain"
        } else {
            "weather_clear"
        };
        out.push(jm::i18n::t(key));
    }
    out
}

/// The minimap and the in-world waypoints, on the HUD.
pub fn hud(game: &Game<'_>, batch: &mut HudBatch) {
    if !game.playing() || game.mod_screen() == Some(jm::MOD_ID) {
        return;
    }
    let Some(player) = game.player.as_ref() else { return };
    let st = jm::state();
    if st.cfg.show_world_waypoints {
        let [gw, gh] = [batch.width, batch.height];
        let markers = world_markers(game, &st, gw, gh);
        if !markers.is_empty() {
            let mut c = Canvas::default();
            jm::render::draw_world_waypoints(&mut c, &Font(&game.gui.atlas), &markers, gw, gh);
            replay(game, batch, &c.ops, None);
        }
    }
    if let Some(step) = st.tutorial.step {
        let [gw, gh] = [batch.width, batch.height];
        let mut c = Canvas::default();
        jm::tutorial::card(&mut c, &Font(&game.gui.atlas), step, gw / 2.0, gh - 120.0, false, false, (0.0, 0.0));
        replay(game, batch, &c.ops, None);
    }
    if !st.cfg.minimap {
        return;
    }
    let (px, py, pz) = (player.position.x, player.position.y, player.position.z);
    let dim = game.dim.id();
    let biome = if st.cfg.preset().show_biome { biome_name(game, px.floor() as i32, py.floor() as i32, pz.floor() as i32) } else { String::new() };
    let ents = entities(game, px, pz, 128.0);
    let extra = info_lines(game, &st.cfg);
    let sc = Scene { x: px, y: py, z: pz, yaw: game.view.y_rot, dim, biome: &biome, waypoints: &st.waypoints, entities: &ents, extra: &extra };
    let [gw, gh] = [batch.width, batch.height];
    let mut c = Canvas::default();
    let font = Font(&game.gui.atlas);
    let image = st.image.as_ref().filter(|k| k.dim == dim);
    jm::render::draw_minimap(&mut c, &font, &st.cfg, &sc, image, gw, gh);
    replay(game, batch, &c.ops, image);
}

/// The fullscreen map, over our own screen.
pub fn screen(game: &Game<'_>, batch: &mut HudBatch) {
    if game.mod_screen() != Some(jm::MOD_ID) {
        return;
    }
    let Some(player) = game.player.as_ref() else { return };
    let mut st = jm::state();
    let (px, py, pz) = (player.position.x, player.position.y, player.position.z);
    let dim = game.dim.id();
    let [gw, gh] = [batch.width, batch.height];
    let cursor = game.cursor();
    let (cx, cz) = st.fs.view(gw, gh).to_world(cursor[0], cursor[1]);
    let under = {
        let ty = st.shown_type;
        let layer = layer_for(ty, py.floor() as i32);
        let cell = st.store.get(dim, layer, cx.floor() as i32, cz.floor() as i32);
        cell.filter(|c| c.explored()).map(|c| {
            let b = biome_name(game, cx.floor() as i32, c.y.into(), cz.floor() as i32);
            if b.is_empty() { format!("y {}", c.y) } else { format!("y {}  {b}", c.y) }
        })
    };
    let ents = entities(game, px, pz, 512.0);
    let extra: Vec<String> = Vec::new();
    let keys = key_rows(game);
    let toast = st.toast_now().map(str::to_owned);
    let player_biome = biome_name(game, px.floor() as i32, py.floor() as i32, pz.floor() as i32);
    let player_info = format!("x: {}, z: {}, y: {}  {player_biome}", px.floor() as i64, pz.floor() as i64, py.floor() as i64);
    let waypoints = st.waypoints.clone();
    let cfg = st.cfg.clone();
    let image = st.image.clone().filter(|k| k.dim == dim);
    let sc = Scene { x: px, y: py, z: pz, yaw: game.view.y_rot, dim, biome: "", waypoints: &waypoints, entities: &ents, extra: &extra };
    let inp = jm::fullscreen::FullInput {
        cfg: &cfg,
        scene: &sc,
        key: image.as_ref(),
        map_type: st.shown_type,
        cursor: (cursor[0], cursor[1]),
        gui_w: gw,
        gui_h: gh,
        under_cursor: under,
        can_teleport: st.can_teleport,
        player_info: player_info.clone(),
        keys: &keys,
        toast: toast.as_deref(),
        tutorial: st.tutorial.step,
    };
    let mut c = Canvas::default();
    let font = Font(&game.gui.atlas);
    jm::fullscreen::draw(&mut st.fs, &mut c, &font, &inp);
    drop(st);
    replay(game, batch, &c.ops, image.as_ref());
}

fn ground_at(game: &Game<'_>, x: i32, z: i32, fallback: i32) -> i32 {
    let st = jm::state();
    let dim = game.dim.id();
    st.store
        .tiles
        .get(dim, Layer::Surface, x, z)
        .filter(|c| c.explored())
        .map_or(fallback, |c| i32::from(c.y) + 1)
}

pub fn press(game: &Game<'_>, button: i32, canceled: &mut bool) {
    if game.mod_screen() != Some(jm::MOD_ID) {
        return;
    }
    *canceled = true;
    let [gw, gh] = gui_size(game);
    let [mx, my] = game.cursor();
    let fallback = game.player.as_ref().map_or(64, |p| p.position.y.floor() as i32);
    let (wx, wz) = jm::state().fs.view(gw, gh).to_world(mx, my);
    let gy = ground_at(game, wx.floor() as i32, wz.floor() as i32, fallback);
    jm::state().fs_press(mx, my, button, gw, gh, game.dim.id(), gy);
}

pub fn release(game: &Game<'_>, canceled: &mut bool) {
    if game.mod_screen() != Some(jm::MOD_ID) {
        return;
    }
    *canceled = true;
    let mut st = jm::state();
    st.fs.dragging = false;
    st.fs.slider = None;
}

pub fn drag(game: &Game<'_>, delta: [f32; 2], button: i32, canceled: &mut bool) {
    if game.mod_screen() != Some(jm::MOD_ID) {
        return;
    }
    *canceled = true;
    let mut st = jm::state();
    if button == 0 && st.fs.slider.is_some() {
        let [mx, _] = game.cursor();
        st.slider_to(mx);
        return;
    }
    if button == 0 && st.fs.dragging {
        st.fs.pan_pixels(delta[0], delta[1]);
    }
}

pub fn scroll(game: &Game<'_>, scroll: [f64; 2], canceled: &mut bool) {
    if game.mod_screen() != Some(jm::MOD_ID) {
        return;
    }
    *canceled = true;
    let [gw, gh] = gui_size(game);
    let [mx, my] = game.cursor();
    let mut st = jm::state();
    if st.fs.panel != jm::fullscreen::Panel::None && st.fs.over_ui(mx, my, gw) {
        st.fs.scroll = if scroll[1] > 0.0 { st.fs.scroll.saturating_sub(1) } else { st.fs.scroll + 1 };
        return;
    }
    st.fs.zoom(scroll[1].signum(), Some((mx, my, gw, gh)));
}

pub fn screen_key(game: &Game<'_>, key: &str, canceled: &mut bool) {
    if game.mod_screen() != Some(jm::MOD_ID) {
        return;
    }
    let map_key = game.mod_key(jm::MOD_ID, "map_toggle").map(|(k, _)| k);
    if jm::state().fs_key(key, map_key.as_deref()) {
        *canceled = true;
    }
}

pub fn screen_char(game: &Game<'_>, c: char, canceled: &mut bool) {
    if game.mod_screen() != Some(jm::MOD_ID) {
        return;
    }
    if jm::state().fs_char(c) {
        *canceled = true;
    }
}

/// A key while playing.
pub fn input_key(game: &mut Game<'_>, key: &str, action: mod_api::KeyAction, modifiers: u32) {
    if action != mod_api::KeyAction::Press || !game.playing() || game.mod_screen().is_some() || key == "key.keyboard.unknown" {
        return;
    }
    for (id, _) in jm::KEYS {
        let bound = game
            .mod_key(jm::MOD_ID, id)
            .is_some_and(|(k, mods)| k == key && mods.bits & 7 == modifiers & 7);
        if !bound {
            continue;
        }
        let effect = jm::state().on_key(id);
        if effect == Some(jm::KeyEffect::CreateWaypoint)
            && let Some(p) = game.player.as_ref()
        {
            let (x, y, z) = (p.position.x.floor() as i32, p.position.y.floor() as i32, p.position.z.floor() as i32);
            let dim = game.dim.id().to_owned();
            let mut st = jm::state();
            st.add_waypoint(None, x, y, z, &dim, jm::waypoints::Kind::Normal);
        }
        return;
    }
}

pub fn disconnect() {
    jm::state().save_all();
}
