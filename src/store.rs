//! Tiles on disk: `<root>/<world>/<dimension>/<layer>/r.<rx>.<rz>.bin`, loaded the first time a
//! region is asked for and written back while dirty.

use crate::tiles::{Cell, Layer, Region, TileStore, region_of};
use std::collections::HashSet;
use std::path::PathBuf;

pub fn safe_token(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '~' })
        .collect();
    if t.is_empty() { "default".to_owned() } else { t }
}

#[derive(Default)]
pub struct MapStore {
    pub tiles: TileStore,
    /// The world's folder; none before a world is open.
    pub root: Option<PathBuf>,
    /// Regions looked for on disk and not found.
    missing: HashSet<(String, Layer, i32, i32)>,
    /// Sum of every change, so renderers know to rebuild.
    pub revision: u64,
}

impl MapStore {
    pub fn open(&mut self, root: PathBuf) {
        self.flush();
        self.tiles.clear();
        self.missing.clear();
        self.root = Some(root);
        self.revision += 1;
    }

    pub fn close(&mut self) {
        self.flush();
        self.tiles.clear();
        self.missing.clear();
        self.root = None;
        self.revision += 1;
    }

    fn path(&self, dim: &str, layer: Layer, rx: i32, rz: i32) -> Option<PathBuf> {
        Some(
            self.root
                .as_ref()?
                .join(safe_token(dim))
                .join(layer.token())
                .join(format!("r.{rx}.{rz}.bin")),
        )
    }

    /// Makes sure a region is in memory if it exists on disk.
    pub fn ensure(&mut self, dim: &str, layer: Layer, rx: i32, rz: i32) {
        let key = (dim.to_owned(), layer, rx, rz);
        if self.tiles.regions.contains_key(&key) || self.missing.contains(&key) {
            return;
        }
        let loaded = self
            .path(dim, layer, rx, rz)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| Region::decode(&b));
        match loaded {
            Some(r) => {
                self.tiles.regions.insert(key, r);
                self.revision += 1;
            }
            None => {
                self.missing.insert(key);
            }
        }
    }

    pub fn get(&mut self, dim: &str, layer: Layer, x: i32, z: i32) -> Option<Cell> {
        let (rx, rz) = region_of(x, z);
        self.ensure(dim, layer, rx, rz);
        self.tiles.get(dim, layer, x, z)
    }

    pub fn set(&mut self, dim: &str, layer: Layer, x: i32, z: i32, cell: Cell) {
        let (rx, rz) = region_of(x, z);
        self.ensure(dim, layer, rx, rz);
        self.missing.remove(&(dim.to_owned(), layer, rx, rz));
        let r = self.tiles.region_mut(dim, layer, rx, rz);
        let before = r.revision;
        r.set(x, z, cell);
        if r.revision != before {
            self.revision += 1;
        }
    }

    /// Forgets a dimension's whole map, in memory and on disk.
    pub fn delete_dim(&mut self, dim: &str) {
        self.tiles.regions.retain(|(d, ..), _| d != dim);
        self.missing.retain(|(d, ..)| d != dim);
        if let Some(root) = &self.root {
            let _ = std::fs::remove_dir_all(root.join(safe_token(dim)));
        }
        self.revision += 1;
    }

    /// Writes every dirty region. Returns how many were written.
    pub fn flush(&mut self) -> usize {
        let Some(root) = self.root.clone() else { return 0 };
        let mut n = 0;
        for ((dim, layer, rx, rz), region) in self.tiles.regions.iter_mut() {
            if !region.dirty {
                continue;
            }
            let dir = root.join(safe_token(dim)).join(layer.token());
            if std::fs::create_dir_all(&dir).is_ok()
                && std::fs::write(dir.join(format!("r.{rx}.{rz}.bin")), region.encode()).is_ok()
            {
                region.dirty = false;
                n += 1;
            }
        }
        n
    }

    /// Drops regions far from the player once they are saved, to bound memory.
    pub fn evict_far(&mut self, dim: &str, x: i32, z: i32, keep_regions: i32) {
        let (px, pz) = region_of(x, z);
        let before = self.tiles.regions.len();
        self.tiles.regions.retain(|(d, _, rx, rz), r| {
            r.dirty || (d == dim && (rx - px).abs() <= keep_regions && (rz - pz).abs() <= keep_regions)
        });
        if self.tiles.regions.len() != before {
            self.missing.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_reload() {
        let dir = std::env::temp_dir().join(format!("jmrs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = MapStore::default();
        s.open(dir.clone());
        let c = Cell { rgb: [5, 6, 7], y: 64, flags: Cell::EXPLORED, ..Default::default() };
        s.set("minecraft:overworld", Layer::Surface, -700, 20, c);
        assert_eq!(s.flush(), 1);
        let mut t = MapStore::default();
        t.open(dir.clone());
        assert_eq!(t.get("minecraft:overworld", Layer::Surface, -700, 20), Some(c));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
