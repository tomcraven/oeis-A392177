use std::fs;
use std::path::Path;

use bevy::prelude::Color;
use image::imageops::{self, FilterType};
use image::{ImageBuffer, RgbaImage};
use rand::Rng;
use rand::SeedableRng;
use rand::prelude::IndexedRandom;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};

use crate::game_snapshot::DiscoverRunConfig;
use crate::model::{PieceId, GameDefinition};
use crate::random_gen::{AttackSymmetry, RandomGenConfig, generate_random_game};
use crate::render::grid_texture_size;
use crate::sim::Simulation;
use crate::spiral::index_to_xy;
use crate::viewport::GridBounds;
use crate::CELL_SIZE;

const EMPTY_RGBA: [u8; 4] = [31, 31, 41, 255];
const BOUNDS_PADDING: i32 = 2;
/// PNG pixels per spiral cell (matches in-game board texel footprint by default).
pub const DEFAULT_CELL_PIXEL_SCALE: u32 = CELL_SIZE as u32;
/// Same order of magnitude as a zoomed-out knight preset in the UI (~4819953).
pub const DEFAULT_TARGET_INDEX: u32 = 4_819_953;
const MAX_CELL_PIXEL_SCALE: u32 = 32;
const MAX_OUTPUT_EDGE_PX: u32 = 8192;

/// Internal: random generator inputs for one batch iteration (not written to disk).
#[derive(Clone, Debug)]
pub struct DiscoverRunSpec {
    pub random_gen: RandomGenConfig,
    pub rng_seed: u64,
    pub turns: usize,
    pub target_index: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoverRunMeta {
    pub placements: usize,
    pub turn_step: usize,
    pub checksum: u64,
    pub bounds: GridBoundsSerde,
    pub target_index: u32,
    pub settled: bool,
    pub max_placement_index: u32,
    #[serde(default)]
    pub catalog_index: usize,
    #[serde(default)]
    pub recipe_id: String,
    #[serde(default)]
    pub recipe_label: String,
    /// Spiral grid size (one logical cell per texel before upscale).
    pub grid_cells: [u32; 2],
    pub cell_pixel_scale: u32,
    /// Output `board.png` dimensions (`grid_cells * cell_pixel_scale`).
    pub image_pixels: [u32; 2],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GridBoundsSerde {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

impl From<GridBounds> for GridBoundsSerde {
    fn from(b: GridBounds) -> Self {
        Self {
            min_x: b.min_x,
            max_x: b.max_x,
            min_y: b.min_y,
            max_y: b.max_y,
        }
    }
}

impl From<GridBoundsSerde> for GridBounds {
    fn from(b: GridBoundsSerde) -> Self {
        Self {
            min_x: b.min_x,
            max_x: b.max_x,
            min_y: b.min_y,
            max_y: b.max_y,
        }
    }
}

/// Sample generator knobs for automated discovery (distinct from UI defaults).
pub fn sample_random_gen_config(rng: &mut impl Rng) -> RandomGenConfig {
    let piece_min = rng.random_range(2u32..=4);
    let piece_max = rng.random_range(piece_min..=7);
    RandomGenConfig {
        piece_count_min: piece_min,
        piece_count_max: piece_max,
        attack_radius_min: rng.random_range(1..=2),
        attack_radius_max: rng.random_range(2..=5),
        pattern_density: rng.random_range(0.18..=0.55),
        attack_symmetry: *AttackSymmetry::ALL
            .choose(rng)
            .unwrap_or(&AttackSymmetry::None),
        identical_pieces: false,
    }
}

pub fn spec_for_iteration(
    base_seed: u64,
    iteration: u64,
    turns: usize,
    target_index: u32,
) -> DiscoverRunSpec {
    let mix = base_seed
        .wrapping_add(iteration)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut rng = StdRng::seed_from_u64(mix);
    let random_gen = sample_random_gen_config(&mut rng);
    let rng_seed = rng.random();
    DiscoverRunSpec {
        random_gen,
        rng_seed,
        turns,
        target_index,
    }
}

fn max_placement_index(placements: &[(u32, PieceId)]) -> u32 {
    placements.iter().map(|&(index, _)| index).max().unwrap_or(0)
}

fn simulate(
    def: &GameDefinition,
    target_index: u32,
    turns: usize,
) -> (Simulation, DiscoverRunMeta) {
    let mut sim = Simulation::new(def);
    if target_index > 0 {
        sim.advance_to_target(def, target_index);
    } else {
        for _ in 0..turns {
            if !sim.step_turn(def) {
                break;
            }
        }
    }
    let settled = target_index == 0 || !sim.needs_work(def, target_index);
    let bounds = bounds_from_placements(&sim.placements, BOUNDS_PADDING);
    let checksum = placement_checksum(&sim.placements);
    let grid_size = grid_texture_size(bounds);
    let meta = DiscoverRunMeta {
        placements: sim.placements.len(),
        turn_step: sim.turn_step,
        checksum,
        bounds: bounds.into(),
        target_index,
        settled,
        max_placement_index: max_placement_index(&sim.placements),
        catalog_index: 0,
        recipe_id: String::new(),
        recipe_label: String::new(),
        grid_cells: [grid_size.x, grid_size.y],
        cell_pixel_scale: DEFAULT_CELL_PIXEL_SCALE,
        image_pixels: [
            grid_size.x * DEFAULT_CELL_PIXEL_SCALE,
            grid_size.y * DEFAULT_CELL_PIXEL_SCALE,
        ],
    };
    (sim, meta)
}

pub fn run_config(config: &DiscoverRunConfig) -> (GameDefinition, Simulation, DiscoverRunMeta) {
    let def = config.to_game_definition();
    let (sim, meta) = simulate(&def, config.target_index, config.turns);
    (def, sim, meta)
}

pub fn run_known_game(
    def: &GameDefinition,
    target_index: u32,
) -> (DiscoverRunConfig, GameDefinition, Simulation, DiscoverRunMeta) {
    let config = DiscoverRunConfig::from_game(def, target_index, 0);
    let (sim, meta) = simulate(def, target_index, 0);
    (config, def.clone(), sim, meta)
}

pub fn run_catalog_index(
    catalog_index: usize,
    target_index: u32,
) -> Option<(DiscoverRunConfig, GameDefinition, Simulation, DiscoverRunMeta)> {
    use crate::discover_catalog::{game_at, recipe_meta};

    let def = game_at(catalog_index)?;
    let (recipe_id, recipe_label) = recipe_meta(catalog_index)?;
    let config = DiscoverRunConfig::from_game(&def, target_index, 0);
    let (sim, mut meta) = simulate(&def, target_index, 0);
    meta.catalog_index = catalog_index;
    meta.recipe_id = recipe_id;
    meta.recipe_label = recipe_label;
    Some((config, def, sim, meta))
}

/// Axis-aligned square centered on the spiral origin.
pub fn square_bounds(half_extent: i32) -> GridBounds {
    GridBounds {
        min_x: -half_extent,
        max_x: half_extent,
        min_y: -half_extent,
        max_y: half_extent,
    }
}

/// Named zoom levels for reviewing multi-scale structure (see preset-discovery skill).
pub const MULTISCALE_HALF_EXTENTS: [(&str, i32); 3] = [
    ("scale_center", 72),
    ("scale_mid", 320),
    ("scale_full", 0),
];

pub fn write_multiscale_boards(
    out_dir: &Path,
    def: &GameDefinition,
    sim: &Simulation,
    full_bounds: GridBounds,
    requested_scale: u32,
) -> std::io::Result<()> {
    fs::create_dir_all(out_dir)?;
    for (label, half) in MULTISCALE_HALF_EXTENTS {
        let bounds = if half > 0 {
            square_bounds(half)
        } else {
            full_bounds
        };
        let grid = grid_texture_size(bounds);
        let scale = fit_output_cell_scale(grid.x, grid.y, requested_scale);
        let path = out_dir.join(format!("{label}.png"));
        write_board_png(def, &sim.occupancy, bounds, scale, &path)?;
    }
    Ok(())
}

pub fn run_random_iteration(
    spec: &DiscoverRunSpec,
) -> (DiscoverRunConfig, GameDefinition, Simulation, DiscoverRunMeta) {
    let mut rng = StdRng::seed_from_u64(spec.rng_seed);
    let def = generate_random_game(&spec.random_gen, &mut rng);
    let config = DiscoverRunConfig::from_game(&def, spec.target_index, spec.turns);
    let (sim, meta) = simulate(&def, config.target_index, config.turns);
    (config, def, sim, meta)
}

/// Reduce upscale when the grid is large so PNGs stay within `MAX_OUTPUT_EDGE_PX`.
pub fn fit_output_cell_scale(grid_w: u32, grid_h: u32, requested: u32) -> u32 {
    let requested = sanitize_cell_pixel_scale(requested);
    let max_grid = grid_w.max(grid_h).max(1);
    let cap = (MAX_OUTPUT_EDGE_PX / max_grid).max(1);
    requested.min(cap)
}

pub fn sanitize_cell_pixel_scale(scale: u32) -> u32 {
    scale.clamp(1, MAX_CELL_PIXEL_SCALE)
}

pub fn write_run_outputs(
    out_dir: &Path,
    config: &DiscoverRunConfig,
    def: &GameDefinition,
    sim: &Simulation,
    meta: &DiscoverRunMeta,
    cell_pixel_scale: u32,
) -> std::io::Result<()> {
    fs::create_dir_all(out_dir)?;
    let bounds: GridBounds = meta.bounds.into();
    let cell_pixel_scale =
        fit_output_cell_scale(meta.grid_cells[0], meta.grid_cells[1], cell_pixel_scale);
    let png_path = out_dir.join("board.png");
    write_board_png(
        def,
        &sim.occupancy,
        bounds,
        cell_pixel_scale,
        &png_path,
    )?;

    let mut meta = meta.clone();
    meta.cell_pixel_scale = cell_pixel_scale;
    meta.image_pixels = [
        meta.grid_cells[0] * cell_pixel_scale,
        meta.grid_cells[1] * cell_pixel_scale,
    ];

    let config_text = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(out_dir.join("config.toml"), config_text)?;

    let meta_text = toml::to_string_pretty(&meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(out_dir.join("meta.toml"), meta_text)?;

    Ok(())
}

/// Re-simulate from `config.toml` and rewrite `board.png` / `meta.toml` (e.g. after changing upscale).
pub fn rerender_saved_run(run_dir: &Path, cell_pixel_scale: u32) -> std::io::Result<()> {
    let text = fs::read_to_string(run_dir.join("config.toml"))?;
    let config: DiscoverRunConfig = toml::from_str(&text).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })?;
    let (def, sim, meta) = run_config(&config);
    write_run_outputs(run_dir, &config, &def, &sim, &meta, cell_pixel_scale)
}

pub fn bounds_from_placements(placements: &[(u32, PieceId)], padding: i32) -> GridBounds {
    if placements.is_empty() {
        return GridBounds {
            min_x: -padding,
            max_x: padding,
            min_y: -padding,
            max_y: padding,
        };
    }
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for &(index, _) in placements {
        let (x, y) = index_to_xy(index);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    GridBounds {
        min_x: min_x - padding,
        max_x: max_x + padding,
        min_y: min_y - padding,
        max_y: max_y + padding,
    }
}

pub fn encode_board_png(
    def: &GameDefinition,
    occupancy: &crate::sim::OccupancyGrid,
    bounds: GridBounds,
    cell_pixel_scale: u32,
) -> std::io::Result<Vec<u8>> {
    let out = raster_board_rgba(def, occupancy, bounds, cell_pixel_scale)?;
    let mut buf = Vec::new();
    out.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Png,
    )
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(buf)
}

pub fn write_board_png(
    def: &GameDefinition,
    occupancy: &crate::sim::OccupancyGrid,
    bounds: GridBounds,
    cell_pixel_scale: u32,
    path: &Path,
) -> std::io::Result<()> {
    let mut raster = BoardPngRaster::new(def, occupancy, bounds, cell_pixel_scale)?;
    while !raster.advance(u32::MAX) {}
    raster.write_png(path)
}

/// Incremental raster + PNG encode (used for WASM export spread across frames).
pub struct BoardPngRaster {
    bounds: GridBounds,
    occupancy: crate::sim::OccupancyGrid,
    piece_colors: Vec<[u8; 4]>,
    cell_pixel_scale: u32,
    width: u32,
    height: u32,
    out_w: u32,
    out_h: u32,
    row_stride: usize,
    grid_row: Vec<u8>,
    out_data: Vec<u8>,
    next_row: u32,
}

impl BoardPngRaster {
    pub fn new(
        def: &GameDefinition,
        occupancy: &crate::sim::OccupancyGrid,
        bounds: GridBounds,
        cell_pixel_scale: u32,
    ) -> std::io::Result<Self> {
        let cell_pixel_scale = sanitize_cell_pixel_scale(cell_pixel_scale);
        let grid_size = grid_texture_size(bounds);
        let width = grid_size.x;
        let height = grid_size.y;
        let out_w = width.saturating_mul(cell_pixel_scale);
        let out_h = height.saturating_mul(cell_pixel_scale);
        let out_len = (out_w as usize)
            .saturating_mul(out_h as usize)
            .saturating_mul(4);
        Ok(Self {
            bounds,
            occupancy: occupancy.clone(),
            piece_colors: def.pieces.iter().map(|a| rgba8(a.color)).collect(),
            cell_pixel_scale,
            width,
            height,
            out_w,
            out_h,
            row_stride: (out_w * 4) as usize,
            grid_row: vec![0u8; (width * 4) as usize],
            out_data: vec![0u8; out_len],
            next_row: 0,
        })
    }

    pub fn progress(&self) -> f32 {
        if self.height == 0 {
            1.0
        } else {
            self.next_row as f32 / self.height as f32
        }
    }

    /// Raster up to `max_grid_rows` more spiral rows. Returns `true` when finished.
    pub fn advance(&mut self, max_grid_rows: u32) -> bool {
        let mut processed = 0u32;
        while self.next_row < self.height && processed < max_grid_rows {
            let y = self.bounds.min_y + self.next_row as i32;
            for x in self.bounds.min_x..=self.bounds.max_x {
                let px = (x - self.bounds.min_x) as u32;
                let index = crate::spiral::xy_to_index(x, y);
                let color = if let Some(piece_id) = self.occupancy.get(&index) {
                    self.piece_colors
                        .get(*piece_id)
                        .copied()
                        .unwrap_or(EMPTY_RGBA)
                } else {
                    EMPTY_RGBA
                };
                let offset = (px * 4) as usize;
                self.grid_row[offset..offset + 4].copy_from_slice(&color);
            }

            let py = (self.bounds.max_y - y) as u32;
            let out_y_base = py * self.cell_pixel_scale;
            for sy in 0..self.cell_pixel_scale {
                let out_row_start = ((out_y_base + sy) as usize) * self.row_stride;
                let out_row = &mut self.out_data[out_row_start..out_row_start + self.row_stride];
                expand_row_nearest(&self.grid_row, self.width, self.cell_pixel_scale, out_row);
            }

            self.next_row += 1;
            processed += 1;
        }
        self.next_row >= self.height
    }

    pub fn encode_png(self) -> std::io::Result<Vec<u8>> {
        png_bytes_from_rgba(&self.out_data, self.out_w, self.out_h)
    }

    pub fn write_png(self, path: &Path) -> std::io::Result<()> {
        let bytes = png_bytes_from_rgba(&self.out_data, self.out_w, self.out_h)?;
        std::fs::write(path, bytes)
    }
}

fn png_bytes_from_rgba(out_data: &[u8], out_w: u32, out_h: u32) -> std::io::Result<Vec<u8>> {
    use png::{BitDepth, ColorType, Encoder};
    use std::io::Cursor;

    let mut buf = Vec::new();
    let mut encoder = Encoder::new(Cursor::new(&mut buf), out_w, out_h);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writer
        .write_image_data(out_data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writer
        .finish()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(buf)
}

fn expand_row_nearest(src_row: &[u8], width: u32, scale: u32, out_row: &mut [u8]) {
    for px in 0..width {
        let src = (px * 4) as usize;
        let pixel = &src_row[src..src + 4];
        for k in 0..scale {
            let dst = ((px * scale + k) * 4) as usize;
            out_row[dst..dst + 4].copy_from_slice(pixel);
        }
    }
}

fn raster_board_rgba(
    def: &GameDefinition,
    occupancy: &crate::sim::OccupancyGrid,
    bounds: GridBounds,
    cell_pixel_scale: u32,
) -> std::io::Result<RgbaImage> {
    let cell_pixel_scale = sanitize_cell_pixel_scale(cell_pixel_scale);
    let grid_size = grid_texture_size(bounds);
    let width = grid_size.x;
    let height = grid_size.y;
    let piece_colors: Vec<[u8; 4]> = def.pieces.iter().map(|a| rgba8(a.color)).collect();

    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in bounds.min_y..=bounds.max_y {
        let py = (bounds.max_y - y) as u32;
        for x in bounds.min_x..=bounds.max_x {
            let px = (x - bounds.min_x) as u32;
            let index = crate::spiral::xy_to_index(x, y);
            let color = if let Some(piece_id) = occupancy.get(&index) {
                piece_colors
                    .get(*piece_id)
                    .copied()
                    .unwrap_or(EMPTY_RGBA)
            } else {
                EMPTY_RGBA
            };
            let offset = ((py * width + px) * 4) as usize;
            data[offset..offset + 4].copy_from_slice(&color);
        }
    }

    let img: RgbaImage =
        ImageBuffer::from_raw(width, height, data).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid image buffer")
        })?;

    let out_w = width.saturating_mul(cell_pixel_scale);
    let out_h = height.saturating_mul(cell_pixel_scale);

    Ok(if cell_pixel_scale == 1 {
        img
    } else {
        imageops::resize(&img, out_w, out_h, FilterType::Nearest)
    })
}

fn placement_checksum(placements: &[(u32, PieceId)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &(index, piece_id) in placements {
        let value = ((index as u64) << 8) ^ piece_id as u64;
        hash ^= value;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn rgba8(color: Color) -> [u8; 4] {
    let color = color.to_srgba();
    [
        float_to_u8(color.red),
        float_to_u8(color.green),
        float_to_u8(color.blue),
        float_to_u8(color.alpha),
    ]
}

fn float_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_run_is_deterministic() {
        let spec = DiscoverRunSpec {
            random_gen: RandomGenConfig::default(),
            rng_seed: 123,
            turns: 200,
            target_index: 0,
        };
        let (_, _, sim_a, meta_a) = run_random_iteration(&spec);
        let (_, _, sim_b, meta_b) = run_random_iteration(&spec);
        assert_eq!(sim_a.placements, sim_b.placements);
        assert_eq!(meta_a.checksum, meta_b.checksum);
    }

    #[test]
    fn write_png_roundtrip_smoke() {
        let spec = DiscoverRunSpec {
            random_gen: RandomGenConfig {
                piece_count_min: 2,
                piece_count_max: 2,
                attack_radius_min: 1,
                attack_radius_max: 2,
                pattern_density: 0.5,
                attack_symmetry: AttackSymmetry::None,
                identical_pieces: false,
            },
            rng_seed: 7,
            turns: 80,
            target_index: 0,
        };
        let (config, def, sim, meta) = run_random_iteration(&spec);
        let dir = std::env::temp_dir().join("rbk_discover_test");
        let _ = fs::remove_dir_all(&dir);
        let scale = fit_output_cell_scale(meta.grid_cells[0], meta.grid_cells[1], DEFAULT_CELL_PIXEL_SCALE);
        write_run_outputs(&dir, &config, &def, &sim, &meta, scale).unwrap();
        assert!(dir.join("board.png").is_file());
        let out = image::open(dir.join("board.png")).unwrap();
        assert_eq!(out.width(), meta.grid_cells[0] * scale);
        assert!(dir.join("config.toml").is_file());
        let loaded: DiscoverRunConfig =
            toml::from_str(&fs::read_to_string(dir.join("config.toml")).unwrap()).unwrap();
        assert_eq!(loaded.game.pieces.len(), def.pieces.len());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn iteration_specs_differ_with_index() {
        let a = spec_for_iteration(99, 0, 10, 0);
        let b = spec_for_iteration(99, 1, 10, 0);
        assert_ne!(a.rng_seed, b.rng_seed);
    }

    #[test]
    fn knight_pairwise_grid_grows_with_target_index() {
        use crate::model::GameDefinition;

        let def = GameDefinition::knight_2_pairwise();
        let mut sim = Simulation::new(&def);
        sim.advance_to_target(&def, 50_000);
        let bounds = bounds_from_placements(&sim.placements, BOUNDS_PADDING);
        assert!(bounds.cell_width() > 200);
        assert!(bounds.cell_height() > 200);
    }
}
