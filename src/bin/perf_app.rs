//! Run scripted camera/scenario perf collection (requires `app_profile`).

use bevy::prelude::*;
use red_black_knights::game_app;
use red_black_knights::perf_harness;

fn main() {
    if !cfg!(feature = "app_profile") {
        eprintln!("perf_app requires --features app_profile");
        std::process::exit(1);
    }
    if perf_harness::perf_scenario_name_from_args().is_none() {
        eprintln!(
            "Usage: perf_app --perf-scenario <name|path.toml>\nBuilt-in: origin_settled, pan_east, zoom_out_catchup, pan_render_stress"
        );
        std::process::exit(1);
    }
    let mut app = App::new();
    game_app::configure_app(&mut app);
    app.run();
}
