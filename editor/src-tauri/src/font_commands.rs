use crate::resolve_library_paths;
use base64::Engine;
use serde::Serialize;
use std::{io::Cursor, path::Path};

#[tauri::command]
pub fn japanese_font() -> tauri::ipc::Response {
    tauri::ipc::Response::new(m2_publish::fonts::FONT_BYTES.to_vec())
}

#[derive(Serialize)]
pub struct TitlePreview {
    image: String,
    text_width: u32,
    scale: f64,
}

#[tauri::command]
pub async fn title_preview(
    games_path: String,
    text: String,
    titlebar: u32,
) -> Result<TitlePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (library, _, stock) = resolve_library_paths(Path::new(&games_path));
        let path = m2_publish::fonts::resource_path(
            &library,
            &stock,
            "system/motion/titleselect_ui.psb.m",
        )
        .map_err(|e| e.to_string())?;
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let ui = m2_publish::decode_resource(&bytes, "titleselect_ui.psb.m")
            .map_err(|e| e.to_string())?;
        let font = m2_publish::fonts::resource_path(
            &library,
            &stock,
            "system/font/makoto_basefont_32pt.psb.m",
        )
        .map_err(|e| e.to_string())?;
        let font = m2_publish::decode_resource(
            &std::fs::read(font).map_err(|e| e.to_string())?,
            "makoto_basefont_32pt.psb.m",
        )
        .map_err(|e| e.to_string())?;
        let preview = m2_publish::title_preview::render(&ui, &font, &text, titlebar)
            .map_err(|e| e.to_string())?;
        let mut png = Cursor::new(Vec::new());
        preview
            .image
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(TitlePreview {
            image: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png.into_inner())
            ),
            text_width: preview.text_width,
            scale: preview.scale,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
