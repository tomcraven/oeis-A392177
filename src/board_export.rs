use bevy::prelude::*;
#[cfg(not(target_family = "wasm"))]
use bevy::tasks::futures::check_ready;
#[cfg(not(target_family = "wasm"))]
use bevy::tasks::{AsyncComputeTaskPool, Task};
#[cfg(not(target_family = "wasm"))]
use bevy::window::{PrimaryWindow, RawHandleWrapperHolder};

#[cfg(not(target_family = "wasm"))]
use crate::discover::{self, DEFAULT_CELL_PIXEL_SCALE};
#[cfg(target_family = "wasm")]
use crate::discover::{BoardPngRaster, DEFAULT_CELL_PIXEL_SCALE};
use crate::model::GameDefinition;
use crate::sim_worker::SimulationBridge;
use crate::ui::UiState;
use crate::viewport::GridBounds;

/// Grid rows rasterized per frame on WASM (keeps the browser responsive).
#[cfg(target_family = "wasm")]
const WASM_EXPORT_GRID_ROWS_PER_FRAME: u32 = 8;

#[derive(Resource, Default)]
pub struct BoardExportPending(pub Option<PendingBoardExport>);

pub struct PendingBoardExport {
    bounds: GridBounds,
}

/// Native: save sheet + optional background write.
#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
pub struct BoardExportDialogState {
    bounds: Option<GridBounds>,
    save_dialog: Option<SaveDialogFuture>,
    write_task: Option<Task<Result<String, String>>>,
}

#[cfg(not(target_family = "wasm"))]
type SaveDialogFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Option<rfd::FileHandle>>>>;

#[cfg(not(target_family = "wasm"))]
impl BoardExportDialogState {
    pub fn is_active(&self) -> bool {
        self.bounds.is_some() || self.save_dialog.is_some() || self.write_task.is_some()
    }
}

/// WASM: incremental raster spread across frames.
#[cfg(target_family = "wasm")]
#[derive(Resource, Default)]
pub struct BoardExportWasmJob {
    raster: Option<BoardPngRaster>,
    ready_to_encode: Option<BoardPngRaster>,
}

#[cfg(target_family = "wasm")]
impl BoardExportWasmJob {
    pub fn is_active(&self) -> bool {
        self.raster.is_some() || self.ready_to_encode.is_some()
    }
}

/// Request a PNG export (native: save dialog first; WASM: incremental encode).
pub fn queue_board_png_export(
    pending: &mut BoardExportPending,
    bounds: GridBounds,
    #[cfg(not(target_family = "wasm"))] dialog: &BoardExportDialogState,
    #[cfg(target_family = "wasm")] wasm_job: &BoardExportWasmJob,
) -> Result<(), String> {
    if pending.0.is_some() {
        return Err("Export already in progress".into());
    }
    #[cfg(not(target_family = "wasm"))]
    if dialog.is_active() {
        return Err("Export already in progress".into());
    }
    #[cfg(target_family = "wasm")]
    if wasm_job.is_active() {
        return Err("Export already in progress".into());
    }
    pending.0 = Some(PendingBoardExport { bounds });
    Ok(())
}

pub fn run_board_export(
    mut pending: ResMut<BoardExportPending>,
    mut ui_state: ResMut<UiState>,
    def: Res<GameDefinition>,
    sim: Res<SimulationBridge>,
    #[cfg(not(target_family = "wasm"))] mut dialog: NonSendMut<BoardExportDialogState>,
    #[cfg(not(target_family = "wasm"))] window_handles: Query<
        &RawHandleWrapperHolder,
        With<PrimaryWindow>,
    >,
    #[cfg(target_family = "wasm")] mut wasm_job: ResMut<BoardExportWasmJob>,
) {
    if let Some(job) = pending.0.take() {
        #[cfg(not(target_family = "wasm"))]
        {
            dialog.bounds = Some(job.bounds);
            begin_async_save_dialog(&mut dialog, &window_handles);
            ui_state.export_status = Some("Choose save location…".into());
        }
        #[cfg(target_family = "wasm")]
        {
            match BoardPngRaster::new(
                def.as_ref(),
                &sim.display.occupancy,
                job.bounds,
                DEFAULT_CELL_PIXEL_SCALE,
            ) {
                Ok(raster) => {
                    wasm_job.raster = Some(raster);
                    ui_state.export_status = Some("Exporting… 0%".into());
                }
                Err(e) => ui_state.export_status = Some(e.to_string()),
            }
        }
    }

    #[cfg(not(target_family = "wasm"))]
    {
        poll_save_dialog(
            &mut dialog,
            &mut ui_state,
            def.as_ref(),
            &sim.display.occupancy,
        );
        poll_write_task(&mut dialog, &mut ui_state);
    }

    #[cfg(target_family = "wasm")]
    poll_wasm_export(&mut wasm_job, &mut ui_state);
}

#[cfg(target_family = "wasm")]
fn poll_wasm_export(wasm_job: &mut BoardExportWasmJob, ui_state: &mut UiState) {
    if let Some(raster) = wasm_job.raster.as_mut() {
        let done = raster.advance(WASM_EXPORT_GRID_ROWS_PER_FRAME);
        if !done {
            let pct = (raster.progress() * 100.0).round();
            ui_state.export_status = Some(format!("Exporting… {pct:.0}%"));
            return;
        }
        wasm_job.ready_to_encode = wasm_job.raster.take();
        ui_state.export_status = Some("Preparing download…".into());
        return;
    }

    let Some(raster) = wasm_job.ready_to_encode.take() else {
        return;
    };

    match raster.encode_png() {
        Ok(bytes) => match download_png_bytes(&bytes, "board.png") {
            Ok(()) => ui_state.export_status = Some("Downloaded board.png".into()),
            Err(err) => ui_state.export_status = Some(err),
        },
        Err(e) => ui_state.export_status = Some(e.to_string()),
    }
}

#[cfg(not(target_family = "wasm"))]
fn poll_save_dialog(
    dialog: &mut BoardExportDialogState,
    ui_state: &mut UiState,
    def: &GameDefinition,
    occupancy: &crate::sim::OccupancyGrid,
) {
    let Some(future) = dialog.save_dialog.as_mut() else {
        return;
    };

    let Some(result) = check_ready(future) else {
        return;
    };

    dialog.save_dialog = None;
    let Some(bounds) = dialog.bounds.take() else {
        return;
    };

    match result {
        Some(handle) => {
            let path = handle.path().to_path_buf();
            ui_state.export_status = Some("Writing PNG…".into());
            let def = def.clone();
            let occupancy = occupancy.clone();
            dialog.write_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                discover::write_board_png(&def, &occupancy, bounds, DEFAULT_CELL_PIXEL_SCALE, &path)
                    .map(|_| format!("Saved {}", path.display()))
                    .map_err(|e| e.to_string())
            }));
        }
        None => ui_state.export_status = Some("Export cancelled".into()),
    }
}

#[cfg(not(target_family = "wasm"))]
fn poll_write_task(dialog: &mut BoardExportDialogState, ui_state: &mut UiState) {
    let Some(task) = dialog.write_task.as_mut() else {
        return;
    };
    let Some(result) = check_ready(task) else {
        return;
    };
    dialog.write_task = None;
    ui_state.export_status = Some(match result {
        Ok(msg) => msg,
        Err(err) => err,
    });
}

#[cfg(not(target_family = "wasm"))]
fn begin_async_save_dialog(
    dialog: &mut BoardExportDialogState,
    window_handles: &Query<&RawHandleWrapperHolder, With<PrimaryWindow>>,
) {
    let mut builder = rfd::AsyncFileDialog::new()
        .set_title("Export board as PNG")
        .set_file_name("red_black_knights_board.png");

    if let Some(dir) = dirs::download_dir().or_else(dirs::home_dir) {
        builder = builder.set_directory(dir);
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.add_filter("PNG image", &["png"]);
    }

    if let Ok(holder) = window_handles.single() {
        if let Ok(guard) = holder.0.lock() {
            if let Some(raw) = guard.as_ref() {
                let parent = unsafe { raw.get_handle() };
                builder = builder.set_parent(&parent);
            }
        }
    }

    dialog.save_dialog = Some(Box::pin(builder.save_file()));
}

#[cfg(target_family = "wasm")]
fn download_png_bytes(bytes: &[u8], filename: &str) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or("no browser window")?;
    let document = window.document().ok_or("no document")?;
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
        .map_err(|_| "failed to create download blob")?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "failed to create object URL")?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "failed to create download link")?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "download link was not an anchor")?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}
