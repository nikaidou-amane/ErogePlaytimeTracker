use std::sync::OnceLock;
use tauri::{AppHandle, Manager, image::Image};

fn images() -> &'static (Image<'static>, Image<'static>, Image<'static>) {
    static IMAGES: OnceLock<(Image<'static>, Image<'static>, Image<'static>)> = OnceLock::new();
    IMAGES.get_or_init(|| {
        let normal = tauri::include_image!("icons/32x32.png");
        let mut paused = normal.rgba().to_vec();
        for pixel in paused.chunks_exact_mut(4) {
            let luminance = (u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2])) / 3;
            pixel[0] = (180 + luminance * 75 / 255) as u8;
            pixel[1] = (85 + luminance * 95 / 255) as u8;
            pixel[2] = (15 + luminance * 35 / 255) as u8;
        }
        let paused = Image::new_owned(paused, normal.width(), normal.height());
        let mut overlay = vec![0_u8; 16 * 16 * 4];
        for y in 0..16_i32 {
            for x in 0..16_i32 {
                if (x - 8).pow(2) + (y - 8).pow(2) <= 60 {
                    let color = if (5..=6).contains(&x) || (10..=11).contains(&x) {
                        if (4..=12).contains(&y) {
                            [255, 255, 255, 255]
                        } else {
                            [230, 150, 25, 255]
                        }
                    } else {
                        [230, 150, 25, 255]
                    };
                    let offset = ((y * 16 + x) * 4) as usize;
                    overlay[offset..offset + 4].copy_from_slice(&color);
                }
            }
        }
        (normal, paused, Image::new_owned(overlay, 16, 16))
    })
}

fn apply(app: &AppHandle, paused: bool) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id("tracker") else {
        return Ok(());
    };
    let (normal, changed, overlay) = images();
    tray.set_icon(Some(if paused { changed } else { normal }.clone()))?;
    if let Some(window) = app.get_webview_window("main") {
        window.set_overlay_icon(paused.then(|| overlay.clone()))?;
    }
    Ok(())
}

pub fn update(app: &AppHandle, requested: bool) -> tauri::Result<bool> {
    if app.tray_by_id("tracker").is_none() {
        return Ok(false);
    }
    let handle = app.clone();
    // Native icon APIs run on the event loop; never wait for that loop while
    // holding tracking/state locks. Queued updates render the latest DB state.
    app.run_on_main_thread(move || {
        let state = handle.try_state::<crate::AppState>();
        let paused = state
            .as_ref()
            .and_then(|state| state.db.active_pause().ok())
            .map(|pause| pause.is_some())
            .unwrap_or(requested);
        match apply(&handle, paused) {
            Ok(()) => {
                if let Some(state) = state {
                    state.tracker.pause_icons_rendered(Some(paused));
                }
            }
            Err(error) => {
                log::warn!("could not update pause icons: {error}");
                if let Some(state) = state {
                    state.tracker.pause_icons_rendered(None);
                }
            }
        }
    })?;
    Ok(true)
}
