fn jm_press(game: &crate::game::Game<'_>, button: i32, canceled: &mut bool) {
    crate::game::journeyatlas_bridge::press(game, button, canceled);
}
