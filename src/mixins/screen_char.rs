fn jm_screen_char(game: &crate::game::Game<'_>, c: char, canceled: &mut bool) {
    crate::game::journeyatlas_bridge::screen_char(game, c, canceled);
}
