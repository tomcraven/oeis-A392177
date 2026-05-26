use std::env;
use std::path::PathBuf;

use red_black_knights::discover::{self, DEFAULT_CELL_PIXEL_SCALE};

fn main() {
    if let Err(e) = run() {
        eprintln!("discover_rerender: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    let mut cell_scale = env::var("DISCOVER_CELL_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CELL_PIXEL_SCALE);
    let mut dirs = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--cell-scale" => {
                i += 1;
                cell_scale = args
                    .get(i)
                    .ok_or("--cell-scale requires a value")?
                    .parse()?;
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => dirs.push(PathBuf::from(&args[i])),
        }
        i += 1;
    }

    if dirs.is_empty() {
        print_help();
        return Ok(());
    }

    let cell_scale = discover::sanitize_cell_pixel_scale(cell_scale);
    for dir in dirs {
        discover::rerender_saved_run(&dir, cell_scale)?;
        println!("rerendered\t{}\tcell_scale\t{cell_scale}", dir.display());
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "Usage: discover_rerender [--cell-scale N] <run_dir> [<run_dir> ...]

Replays config.toml and writes upscaled board.png (default scale {DEFAULT_CELL_PIXEL_SCALE})."
    );
}
