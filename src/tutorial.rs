//! The first-visit tutorial, once per world: a card that walks through the minimap, the
//! fullscreen map, its toolbars, waypoints and options. Steps advance when the player does what
//! they ask, or with "Next" on the fullscreen map; "Skip" ends it. Progress is saved per world.

use crate::draw::{Canvas, Measure, Rect};
use crate::i18n::{t, tf};

pub const STEPS: u8 = 6;

/// Something the player did that a step waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    MapOpened,
    WaypointOnMap,
    WaypointInWorld,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tutorial {
    /// The current step (1..=STEPS), none once done or skipped.
    pub step: Option<u8>,
    /// Frame the current step began on.
    pub since: u64,
    pub dirty: bool,
}

impl Tutorial {
    /// Reads the saved progress: a step number, or `done`. A world never seen starts at step 1.
    pub fn load(saved: Option<&str>) -> Self {
        let step = match saved.map(str::trim) {
            Some("done") => None,
            Some(n) => n.parse().ok().filter(|s| (1..=STEPS).contains(s)).or(Some(1)),
            None => Some(1),
        };
        Tutorial { step, since: 0, dirty: saved.is_none() }
    }

    pub fn save(&self) -> String {
        self.step.map_or_else(|| "done".to_owned(), |s| s.to_string())
    }

    fn go(&mut self, step: Option<u8>, frame: u64) {
        self.step = step.filter(|s| *s <= STEPS);
        self.since = frame;
        self.dirty = true;
    }

    pub fn event(&mut self, e: Event, frame: u64) {
        match (self.step, e) {
            (Some(1), Event::MapOpened) => self.go(Some(2), frame),
            (Some(2..=4), Event::WaypointOnMap) => self.go(Some(5), frame),
            (Some(5), Event::WaypointInWorld) => self.go(Some(6), frame),
            _ => {}
        }
    }

    pub fn next(&mut self, frame: u64) {
        let s = self.step.map(|s| s + 1);
        self.go(s, frame);
    }

    pub fn skip(&mut self, frame: u64) {
        self.go(None, frame);
    }

    pub fn restart(&mut self, frame: u64) {
        self.go(Some(1), frame);
    }

    /// The last step closes itself after about ten seconds.
    pub fn tick(&mut self, frame: u64) {
        if self.step == Some(STEPS) && frame.saturating_sub(self.since) > 600 {
            self.go(None, frame);
        }
    }
}

/// Splits `text` into lines no wider than `width`.
pub fn wrap(m: &dyn Measure, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if m.text_width(&candidate) > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = word.to_owned();
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// What the card on the fullscreen map offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardButton {
    Next,
    Skip,
}

/// Draws the card for `step` with its top centre at `(cx, y)`. On the fullscreen map
/// (`buttons`), it has "Next" (or "Done") and "Skip"; the rects come back for clicks.
pub fn card(c: &mut Canvas, m: &dyn Measure, step: u8, cx: f32, y: f32, map_open: bool, buttons: bool, cursor: (f32, f32)) -> Vec<(Rect, CardButton)> {
    let w = 240.0;
    let x = cx - w / 2.0;
    // On the map, a step that needs the world says so; off the map, a map step asks to open it.
    let body_key = match (step, map_open) {
        (2..=4, false) => "tuto_reopen",
        (5, true) => "tuto_5_close",
        _ => ["tuto_1", "tuto_2", "tuto_3", "tuto_4", "tuto_5", "tuto_6"][usize::from(step.clamp(1, STEPS) - 1)],
    };
    let lines = wrap(m, &t(body_key), w - 16.0);
    let h = 22.0 + lines.len() as f32 * 10.0 + if buttons { 22.0 } else { 6.0 };
    c.fill(x + 1.0, y, w - 2.0, h, 0xEC0E_1430);
    c.fill(x, y + 1.0, w, h - 2.0, 0xEC0E_1430);
    c.fill(x, y, w, 1.0, 0xFF4C_6EF5);
    c.text(x + 8.0, y + 6.0, tf("tuto_title", &[&step, &STEPS]), 0xFF8F_B4FF);
    // Progress dots.
    for i in 0..STEPS {
        let col = if i < step { 0xFF4C_6EF5 } else { 0xFF3A_3E46 };
        c.fill(x + w - 8.0 - f32::from(STEPS - i) * 7.0, y + 8.0, 5.0, 5.0, col);
    }
    for (i, l) in lines.iter().enumerate() {
        c.text(x + 8.0, y + 20.0 + i as f32 * 10.0, l.as_str(), 0xFFE8_ECF4);
    }
    let mut out = Vec::new();
    if buttons {
        let by = y + h - 18.0;
        let next_label = t(if step >= STEPS { "tuto_finish" } else { "tuto_next" });
        for (label, button, right) in [(t("tuto_skip"), CardButton::Skip, false), (next_label, CardButton::Next, true)] {
            let bw = m.text_width(&label) + 14.0;
            let bx = if right { x + w - 8.0 - bw } else { x + 8.0 };
            let r = Rect { x: bx, y: by, w: bw, h: 13.0 };
            let hover = r.contains(cursor.0, cursor.1);
            let bg = match (right, hover) {
                (true, true) => 0xFF6A_88FF,
                (true, false) => 0xFF4C_6EF5,
                (false, true) => 0xFF3A_4FB0,
                (false, false) => 0xFF24_3170,
            };
            c.fill(r.x, r.y, r.w, r.h, bg);
            c.text(r.x + 7.0, r.y + 3.0, label, 0xFFFF_FFFF);
            out.push((r, button));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::Approx;

    #[test]
    fn walks_through() {
        let mut t = Tutorial::load(None);
        assert_eq!(t.step, Some(1));
        t.event(Event::WaypointInWorld, 1);
        assert_eq!(t.step, Some(1));
        t.event(Event::MapOpened, 2);
        assert_eq!(t.step, Some(2));
        t.next(3);
        t.next(4);
        t.event(Event::WaypointOnMap, 5);
        assert_eq!(t.step, Some(5));
        t.event(Event::WaypointInWorld, 6);
        assert_eq!(t.step, Some(6));
        t.tick(700);
        assert_eq!(t.step, None);
        assert_eq!(Tutorial::load(Some(&t.save())).step, None);
    }

    #[test]
    fn saved_progress_and_card() {
        assert_eq!(Tutorial::load(Some("3")).step, Some(3));
        assert_eq!(Tutorial::load(Some("done")).step, None);
        let mut c = Canvas::default();
        let b = card(&mut c, &Approx, 2, 300.0, 100.0, true, true, (0.0, 0.0));
        assert_eq!(b.len(), 2);
        assert!(wrap(&Approx, "aaa bbb ccc", 40.0).len() >= 2);
    }
}
