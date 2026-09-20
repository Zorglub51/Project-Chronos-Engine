//! Read console save previews without loading the emulator state into memory.
use base64::Engine;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const PIXEL_CAPACITY: usize = 320 * 240 * 3;

#[derive(Debug, Serialize)]
pub struct SaveStateInfo {
    pub slot: u32,
    pub exists: bool,
    pub thumbnail: Option<String>,
    pub preview_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GameSaves {
    pub states: Vec<SaveStateInfo>,
    /// Presence of the imported SRAM file, not proof of in-game progression.
    pub sram_present: bool,
}

fn is_file(path: &Path) -> Result<bool, String> {
    match fs::metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
    }
}

pub fn read_game_saves(game: &Path) -> Result<GameSaves, String> {
    let mut states = Vec::with_capacity(4);
    for slot in 0..4 {
        let native = game.join(format!("saves/state_{slot}.bin"));
        let legacy = game.join(format!("save/save{}.json", slot + 1));
        let preview = if is_file(&native)? {
            Some(native_thumbnail(&native))
        } else if is_file(&legacy)? {
            Some(legacy_thumbnail(&legacy))
        } else {
            None
        };
        let exists = preview.is_some();
        let (thumbnail, preview_error) = match preview {
            Some(Ok(png)) => (Some(png), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, None),
        };
        states.push(SaveStateInfo {
            slot,
            exists,
            thumbnail,
            preview_error,
        });
    }
    Ok(GameSaves {
        states,
        sram_present: is_file(&game.join("sram.bin"))?,
    })
}

fn pixel_count(width: u32, height: u32) -> Result<usize, String> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(3))
        .ok_or("Invalid thumbnail dimensions")?;
    if width == 0 || height == 0 || bytes > PIXEL_CAPACITY as u64 {
        return Err("Invalid thumbnail dimensions".into());
    }
    Ok(bytes as usize)
}

fn png_url(width: u32, height: u32, pixels: Vec<u8>) -> Result<String, String> {
    let img = image::RgbImage::from_raw(width, height, pixels).ok_or("Invalid RGB thumbnail")?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

fn native_thumbnail(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    // Original struct_statedata / struct_statedata_l: 512-byte prefix,
    // 256 KiB / 1536 KiB state, 256-byte medal data, then width/height/RGB.
    // Check the complete known layout; never guess offsets from pixel data.
    let offset = match file.metadata().map_err(|e| e.to_string())?.len() {
        895_848 => 0x40300,
        2_206_568 => 0x180300,
        _ => return Err("Unrecognized console save size; preview unavailable".into()),
    };
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut dimensions = [0; 8];
    file.read_exact(&mut dimensions)
        .map_err(|e| e.to_string())?;
    let width = u32::from_le_bytes(dimensions[..4].try_into().unwrap());
    let height = u32::from_le_bytes(dimensions[4..].try_into().unwrap());
    let mut pixels = vec![0; pixel_count(width, height)?];
    file.read_exact(&mut pixels).map_err(|e| e.to_string())?;
    png_url(width, height, pixels)
}

fn legacy_thumbnail(path: &Path) -> Result<String, String> {
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let thumb = &json["root"]["_05_thumbnail"];
    let dimension = |key: &str| {
        thumb[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| "Invalid thumbnail dimensions".to_string())
    };
    let (width, height) = (dimension("_00_width")?, dimension("_01_height")?);
    let expected = pixel_count(width, height)?;
    let blob = thumb["_02_pixels"]["__blob__"]
        .as_str()
        .ok_or("No thumbnail blob")?;
    if blob.len() > PIXEL_CAPACITY * 4 / 3 + 4 {
        return Err("Thumbnail blob too large".into());
    }
    let pixels = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .map_err(|e| e.to_string())?;
    if pixels.len() != expected {
        return Err("RGB data size mismatch".into());
    }
    png_url(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "chronos-save-preview-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn state(path: &Path, size: u64, offset: u64, width: u32, height: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = File::create(path).unwrap();
        f.set_len(size).unwrap();
        f.seek(SeekFrom::Start(offset)).unwrap();
        f.write_all(&width.to_le_bytes()).unwrap();
        f.write_all(&height.to_le_bytes()).unwrap();
        f.write_all(&[255, 0, 0, 0, 255, 0]).unwrap();
    }

    #[test]
    fn native_small_and_large_previews_keep_slots_and_rgb_pixels() {
        let t = Temp::new();
        for (slot, size, offset) in [(1, 895_848, 0x40300), (3, 2_206_568, 0x180300)] {
            state(
                &t.0.join(format!("saves/state_{slot}.bin")),
                size,
                offset,
                2,
                1,
            );
        }
        fs::write(t.0.join("sram.bin"), vec![0; 8448]).unwrap();
        let saves = read_game_saves(&t.0).unwrap();
        assert!(saves.sram_present);
        assert_eq!(
            saves.states.iter().map(|s| s.exists).collect::<Vec<_>>(),
            [false, true, false, true]
        );
        for slot in [1, 3] {
            let png = saves.states[slot]
                .thumbnail
                .as_ref()
                .unwrap()
                .split_once(',')
                .unwrap()
                .1;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(png)
                .unwrap();
            let img = image::load_from_memory(&bytes).unwrap().to_rgb8();
            assert_eq!(img.dimensions(), (2, 1));
            assert_eq!(img.into_raw(), [255, 0, 0, 0, 255, 0]);
        }
    }

    #[test]
    fn malformed_previews_remain_present_and_do_not_allocate_from_bad_dimensions() {
        let t = Temp::new();
        state(
            &t.0.join("saves/state_0.bin"),
            895_848,
            0x40300,
            u32::MAX,
            u32::MAX,
        );
        fs::write(t.0.join("saves/state_2.bin"), b"truncated").unwrap();
        let saves = read_game_saves(&t.0).unwrap();
        for slot in [0, 2] {
            assert!(saves.states[slot].exists);
            assert!(saves.states[slot].thumbnail.is_none());
            assert!(saves.states[slot].preview_error.is_some());
        }
        assert!(!saves.sram_present);
    }

    #[test]
    fn legacy_json_is_supported_but_native_save_takes_precedence() {
        let t = Temp::new();
        fs::create_dir(t.0.join("save")).unwrap();
        let json = br#"{"root":{"_05_thumbnail":{"_00_width":1,"_01_height":1,"_02_pixels":{"__blob__":"/wAA"}}}}"#;
        fs::write(t.0.join("save/save1.json"), json).unwrap();
        assert!(read_game_saves(&t.0).unwrap().states[0].thumbnail.is_some());
        fs::create_dir(t.0.join("saves")).unwrap();
        fs::write(t.0.join("saves/state_0.bin"), b"unreadable native preview").unwrap();
        assert!(read_game_saves(&t.0).unwrap().states[0]
            .preview_error
            .is_some());
    }
}
