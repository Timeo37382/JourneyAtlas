//! Player options, saved as `key=value` lines.

use crate::shading::MapType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    TopCenter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Square,
    Circle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub minimap: bool,
    /// Which of the two map mods minimap presets is active.
    pub preset: u8,
    pub presets: [Preset; 2],
    pub map_type: MapType,
    /// Automatic day/night/cave switch (cave when under a roof).
    pub auto_map_type: bool,
    pub show_waypoints: bool,
    pub show_world_waypoints: bool,
    pub show_mobs: bool,
    pub show_players: bool,
    pub show_villagers: bool,
    pub show_animals: bool,
    pub show_pets: bool,
    /// Mapping on (scanning chunks); off pauses it.
    pub mapping: bool,
    pub show_entity_names: bool,
    /// Mobs as their heads (the look) rather than coloured dots.
    pub mob_heads: bool,
    /// Mob icon size on the maps (1.0 = 8 px on the minimap).
    pub mob_scale: f32,
    /// Opacity of the map picture in the minimap, 0.2..1.
    pub map_opacity: f32,
    pub show_time: bool,
    pub show_weather: bool,
    /// Chunk / region grid on the fullscreen map.
    pub grid: bool,
    /// Death waypoint removed once the player gets back to it.
    pub auto_remove_death: bool,
    pub create_death_waypoints: bool,
    /// Scan radius in chunks around the player.
    pub scan_radius: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub corner: Corner,
    pub shape: Shape,
    /// Size in GUI pixels.
    pub size: f32,
    /// Index into [`ZOOMS`].
    pub zoom: usize,
    pub rotate: bool,
    pub show_coords: bool,
    pub show_biome: bool,
    pub show_compass: bool,
}

/// Screen pixels per block, from far to close.
pub const ZOOMS: [f32; 7] = [0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 8.0];

impl Default for Config {
    fn default() -> Self {
        Self {
            minimap: true,
            preset: 0,
            presets: [
                Preset {
                    corner: Corner::TopRight,
                    shape: Shape::Square,
                    size: 110.0,
                    zoom: 2,
                    rotate: false,
                    show_coords: true,
                    show_biome: true,
                    show_compass: true,
                },
                Preset {
                    corner: Corner::TopRight,
                    shape: Shape::Circle,
                    size: 120.0,
                    zoom: 3,
                    rotate: true,
                    show_coords: true,
                    show_biome: false,
                    show_compass: true,
                },
            ],
            map_type: MapType::Day,
            auto_map_type: true,
            show_waypoints: true,
            show_world_waypoints: true,
            show_mobs: true,
            show_players: true,
            show_villagers: true,
            show_animals: true,
            show_pets: true,
            mapping: true,
            show_entity_names: false,
            mob_heads: true,
            mob_scale: 0.75,
            map_opacity: 1.0,
            show_time: false,
            show_weather: false,
            grid: false,
            auto_remove_death: true,
            create_death_waypoints: true,
            scan_radius: 8,
        }
    }
}

/// Kinds of creature on the maps.
pub mod kind {
    pub const MONSTER: u8 = 0;
    pub const ANIMAL: u8 = 1;
    pub const PLAYER: u8 = 2;
    pub const VILLAGER: u8 = 3;
    pub const PET: u8 = 4;
}

impl Config {
    /// Whether creatures of this kind are shown.
    pub fn shows(&self, k: u8) -> bool {
        match k {
            kind::MONSTER => self.show_mobs,
            kind::PLAYER => self.show_players,
            kind::VILLAGER => self.show_villagers,
            kind::PET => self.show_pets,
            _ => self.show_animals,
        }
    }

    pub fn preset(&self) -> &Preset {
        &self.presets[usize::from(self.preset.min(1))]
    }
    pub fn preset_mut(&mut self) -> &mut Preset {
        &mut self.presets[usize::from(self.preset.min(1))]
    }

    pub fn encode(&self) -> String {
        let mut s = String::new();
        let mut put = |k: &str, v: String| s.push_str(&format!("{k}={v}\n"));
        put("minimap", self.minimap.to_string());
        put("preset", self.preset.to_string());
        put("map_type", self.map_type.name().to_owned());
        put("auto_map_type", self.auto_map_type.to_string());
        put("show_waypoints", self.show_waypoints.to_string());
        put("show_world_waypoints", self.show_world_waypoints.to_string());
        put("show_mobs", self.show_mobs.to_string());
        put("show_players", self.show_players.to_string());
        put("show_villagers", self.show_villagers.to_string());
        put("show_animals", self.show_animals.to_string());
        put("show_pets", self.show_pets.to_string());
        put("mapping", self.mapping.to_string());
        put("show_entity_names", self.show_entity_names.to_string());
        put("mob_heads", self.mob_heads.to_string());
        put("mob_scale", self.mob_scale.to_string());
        put("map_opacity", self.map_opacity.to_string());
        put("show_time", self.show_time.to_string());
        put("show_weather", self.show_weather.to_string());
        put("grid", self.grid.to_string());
        put("auto_remove_death", self.auto_remove_death.to_string());
        put("create_death_waypoints", self.create_death_waypoints.to_string());
        put("scan_radius", self.scan_radius.to_string());
        for (i, p) in self.presets.iter().enumerate() {
            put(&format!("p{i}.corner"), corner_name(p.corner).to_owned());
            put(&format!("p{i}.shape"), if p.shape == Shape::Circle { "circle" } else { "square" }.to_owned());
            put(&format!("p{i}.size"), p.size.to_string());
            put(&format!("p{i}.zoom"), p.zoom.to_string());
            put(&format!("p{i}.rotate"), p.rotate.to_string());
            put(&format!("p{i}.show_coords"), p.show_coords.to_string());
            put(&format!("p{i}.show_biome"), p.show_biome.to_string());
            put(&format!("p{i}.show_compass"), p.show_compass.to_string());
        }
        s
    }

    pub fn decode(text: &str) -> Self {
        let mut c = Config::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let b = v == "true";
            match k {
                "minimap" => c.minimap = b,
                "preset" => c.preset = v.parse().unwrap_or(0).min(1),
                "map_type" => c.map_type = MapType::parse(v).unwrap_or(MapType::Day),
                "auto_map_type" => c.auto_map_type = b,
                "show_waypoints" => c.show_waypoints = b,
                "show_world_waypoints" => c.show_world_waypoints = b,
                "show_mobs" => c.show_mobs = b,
                "show_players" => c.show_players = b,
                "show_villagers" => c.show_villagers = b,
                "show_animals" => c.show_animals = b,
                "show_pets" => c.show_pets = b,
                "mapping" => c.mapping = b,
                "show_entity_names" => c.show_entity_names = b,
                "mob_heads" => c.mob_heads = b,
                "mob_scale" => c.mob_scale = v.parse().unwrap_or(0.75f32).clamp(0.5, 1.5),
                "map_opacity" => c.map_opacity = v.parse().unwrap_or(1.0f32).clamp(0.2, 1.0),
                "show_time" => c.show_time = b,
                "show_weather" => c.show_weather = b,
                "grid" => c.grid = b,
                "auto_remove_death" => c.auto_remove_death = b,
                "create_death_waypoints" => c.create_death_waypoints = b,
                "scan_radius" => c.scan_radius = v.parse().unwrap_or(8).clamp(2, 16),
                _ => {
                    let Some((p, field)) = k.split_once('.') else { continue };
                    let i = match p {
                        "p0" => 0,
                        "p1" => 1,
                        _ => continue,
                    };
                    let pr = &mut c.presets[i];
                    match field {
                        "corner" => pr.corner = parse_corner(v).unwrap_or(pr.corner),
                        "shape" => pr.shape = if v == "circle" { Shape::Circle } else { Shape::Square },
                        "size" => pr.size = v.parse().unwrap_or(pr.size).clamp(48.0, 256.0),
                        "zoom" => pr.zoom = v.parse().unwrap_or(pr.zoom).min(ZOOMS.len() - 1),
                        "rotate" => pr.rotate = b,
                        "show_coords" => pr.show_coords = b,
                        "show_biome" => pr.show_biome = b,
                        "show_compass" => pr.show_compass = b,
                        _ => {}
                    }
                }
            }
        }
        c
    }
}

pub fn corner_name(c: Corner) -> &'static str {
    match c {
        Corner::TopRight => "top_right",
        Corner::TopLeft => "top_left",
        Corner::BottomRight => "bottom_right",
        Corner::BottomLeft => "bottom_left",
        Corner::TopCenter => "top_center",
    }
}

fn parse_corner(s: &str) -> Option<Corner> {
    Some(match s {
        "top_right" => Corner::TopRight,
        "top_left" => Corner::TopLeft,
        "bottom_right" => Corner::BottomRight,
        "bottom_left" => Corner::BottomLeft,
        "top_center" => Corner::TopCenter,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut c = Config::default();
        c.preset = 1;
        c.map_type = MapType::Cave;
        c.presets[1].corner = Corner::BottomLeft;
        assert_eq!(Config::decode(&c.encode()), c);
    }
}
