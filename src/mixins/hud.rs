fn jm_hud(game: &crate::game::Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    crate::game::journeyatlas_bridge::hud(game, batch);
}
