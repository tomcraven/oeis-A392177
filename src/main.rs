use bevy::prelude::*;
use red_black_knights::game_app;

fn main() {
    let mut app = App::new();
    game_app::configure_app(&mut app);
    app.run();
}
