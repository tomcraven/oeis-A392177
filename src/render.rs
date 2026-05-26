use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::CELL_SIZE;
use crate::model::GameDefinition;
use crate::sim_worker::SimulationBridge;
use crate::spiral::xy_to_index;
use crate::viewport::{GridBounds, ViewportState, grid_to_world};

#[derive(Resource)]
pub struct RenderAssets {
    pub image: Handle<Image>,
    pub empty_color: [u8; 4],
    pub army_colors: Vec<[u8; 4]>,
}

#[derive(Resource, Default)]
pub struct RenderCache {
    pub rendered_bounds: Option<GridBounds>,
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
    let army_colors = def.armies.iter().map(|a| rgba8(a.color)).collect();
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
        army_colors,
    });
}

pub fn sync_army_materials(
    def: Res<GameDefinition>,
    mut assets: ResMut<RenderAssets>,
    mut cache: ResMut<RenderCache>,
) {
    if !def.is_changed() && assets.army_colors.len() == def.armies.len() {
        return;
    }
    assets.army_colors = def.armies.iter().map(|a| rgba8(a.color)).collect();
    cache.rendered_bounds = None;
}

pub fn draw_spiral_cells(
    assets: Res<RenderAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<RenderCache>,
    mut viewport: ResMut<ViewportState>,
    sim: Res<SimulationBridge>,
    mut board_q: Query<(&mut Transform, &mut Sprite), With<BoardTexture>>,
) {
    if assets.army_colors.is_empty() {
        return;
    }
    let Some(bounds) = viewport.bounds else {
        return;
    };
    let grid_size = grid_texture_size(bounds);

    const GPU_TEXTURE_DIMENSION_LIMIT: u32 = 16_000;
    if grid_size.x > GPU_TEXTURE_DIMENSION_LIMIT || grid_size.y > GPU_TEXTURE_DIMENSION_LIMIT {
        return;
    }

    if !viewport.render_dirty && cache.rendered_bounds == Some(bounds) {
        return;
    }

    let width = grid_size.x;
    let height = grid_size.y;
    let mut data = vec![0; (width * height * 4) as usize];

    for y in bounds.min_y..=bounds.max_y {
        let py = (bounds.max_y - y) as u32;
        for x in bounds.min_x..=bounds.max_x {
            let px = (x - bounds.min_x) as u32;
            let index = xy_to_index(x, y);
            let color = if let Some(&army_id) = sim.display.occupancy.get(&index) {
                assets
                    .army_colors
                    .get(army_id)
                    .copied()
                    .unwrap_or(assets.empty_color)
            } else {
                assets.empty_color
            };
            let offset = ((py * width + px) * 4) as usize;
            data[offset..offset + 4].copy_from_slice(&color);
        }
    }

    if let Some(image) = images.get_mut(&assets.image) {
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
        image.data = Some(data);
    }

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

    cache.rendered_bounds = Some(bounds);
    viewport.render_dirty = false;
}

pub fn grid_texture_size(bounds: GridBounds) -> UVec2 {
    UVec2::new(
        bounds.cell_width().max(1) as u32,
        bounds.cell_height().max(1) as u32,
    )
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
