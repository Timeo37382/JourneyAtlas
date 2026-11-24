fn jm_scroll(game: &crate::game::Game<'_>, scroll: [f64; 2], canceled: &mut bool) {
    crate::game::journeyatlas_bridge::scroll(game, scroll, canceled);
}
