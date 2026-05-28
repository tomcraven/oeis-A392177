use std::env;
use std::path::PathBuf;

use red_black_knights::discover::{
    self, DEFAULT_CELL_PIXEL_SCALE, DEFAULT_TARGET_INDEX, write_multiscale_boards,
    write_run_outputs,
};
use red_black_knights::model::GameDefinition;

fn main() {
    if let Err(e) = run() {
        eprintln!("discover_reference: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut out_dir = PathBuf::from(".discover/reference/knight_2_pairwise");
    let mut target_index = env_u32("DISCOVER_TARGET_INDEX", DEFAULT_TARGET_INDEX);
    let mut cell_scale = env_u32("DISCOVER_CELL_SCALE", DEFAULT_CELL_PIXEL_SCALE);
    let mut preset = String::from("knight_2_pairwise");

    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(args.get(i).ok_or("--out requires a path")?);
            }
            "--target-index" => {
                i += 1;
                target_index = args
                    .get(i)
                    .ok_or("--target-index requires a value")?
                    .parse()?;
            }
            "--cell-scale" => {
                i += 1;
                cell_scale = args
                    .get(i)
                    .ok_or("--cell-scale requires a value")?
                    .parse()?;
            }
            "--preset" => {
                i += 1;
                preset = args.get(i).ok_or("--preset requires a value")?.clone();
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
        i += 1;
    }

    let def = preset_definition(&preset)?;
    let (config, def, sim, meta) = discover::run_known_game(&def, target_index);
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir)?;
    }
    write_run_outputs(&out_dir, &config, &def, &sim, &meta, cell_scale)?;
    write_multiscale_boards(&out_dir, &def, &sim, meta.bounds.into(), cell_scale)?;

    println!(
        "reference\t{}\tpreset\t{preset}\ttarget_index\t{target_index}\tgrid\t{}x{}\tsettled\t{}",
        out_dir.display(),
        meta.grid_cells[0],
        meta.grid_cells[1],
        meta.settled
    );
    Ok(())
}

fn preset_definition(name: &str) -> Result<GameDefinition, Box<dyn std::error::Error>> {
    match name {
        "knight_2_pairwise" => Ok(GameDefinition::knight_2_pairwise()),
        other => Err(format!("unknown preset {other:?} (only knight_2_pairwise for now)").into()),
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

fn print_help() {
    eprintln!(
        "Usage: discover_reference [--out DIR] [--preset NAME] [--target-index N] [--cell-scale N]

Writes config.toml, meta.toml, board.png (full bounds), plus scale_center/mid/full.png zoom ladder."
    );
}
