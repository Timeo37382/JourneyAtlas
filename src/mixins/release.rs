fn jm_release(game: &crate::game::Game<'_>, canceled: &mut bool) {
    crate::game::journeyatlas_bridge::release(game, canceled);
}
