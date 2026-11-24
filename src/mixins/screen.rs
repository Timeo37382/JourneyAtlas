fn jm_screen(game: &crate::game::Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    crate::game::journeyatlas_bridge::screen(game, batch);
}
