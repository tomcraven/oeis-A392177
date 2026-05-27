use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::CELL_SIZE;
use crate::model::GameDefinition;
use crate::model::PieceId;
use crate::sim::{EMPTY_ARMY_SLOT, OccupancyGrid};
use crate::sim_worker::SimulationBridge;
use crate::spiral::xy_to_index;
use crate::ui::BoardColourMode;
use crate::viewport::{GridBounds, ViewportState, grid_to_world};

#[derive(Resource)]
pub struct RenderAssets {
    pub image: Handle<Image>,
    pub empty_color: [u8; 4],
    pub piece_colors: Vec<[u8; 4]>,
}

#[derive(Resource, Default)]
pub struct RenderCache {
    pub rendered_bounds: Option<GridBounds>,
    pub colour_mode: BoardColourMode,
    /// Reused RGBA buffer for grid-sized redraws (avoids alloc at max zoom).
    scratch_rgba: Vec<u8>,
}

#[derive(Component)]
pub struct BoardTexture;

pub fn setup_render_assets(
    mut commands: Commands,
    def: Res<GameDefinition>,
    mut images: ResMut<Assets<Image>>,
) {
    let handle = images.add(empty_image(1, 1));
    let empty_color = rgba8(Color::srgba(0.12, 0.12, 0.16, 1.0));
    let piece_colors = def.pieces.iter().map(|a| rgba8(a.color)).collect();
    commands.spawn((
        Sprite {
            image: handle.clone(),
            custom_size: Some(Vec2::splat(CELL_SIZE)),
            ..default()
        },
        Transform::default(),
        BoardTexture,
    ));
    commands.insert_resource(RenderAssets {
        image: handle,
        empty_color,
        piece_colors,
    });
}

pub fn sync_piece_materials(
    def: Res<GameDefinition>,
    mut assets: ResMut<RenderAssets>,
    mut cache: ResMut<RenderCache>,
) {
    if !def.is_changed() && assets.piece_colors.len() == def.pieces.len() {
        return;
    }
    assets.piece_colors = def.pieces.iter().map(|a| rgba8(a.color)).collect();
    cache.rendered_bounds = None;
}

pub fn draw_spiral_cells(
    assets: Res<RenderAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<RenderCache>,
    mut viewport: ResMut<ViewportState>,
    sim: Res<SimulationBridge>,
    ui_state: Res<crate::ui::UiState>,
    mut board_q: Query<(&mut Transform, &mut Sprite), With<BoardTexture>>,
    #[cfg(feature = "app_profile")] mut profile_frame: Option<
        ResMut<crate::app_profile::AppProfileFrame>,
    >,
) {
    draw_spiral_cells_inner(
        &assets,
        &mut images,
        &mut cache,
        &mut viewport,
        &sim.display.occupancy,
        &ui_state,
        &mut board_q,
        #[cfg(feature = "app_profile")]
        profile_frame.as_deref_mut(),
    );
}

fn draw_spiral_cells_inner(
    assets: &RenderAssets,
    images: &mut Assets<Image>,
    cache: &mut RenderCache,
    viewport: &mut ViewportState,
    occupancy: &OccupancyGrid,
    ui_state: &crate::ui::UiState,
    board_q: &mut Query<(&mut Transform, &mut Sprite), With<BoardTexture>>,
    #[cfg(feature = "app_profile")] mut profile_frame: Option<&mut crate::app_profile::AppProfileFrame>,
) {
    if assets.piece_colors.is_empty() {
        return;
    }
    let Some(bounds) = viewport.bounds else {
        return;
    };
    let grid_size = grid_texture_size(bounds);
    let colour_mode = ui_state.board_colour_mode;

    const GPU_TEXTURE_DIMENSION_LIMIT: u32 = 16_000;
    if grid_size.x > GPU_TEXTURE_DIMENSION_LIMIT || grid_size.y > GPU_TEXTURE_DIMENSION_LIMIT {
        return;
    }

    if !viewport.render_dirty
        && cache.rendered_bounds == Some(bounds)
        && cache.colour_mode == colour_mode
    {
        return;
    }

    let width = grid_size.x;
    let height = grid_size.y;
    let byte_len = (width as usize) * (height as usize) * 4;
    if cache.scratch_rgba.len() != byte_len {
        cache.scratch_rgba.resize(byte_len, 0);
    }

    let piece_px: Vec<u32> = assets
        .piece_colors
        .iter()
        .map(|c| rgba8_to_u32(*c))
        .collect();
    let empty_px = rgba8_to_u32(assets.empty_color);

    let mut raster = || {
        raster_spiral_grid_into(
            bounds,
            width,
            height,
            occupancy,
            &piece_px,
            empty_px,
            colour_mode,
            &mut cache.scratch_rgba,
        );
    };

    #[cfg(feature = "app_profile")]
    if let Some(frame) = profile_frame.as_mut() {
        crate::app_profile::scope("render_raster", frame, raster);
    } else {
        raster();
    }
    #[cfg(not(feature = "app_profile"))]
    raster();

    #[cfg(feature = "app_profile")]
    let write_image = || {
        write_grid_image(images, &assets.image, width, height, &mut cache.scratch_rgba)
    };
    #[cfg(not(feature = "app_profile"))]
    write_grid_image(images, &assets.image, width, height, &mut cache.scratch_rgba);

    #[cfg(feature = "app_profile")]
    if let Some(frame) = profile_frame.as_mut() {
        crate::app_profile::scope("render_image_write", frame, write_image);
    } else {
        write_image();
    }

    let mut layout_sprite = || {
        if let Ok((mut transform, mut sprite)) = board_q.single_mut() {
            let min = grid_to_world(bounds.min_x, bounds.min_y) - Vec2::splat(CELL_SIZE * 0.5);
            let max = grid_to_world(bounds.max_x, bounds.max_y) + Vec2::splat(CELL_SIZE * 0.5);
            let center = (min + max) * 0.5;
            transform.translation = center.extend(0.0);
            sprite.custom_size = Some(Vec2::new(
                grid_size.x as f32 * CELL_SIZE,
                grid_size.y as f32 * CELL_SIZE,
            ));
        }
    };

    #[cfg(feature = "app_profile")]
    if let Some(frame) = profile_frame.as_mut() {
        crate::app_profile::scope("render_sprite_layout", frame, layout_sprite);
    } else {
        layout_sprite();
    }
    #[cfg(not(feature = "app_profile"))]
    layout_sprite();

    cache.rendered_bounds = Some(bounds);
    cache.colour_mode = colour_mode;
    viewport.render_dirty = false;
}

/// CPU raster of visible spiral cells into RGBA8 (headless bench + app path).
pub fn raster_spiral_grid(
    bounds: GridBounds,
    width: u32,
    height: u32,
    occupancy: &OccupancyGrid,
    piece_colors: &[[u8; 4]],
    empty_color: [u8; 4],
    colour_mode: BoardColourMode,
) -> Vec<u8> {
    let piece_px: Vec<u32> = piece_colors.iter().map(|c| rgba8_to_u32(*c)).collect();
    let mut data = vec![0u8; (width as usize) * (height as usize) * 4];
    raster_spiral_grid_into(
        bounds,
        width,
        height,
        occupancy,
        &piece_px,
        rgba8_to_u32(empty_color),
        colour_mode,
        &mut data,
    );
    data
}

pub fn raster_spiral_grid_into(
    bounds: GridBounds,
    width: u32,
    _height: u32,
    occupancy: &OccupancyGrid,
    piece_colors_u32: &[u32],
    empty_pixel: u32,
    colour_mode: BoardColourMode,
    data: &mut [u8],
) {
    fill_rgba_buffer_u32(data, empty_pixel);

    match colour_mode {
        BoardColourMode::Piece => {
            #[cfg(not(target_family = "wasm"))]
            {
                raster_piece_rows_parallel(
                    bounds,
                    width,
                    occupancy,
                    piece_colors_u32,
                    data,
                );
            }
            #[cfg(target_family = "wasm")]
            {
                raster_piece_rows_sequential(bounds, width, occupancy, piece_colors_u32, data);
            }
        }
    }
}

#[cfg(target_family = "wasm")]
fn raster_piece_rows_sequential(
    bounds: GridBounds,
    width: u32,
    occupancy: &OccupancyGrid,
    piece_colors_u32: &[u32],
    data: &mut [u8],
) {
    let cells = occupancy.cells_slice();
    for y in bounds.min_y..=bounds.max_y {
        raster_one_row(y, bounds, width, cells, piece_colors_u32, data);
    }
}

#[cfg(not(target_family = "wasm"))]
fn raster_piece_rows_parallel(
    bounds: GridBounds,
    width: u32,
    occupancy: &OccupancyGrid,
    piece_colors_u32: &[u32],
    data: &mut [u8],
) {
    use std::thread;
    let cells = occupancy.cells_slice();
    let min_y = bounds.min_y;
    let max_y = bounds.max_y;
    let row_count = (max_y - min_y + 1) as usize;
    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, row_count.max(1));
    let rows_per = row_count.div_ceil(threads);

    let base_ptr = data.as_mut_ptr() as usize;
    let len = data.len();

    thread::scope(|scope| {
        for t in 0..threads {
            let y_start = min_y + (t * rows_per) as i32;
            if y_start > max_y {
                break;
            }
            let y_end = (y_start + rows_per as i32 - 1).min(max_y);
            scope.spawn(move || {
                let data = unsafe { std::slice::from_raw_parts_mut(base_ptr as *mut u8, len) };
                for y in y_start..=y_end {
                    raster_one_row(y, bounds, width, cells, piece_colors_u32, data);
                }
            });
        }
    });
}

fn raster_one_row(
    y: i32,
    bounds: GridBounds,
    width: u32,
    cells: &[PieceId],
    piece_colors_u32: &[u32],
    data: &mut [u8],
) {
    let py = (bounds.max_y - y) as u32;
    let row_base = (py * width) as usize * 4;
    let pixels = data.as_mut_ptr() as *mut u32;
    let pixel_stride = width as usize;

    for x in bounds.min_x..=bounds.max_x {
        let index = xy_to_index(x, y) as usize;
        let piece_id = match cells.get(index) {
            Some(&id) if id != EMPTY_ARMY_SLOT => id,
            _ => continue,
        };
        let Some(&px) = piece_colors_u32.get(piece_id) else {
            continue;
        };
        let col = (x - bounds.min_x) as usize;
        unsafe {
            pixels.add(row_base / 4 + col).write(px);
        }
    }
    let _ = pixel_stride;
}

fn fill_rgba_buffer_u32(data: &mut [u8], pixel: u32) {
    debug_assert_eq!(data.len() % 4, 0);
    let words =
        unsafe { std::slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u32, data.len() / 4) };
    words.fill(pixel);
}

fn write_grid_image(
    images: &mut Assets<Image>,
    handle: &Handle<Image>,
    width: u32,
    height: u32,
    scratch: &mut Vec<u8>,
) {
    let Some(image) = images.get_mut(handle) else {
        return;
    };
    let size = image.size();
    if size.x != width || size.y != height {
        *image = Image::new_fill(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
    }
    match &mut image.data {
        Some(existing) if existing.len() == scratch.len() => {
            std::mem::swap(existing, scratch);
        }
        slot => {
            *slot = Some(std::mem::take(scratch));
        }
    }
}

pub fn grid_texture_size(bounds: GridBounds) -> UVec2 {
    UVec2::new(
        bounds.cell_width().max(1) as u32,
        bounds.cell_height().max(1) as u32,
    )
}

fn rgba8_to_u32(color: [u8; 4]) -> u32 {
    u32::from_ne_bytes(color)
}

fn empty_image(width: u32, height: u32) -> Image {
    Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
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

/// FNV-style checksum for raster regression tests.
pub fn raster_checksum(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GameDefinition;
    use crate::sim::Simulation;
    use std::time::Instant;

    fn run_sim_placements(preset: fn() -> GameDefinition, turns: usize) -> OccupancyGrid {
        let def = preset();
        let mut sim = Simulation::new(&def);
        for _ in 0..turns {
            assert!(sim.step_turn(&def));
        }
        sim.occupancy
    }

    fn piece_colors_for(def: &GameDefinition) -> Vec<[u8; 4]> {
        def.pieces.iter().map(|a| rgba8(a.color)).collect()
    }

    #[test]
    fn raster_checksum_stable_for_knight_pairwise() {
        let def = GameDefinition::knight_2_pairwise();
        let occ = run_sim_placements(GameDefinition::knight_2_pairwise, 2_000);
        let bounds = GridBounds {
            min_x: -20,
            max_x: 20,
            min_y: -20,
            max_y: 20,
        };
        let size = grid_texture_size(bounds);
        let empty = rgba8(Color::srgba(0.12, 0.12, 0.16, 1.0));
        let colors = piece_colors_for(&def);
        let a = raster_spiral_grid(
            bounds,
            size.x,
            size.y,
            &occ,
            &colors,
            empty,
            BoardColourMode::Piece,
        );
        let b = raster_spiral_grid(
            bounds,
            size.x,
            size.y,
            &occ,
            &colors,
            empty,
            BoardColourMode::Piece,
        );
        assert_eq!(raster_checksum(&a), raster_checksum(&b));
        assert_eq!(a, b);
    }

    #[test]
    fn raster_large_bounds_completes_within_budget() {
        let def = GameDefinition::knight_3_clique();
        let occ = run_sim_placements(GameDefinition::knight_3_clique, 5_000);
        let bounds = GridBounds {
            min_x: -80,
            max_x: 80,
            min_y: -80,
            max_y: 80,
        };
        let size = grid_texture_size(bounds);
        let empty = rgba8(Color::srgba(0.12, 0.12, 0.16, 1.0));
        let colors = piece_colors_for(&def);
        let iters = 5u32;
        let start = Instant::now();
        for _ in 0..iters {
            let data = raster_spiral_grid(
                bounds,
                size.x,
                size.y,
                &occ,
                &colors,
                empty,
                BoardColourMode::Piece,
            );
            assert_eq!(data.len(), (size.x * size.y * 4) as usize);
        }
        let ms = start.elapsed().as_secs_f64() * 1e3 / iters as f64;
        eprintln!("raster_large_bounds median ~{ms:.2} ms ({iters} iters, {} cells)", bounds.cell_count());
        assert!(
            ms < 500.0,
            "raster perf regression: {ms:.1} ms per frame (budget 500 ms)"
        );
    }

    #[test]
    fn max_zoom_raster_completes_within_budget() {
        let def = GameDefinition::knight_2_pairwise();
        let occ = run_sim_placements(GameDefinition::knight_2_pairwise, 50_000);
        let half = 512;
        let bounds = GridBounds {
            min_x: -half,
            max_x: half,
            min_y: -half,
            max_y: half,
        };
        let size = grid_texture_size(bounds);
        let empty = rgba8(Color::srgba(0.12, 0.12, 0.16, 1.0));
        let colors = piece_colors_for(&def);
        let start = Instant::now();
        let data = raster_spiral_grid(
            bounds,
            size.x,
            size.y,
            &occ,
            &colors,
            empty,
            BoardColourMode::Piece,
        );
        let ms = start.elapsed().as_secs_f64() * 1e3;
        assert_eq!(data.len(), (size.x * size.y * 4) as usize);
        eprintln!(
            "max_zoom_raster_test ~{ms:.1} ms ({} cells)",
            bounds.cell_count()
        );
        assert!(
            ms < 120.0,
            "max-zoom raster regression: {ms:.1} ms (budget 120 ms for ±512 grid)"
        );
    }
}
