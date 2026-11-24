fn jm_drag(game: &crate::game::Game<'_>, delta: [f32; 2], button: i32, canceled: &mut bool) {
    crate::game::journeyatlas_bridge::drag(game, delta, button, canceled);
}
