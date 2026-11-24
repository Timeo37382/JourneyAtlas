//! Turning stored cells into map pixels: relief from neighbouring heights, water depth,
//! and the day / night / cave looks map mods offers.

use crate::tiles::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapType {
    Day,
    Night,
    Cave,
    Topo,
}

impl MapType {
    pub fn next(self) -> Self {
        match self {
            MapType::Day => MapType::Night,
            MapType::Night => MapType::Cave,
            MapType::Cave => MapType::Topo,
            MapType::Topo => MapType::Day,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            MapType::Day => "day",
            MapType::Night => "night",
            MapType::Cave => "cave",
            MapType::Topo => "topo",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "day" => Some(MapType::Day),
            "night" => Some(MapType::Night),
            "cave" => Some(MapType::Cave),
            "topo" => Some(MapType::Topo),
            _ => None,
        }
    }
}

fn scale(rgb: [u8; 3], f: f32) -> [u8; 3] {
    rgb.map(|c| (f32::from(c) * f).clamp(0.0, 255.0) as u8)
}

/// Colour of one cell as `0xAARRGGBB`. `west` and `north` are the heights of the neighbours,
/// light comes from the north-west as in the "bevel" relief.
pub fn shade(cell: Cell, west: Option<i16>, north: Option<i16>, ty: MapType) -> u32 {
    if !cell.explored() {
        return 0;
    }
    if ty == MapType::Topo {
        return topo(cell, west, north);
    }
    let mut rgb = cell.rgb;
    if cell.is_water() {
        // Deeper water is darker.
        let f = 1.0 - (f32::from(cell.water).min(24.0) / 24.0) * 0.35;
        rgb = scale(rgb, f);
    } else {
        let mut slope = 0i32;
        if let Some(w) = west {
            slope += i32::from(cell.y - w);
        }
        if let Some(n) = north {
            slope += i32::from(cell.y - n);
        }
        let f = (1.0 + slope.clamp(-6, 6) as f32 * 0.06).clamp(0.6, 1.3);
        rgb = scale(rgb, f);
    }
    match ty {
        MapType::Day => {}
        MapType::Night => {
            // Dark everywhere except where a block light shines, like the night map.
            let lit = f32::from(cell.light.min(15)) / 15.0;
            let f = 0.22 + 0.78 * lit;
            rgb = scale(rgb, f);
            if lit < 0.5 {
                // A slight blue cast in the dark.
                rgb[2] = rgb[2].saturating_add(10);
            }
        }
        MapType::Topo => {}
        MapType::Cave => {
            // Caves stay readable, torches stand out.
            let lit = f32::from(cell.light.min(15)) / 15.0;
            rgb = scale(rgb, 0.55 + 0.45 * lit);
        }
    }
    0xFF00_0000 | (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2])
}

/// Topographic look: colour by altitude band, with a contour line every 8 blocks.
fn topo(cell: Cell, west: Option<i16>, north: Option<i16>) -> u32 {
    let y = i32::from(cell.y);
    let rgb: [u8; 3] = if cell.is_water() {
        let d = f32::from(cell.water.min(24)) / 24.0;
        [(90.0 - 60.0 * d) as u8, (150.0 - 70.0 * d) as u8, (215.0 - 40.0 * d) as u8]
    } else {
        // Low land green, hills tan, mountains grey, peaks white.
        let stops: [(i32, [u8; 3]); 6] =
            [(-64, [40, 70, 40]), (62, [120, 170, 90]), (90, [190, 200, 120]), (120, [180, 150, 100]), (160, [150, 140, 135]), (220, [245, 245, 245])];
        let mut c = stops[stops.len() - 1].1;
        for w in stops.windows(2) {
            let ((y0, c0), (y1, c1)) = (w[0], w[1]);
            if y <= y1 {
                let t = ((y - y0) as f32 / (y1 - y0) as f32).clamp(0.0, 1.0);
                c = [0, 1, 2].map(|i| (f32::from(c0[i]) + (f32::from(c1[i]) - f32::from(c0[i])) * t) as u8);
                break;
            }
        }
        c
    };
    let band = |h: i16| i32::from(h).div_euclid(8);
    let contour = !cell.is_water() && [west, north].into_iter().flatten().any(|h| band(h) != band(cell.y));
    let rgb = if contour { scale(rgb, 0.6) } else { rgb };
    0xFF00_0000 | (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexplored_is_transparent() {
        assert_eq!(shade(Cell::default(), None, None, MapType::Day), 0);
    }

    #[test]
    fn higher_than_neighbours_is_brighter() {
        let c = Cell { rgb: [100, 100, 100], y: 70, flags: Cell::EXPLORED, ..Default::default() };
        let up = shade(c, Some(60), Some(60), MapType::Day) & 0xff;
        let down = shade(c, Some(80), Some(80), MapType::Day) & 0xff;
        assert!(up > 100 && down < 100);
    }
}
