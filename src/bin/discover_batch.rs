use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use red_black_knights::discover::{
    self, DEFAULT_CELL_PIXEL_SCALE, DEFAULT_TARGET_INDEX, write_multiscale_boards,
    write_run_outputs,
};
use red_black_knights::discover_catalog;

fn main() {
    if let Err(e) = run() {
        eprintln!("discover_batch: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let mut out_dir = PathBuf::from(".discover/pending");
    let mut start_iteration = env_u64("DISCOVER_START", 0);
    let mut count = env_usize("DISCOVER_COUNT", 1);
    let mut turns = env_usize("DISCOVER_TURNS", 0);
    let mut target_index = env_u32("DISCOVER_TARGET_INDEX", DEFAULT_TARGET_INDEX);
    let mut cell_scale = env_u32("DISCOVER_CELL_SCALE", DEFAULT_CELL_PIXEL_SCALE);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(args.get(i).ok_or("--out requires a path")?);
            }
            "--start" => {
                i += 1;
                start_iteration = args.get(i).ok_or("--start requires a value")?.parse()?;
            }
            "--count" => {
                i += 1;
                count = args.get(i).ok_or("--count requires a value")?.parse()?;
            }
            "--turns" => {
                i += 1;
                turns = args.get(i).ok_or("--turns requires a value")?.parse()?;
            }
            "--cell-scale" => {
                i += 1;
                cell_scale = args
                    .get(i)
                    .ok_or("--cell-scale requires a value")?
                    .parse()?;
            }
            "--target-index" => {
                i += 1;
                target_index = args
                    .get(i)
                    .ok_or("--target-index requires a value")?
                    .parse()?;
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
        i += 1;
    }

    if count == 0 {
        return Err("--count must be at least 1".into());
    }

    let cell_scale = discover::sanitize_cell_pixel_scale(cell_scale);

    fs::create_dir_all(&out_dir)?;

    println!(
        "mode\tcatalog\tcatalog_len\t{}\tstart\t{}\tcount\t{}\ttarget_index\t{}\tcell_scale\t{}",
        discover_catalog::catalog_len(),
        start_iteration,
        count,
        target_index,
        cell_scale
    );

    for offset in 0..count {
        let iteration = start_iteration + offset as u64;
        let catalog_index = discover_catalog::recipe_for_iteration(iteration);
        let run_dir = out_dir.join(format!("run_{iteration:05}"));
        if run_dir.exists() {
            fs::remove_dir_all(&run_dir)?;
        }
        emit_run(&run_dir, catalog_index, target_index, turns, cell_scale)?;
        let recipe = discover_catalog::recipe_meta(catalog_index).map(|(id, _)| id);
        println!(
            "run\t{}\tcatalog_index\t{}\trecipe\t{}\tplacements\t{}\tgrid\t{}x{}\tsettled\t{}",
            run_dir.display(),
            catalog_index,
            recipe.unwrap_or_default(),
            read_field_usize(&run_dir, "placements")?,
            read_field_u32(&run_dir, "grid_cells", 0)?,
            read_field_u32(&run_dir, "grid_cells", 1)?,
            read_field_bool(&run_dir, "settled")?
        );
    }

    write_batch_manifest(
        &out_dir,
        start_iteration,
        count,
        turns,
        target_index,
        cell_scale,
    )?;
    Ok(())
}

fn emit_run(
    run_dir: &Path,
    catalog_index: usize,
    target_index: u32,
    turns: usize,
    cell_pixel_scale: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let effective_target = if turns > 0 && target_index == 0 {
        0
    } else {
        target_index
    };
    let (config, def, sim, meta) = discover::run_catalog_index(catalog_index, effective_target)
        .ok_or("catalog index out of range")?;
    if turns > 0 && target_index == 0 {
        return Err("fixed-turn mode not supported for catalog batch".into());
    }
    write_run_outputs(run_dir, &config, &def, &sim, &meta, cell_pixel_scale)?;
    write_multiscale_boards(run_dir, &def, &sim, meta.bounds.into(), cell_pixel_scale)?;
    Ok(())
}

fn write_batch_manifest(
    out_dir: &Path,
    start_iteration: u64,
    count: usize,
    turns: usize,
    target_index: u32,
    cell_scale: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = format!(
        "mode = \"catalog\"\ncatalog_len = {}\nstart_iteration = {start_iteration}\ncount = {count}\nturns = {turns}\ntarget_index = {target_index}\ncell_pixel_scale = {cell_scale}\n",
        discover_catalog::catalog_len()
    );
    fs::write(out_dir.join("batch.toml"), manifest)?;
    Ok(())
}

fn read_field_usize(run_dir: &Path, key: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(run_dir.join("meta.toml"))?;
    let prefix = format!("{key} = ");
    Ok(text
        .lines()
        .find_map(|line| line.strip_prefix(&prefix).and_then(|v| v.parse().ok()))
        .unwrap_or(0))
}

fn read_field_u32(
    run_dir: &Path,
    table: &str,
    index: usize,
) -> Result<u32, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(run_dir.join("meta.toml"))?;
    let mut in_table = false;
    let mut values = Vec::new();
    for line in text.lines() {
        if line.trim() == format!("{table} = [") {
            in_table = true;
            continue;
        }
        if in_table {
            if line.trim() == "]" {
                break;
            }
            if let Some(v) = line.trim().trim_end_matches(',').parse().ok() {
                values.push(v);
            }
        }
    }
    Ok(values.get(index).copied().unwrap_or(0))
}

fn read_field_bool(run_dir: &Path, key: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(run_dir.join("meta.toml"))?;
    let prefix = format!("{key} = ");
    Ok(text
        .lines()
        .find_map(|line| line.strip_prefix(&prefix).and_then(|v| v.parse().ok()))
        .unwrap_or(false))
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
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
        "Usage: discover_batch [--out DIR] [--start N] [--count N] [--target-index N] [--cell-scale N]

Sweeps the simple-piece catalog (pairwise, cliques, mixed trios) — not random attack patterns.
Catalog size: {} recipes. Iteration i uses catalog index (i mod catalog_len).

Environment: DISCOVER_START, DISCOVER_COUNT, DISCOVER_TARGET_INDEX (default 4819953), DISCOVER_CELL_SCALE.",
        discover_catalog::catalog_len()
    );
}
