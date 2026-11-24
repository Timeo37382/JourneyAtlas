//! Map tile storage, classic style: the world is cut into 512x512-block regions, one per
//! (dimension, layer). Each column keeps its colour and the height of its top block, so relief
//! shading and night tinting are computed at draw time, not at scan time.

use std::collections::HashMap;

pub const REGION_SIZE: i32 = 512;
pub const REGION_AREA: usize = (REGION_SIZE * REGION_SIZE) as usize;

/// One mapped column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub rgb: [u8; 3],
    /// Height of the surface block (or water surface).
    pub y: i16,
    /// Water depth above the floor, 0 when dry.
    pub water: u8,
    /// Block light at the surface (0..15), used by the night map.
    pub light: u8,
    pub flags: u8,
}

impl Cell {
    pub const EXPLORED: u8 = 1;
    pub const WATER: u8 = 2;

    pub fn explored(&self) -> bool {
        self.flags & Self::EXPLORED != 0
    }
    pub fn is_water(&self) -> bool {
        self.flags & Self::WATER != 0
    }
}

/// Which picture of the world a region holds. Cave layers are keyed by their 16-block slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    Surface,
    Cave(i32),
}

impl Layer {
    pub fn token(self) -> String {
        match self {
            Layer::Surface => "surface".to_owned(),
            Layer::Cave(s) => format!("cave{s}"),
        }
    }
}

pub struct Region {
    pub cells: Vec<Cell>,
    pub dirty: bool,
    /// Bumped on every change, so a renderer can cache what it built from this region.
    pub revision: u32,
}

impl Default for Region {
    fn default() -> Self {
        Self { cells: vec![Cell::default(); REGION_AREA], dirty: false, revision: 0 }
    }
}

pub fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

pub fn region_of(x: i32, z: i32) -> (i32, i32) {
    (floor_div(x, REGION_SIZE), floor_div(z, REGION_SIZE))
}

fn index_in_region(x: i32, z: i32) -> usize {
    let lx = x.rem_euclid(REGION_SIZE) as usize;
    let lz = z.rem_euclid(REGION_SIZE) as usize;
    lz * REGION_SIZE as usize + lx
}

impl Region {
    pub fn get(&self, x: i32, z: i32) -> Cell {
        self.cells[index_in_region(x, z)]
    }

    pub fn set(&mut self, x: i32, z: i32, cell: Cell) {
        let i = index_in_region(x, z);
        if self.cells[i] != cell {
            self.cells[i] = cell;
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Run-length encoding of the 7-byte cells: `[run:u16][cell:7]...` behind a magic and version.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 * 1024);
        out.extend_from_slice(b"JMRS");
        out.push(1);
        let mut i = 0;
        while i < self.cells.len() {
            let c = self.cells[i];
            let mut run = 1usize;
            while i + run < self.cells.len() && run < u16::MAX as usize && self.cells[i + run] == c {
                run += 1;
            }
            out.extend_from_slice(&(run as u16).to_le_bytes());
            out.extend_from_slice(&c.rgb);
            out.extend_from_slice(&c.y.to_le_bytes());
            out.push(c.water);
            out.push((c.light & 0x0f) | (c.flags << 4));
            i += run;
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Region> {
        if bytes.len() < 5 || &bytes[..4] != b"JMRS" || bytes[4] != 1 {
            return None;
        }
        let mut cells = Vec::with_capacity(REGION_AREA);
        let mut p = 5;
        while p + 9 <= bytes.len() && cells.len() < REGION_AREA {
            let run = u16::from_le_bytes([bytes[p], bytes[p + 1]]) as usize;
            let packed = bytes[p + 8];
            let c = Cell {
                rgb: [bytes[p + 2], bytes[p + 3], bytes[p + 4]],
                y: i16::from_le_bytes([bytes[p + 5], bytes[p + 6]]),
                water: bytes[p + 7],
                light: packed & 0x0f,
                flags: packed >> 4,
            };
            for _ in 0..run.min(REGION_AREA - cells.len()) {
                cells.push(c);
            }
            p += 9;
        }
        if cells.len() != REGION_AREA {
            return None;
        }
        Some(Region { cells, dirty: false, revision: 1 })
    }
}

/// All loaded regions of one world, by (dimension, layer, rx, rz).
#[derive(Default)]
pub struct TileStore {
    pub regions: HashMap<(String, Layer, i32, i32), Region>,
}

impl TileStore {
    pub fn get(&self, dim: &str, layer: Layer, x: i32, z: i32) -> Option<Cell> {
        let (rx, rz) = region_of(x, z);
        self.regions
            .get(&(dim.to_owned(), layer, rx, rz))
            .map(|r| r.get(x, z))
    }

    pub fn region_mut(&mut self, dim: &str, layer: Layer, rx: i32, rz: i32) -> &mut Region {
        self.regions.entry((dim.to_owned(), layer, rx, rz)).or_default()
    }

    pub fn set(&mut self, dim: &str, layer: Layer, x: i32, z: i32, cell: Cell) {
        let (rx, rz) = region_of(x, z);
        self.region_mut(dim, layer, rx, rz).set(x, z, cell);
    }

    pub fn clear(&mut self) {
        self.regions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_round_trip() {
        let mut r = Region::default();
        r.set(3, 4, Cell { rgb: [1, 2, 3], y: -12, water: 4, light: 9, flags: Cell::EXPLORED | Cell::WATER });
        r.set(-1, -1, Cell { rgb: [9, 9, 9], y: 300, water: 0, light: 15, flags: Cell::EXPLORED });
        let back = Region::decode(&r.encode()).unwrap();
        assert_eq!(back.get(3, 4), r.get(3, 4));
        assert_eq!(back.get(-1, -1), r.get(-1, -1));
        assert_eq!(back.get(100, 100), Cell::default());
    }

    #[test]
    fn negative_coords_map_to_regions() {
        assert_eq!(region_of(-1, 0), (-1, 0));
        assert_eq!(region_of(511, 512), (0, 1));
    }
}
