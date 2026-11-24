//! Waypoints: named points with a colour, per dimension, saved as one line each.
//! the death waypoints are waypoints of kind `Death`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Death,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub dim: String,
    /// `0xRRGGBB`.
    pub color: u32,
    pub enabled: bool,
    pub kind: Kind,
}

/// The colours map mods cycles through for new waypoints.
pub const PALETTE: [u32; 12] = [
    0xFF5555, 0x55FF55, 0x5555FF, 0xFFFF55, 0xFF55FF, 0x55FFFF, 0xFFAA00, 0xAA00AA, 0x00AAAA,
    0xAAAAAA, 0xFFFFFF, 0x00AA00,
];

pub fn color_for(index: usize) -> u32 {
    PALETTE[index % PALETTE.len()]
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `v1\tname\tx\ty\tz\tdim\tRRGGBB\tenabled\tkind` per line.
pub fn encode(list: &[Waypoint]) -> String {
    let mut s = String::new();
    for w in list {
        s.push_str(&format!(
            "v1\t{}\t{}\t{}\t{}\t{}\t{:06X}\t{}\t{}\n",
            escape(&w.name),
            w.x,
            w.y,
            w.z,
            escape(&w.dim),
            w.color & 0xFF_FFFF,
            u8::from(w.enabled),
            match w.kind {
                Kind::Normal => "normal",
                Kind::Death => "death",
            }
        ));
    }
    s
}

pub fn decode(text: &str) -> Vec<Waypoint> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() != 9 || f[0] != "v1" {
                return None;
            }
            Some(Waypoint {
                name: unescape(f[1]),
                x: f[2].parse().ok()?,
                y: f[3].parse().ok()?,
                z: f[4].parse().ok()?,
                dim: unescape(f[5]),
                color: u32::from_str_radix(f[6], 16).ok()?,
                enabled: f[7] == "1",
                kind: if f[8] == "death" { Kind::Death } else { Kind::Normal },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let w = vec![Waypoint {
            name: "Base\tmaison".into(),
            x: -10,
            y: 64,
            z: 300,
            dim: "minecraft:overworld".into(),
            color: 0x12ABEF,
            enabled: true,
            kind: Kind::Death,
        }];
        assert_eq!(decode(&encode(&w)), w);
    }
}
