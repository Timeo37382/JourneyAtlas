fn jm_screen_key(game: &crate::game::Game<'_>, key: &str, canceled: &mut bool) {
    crate::game::journeyatlas_bridge::screen_key(game, key, canceled);
}
