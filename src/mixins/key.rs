fn jm_key(game: &mut crate::game::Game<'_>, key: &str, action: mod_api::KeyAction, modifiers: u32) {
    crate::game::journeyatlas_bridge::input_key(game, key, action, modifiers);
}
