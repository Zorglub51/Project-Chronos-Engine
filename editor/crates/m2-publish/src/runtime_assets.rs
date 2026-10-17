//! Desktop-only preparation of individual native Image resources and fixed
//! motion templates. No catalogue, ROM or save data is emitted here.
mod motion;

use crate::{console::ConsoleVariant, library, save_format, templates, Error, LineupEntry};
use image::{imageops, ImageReader, Rgba, RgbaImage};
use m2_psb::{Stream, Value};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

pub const FULL_COVER_SLOTS: usize = 7;
pub const THUMBNAIL_SLOTS: usize = 100;
pub const THUMBNAIL_PIXELS: u32 = 48;
pub const SOURCE_NAME: &str = "chronos_cover";
pub const TRANSPARENT_PATH: &str = "system/image/chronos_transparent.image.psb";
pub const BACK_DIRECTORY: &str = "runtime/back";
const GENERATOR: &str = "chronos-images-v2-3";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Surface {
    pub label: String,
    pub width: u32,
    pub height: u32,
    /// Effective registration origin, including the sleeve's original offset.
    pub origin_x: f64,
    pub origin_y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreparedCover {
    pub schema: u32,
    pub revision: String,
    /// Native Resource paths omit the physical file's final `.m` suffix.
    pub cover_path: String,
    pub thumb_path: String,
    pub width: u32,
    pub height: u32,
    pub origin_x: f64,
    pub origin_y: f64,
    pub front_origin_x: f64,
    pub front_origin_y: f64,
    pub left: Option<Surface>,
    pub right: Option<Surface>,
    pub cover_digest: String,
    pub thumb_digest: String,
}

#[derive(Debug, Clone, Default)]
pub struct AssetReport {
    pub covers_prepared: usize,
    pub templates_prepared: usize,
    pub fonts_prepared: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Geometry {
    front_origin_x: f64,
    front_origin_y: f64,
    left: Surface,
    right: Surface,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Library(message.into())
}
fn obj(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn integer(n: u32) -> Value {
    Value::Int(i64::from(n))
}
fn digest(bytes: &[u8]) -> String {
    save_format::digest_hex(&Md5::digest(bytes).into())
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    save_format::atomic_write(path, bytes).map_err(invalid)
}
fn regular(path: &Path) -> Result<(), Error> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(invalid(format!(
            "Expected a regular image/resource: {}",
            path.display()
        )));
    }
    Ok(())
}
fn safe_directory(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(m) if !m.file_type().is_dir() => Err(invalid(format!(
            "Expected a regular asset directory: {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn read_png(path: &Path) -> Result<RgbaImage, Error> {
    regular(path)?;
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| invalid(format!("Read cover {}: {e}", path.display())))?
        .to_rgba8();
    if image.width() == 0 || image.height() == 0 {
        return Err(invalid("Empty cover image"));
    }
    Ok(image)
}
fn png_bytes(image: &RgbaImage) -> Result<Vec<u8>, Error> {
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| invalid(e.to_string()))?;
    Ok(bytes.into_inner())
}

/// The proven original native Image schema: one complete RGBA8 texture chunk
/// per label. A texture atlas inside one Image entry is deliberately avoided.
pub fn image_psb(images: &[(&str, &RgbaImage)]) -> Result<Vec<u8>, Error> {
    let mut entries = Vec::new();
    for (index, (label, image)) in images.iter().enumerate() {
        let (w, h) = image.dimensions();
        if w == 0 || h == 0 || w > 512 || h > 512 {
            return Err(invalid("Native image dimensions must be 1–512 pixels"));
        }
        entries.push(obj([
            ("label", Value::String((*label).into())),
            ("width", integer(w)),
            ("height", integer(h)),
            (
                "texture",
                Value::Array(vec![obj([
                    ("left", Value::Float(0.0)),
                    ("top", Value::Float(0.0)),
                    ("width", Value::Float(f64::from(w))),
                    ("height", Value::Float(f64::from(h))),
                    (
                        "image",
                        obj([
                            ("ast", integer(0)),
                            ("width", integer(w)),
                            ("height", integer(h)),
                            ("truncated_width", integer(w)),
                            ("truncated_height", integer(h)),
                            ("type", Value::String("RGBA8".into())),
                            (
                                "pixel",
                                Value::Stream(Stream {
                                    index: index as u32,
                                    data: image.as_raw().clone(),
                                }),
                            ),
                        ]),
                    ),
                ])]),
            ),
        ]));
    }
    let tree = obj([
        ("id", Value::String("image".into())),
        ("imageList", Value::Array(entries)),
        ("label", Value::String("chronos_image".into())),
        ("spec", Value::String("and".into())),
        ("version", Value::Float(1.0)),
    ]);
    let bytes = m2_psb::write(&tree, 4)?;
    let checked = m2_psb::read(&bytes)?;
    if checked.to_json() != tree.to_json() {
        return Err(invalid("Native image PSB roundtrip failed"));
    }
    Ok(bytes)
}

fn pack(path: &Path, raw: &[u8]) -> Result<Vec<u8>, Error> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("Invalid resource filename"))?;
    let bytes = m2_mzs::pack_default(raw, name)?;
    if m2_mzs::unpack_default(&bytes, name)? != raw {
        return Err(invalid("Image MZS roundtrip failed"));
    }
    Ok(bytes)
}
fn valid_cache(dir: &Path, cover: &PreparedCover) -> bool {
    cover.schema == 1
        && [
            (&cover.cover_path, &cover.cover_digest),
            (&cover.thumb_path, &cover.thumb_digest),
        ]
        .iter()
        .all(|(path, hash)| {
            let path = Path::new(path);
            if path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            {
                return false;
            }
            let physical = dir.join(format!("{}.m", path.to_string_lossy()));
            regular(&physical).is_ok()
                && fs::read(physical).is_ok_and(|bytes| digest(&bytes) == **hash)
        })
}

/// Refresh one cover independently of folder, title, sorting or save identity.
/// The descriptor is committed last; failures keep its previous revision valid.
pub fn prepare_entry_assets(entry: &Path) -> Result<PreparedCover, Error> {
    safe_directory(entry)?;
    let json = entry.join("game.json");
    let game: Option<library::GameJson> = if json.is_file() {
        Some(serde_json::from_slice(&fs::read(json)?)?)
    } else {
        None
    };
    let name = game
        .as_ref()
        .and_then(|g| g.cover.as_deref())
        .unwrap_or("cover.png");
    if Path::new(name)
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid("Cover path must stay inside its entry directory"));
    }
    let input = entry.join(name);
    let image = if input.exists() {
        if !input.canonicalize()?.starts_with(entry.canonicalize()?) {
            return Err(invalid("Cover resolves outside its entry directory"));
        }
        read_png(&input)?
    } else if fs::symlink_metadata(&input).is_ok() {
        return Err(invalid("Invalid cover image link"));
    } else {
        RgbaImage::from_pixel(240, 240, Rgba([32, 32, 40, 255]))
    };
    let size = game.as_ref().and_then(|g| g.cover_size.as_ref());
    let width = size.map(|s| s.width).filter(|w| *w > 0).unwrap_or(240);
    let height = size.map(|s| s.height).filter(|h| *h > 0).unwrap_or(240);
    if width > 512 || height > 512 {
        return Err(invalid("Cover geometry exceeds the native 512-pixel limit"));
    }
    let origin_x = f64::from(
        size.map(|s| s.origin_x)
            .filter(|n| *n > 0)
            .unwrap_or(width / 2),
    );
    let origin_y = f64::from(
        size.map(|s| s.origin_y)
            .filter(|n| *n > 0)
            .unwrap_or(height / 2),
    );
    let center = if image.dimensions() == (width, height) {
        image
    } else {
        imageops::resize(&image, width, height, imageops::FilterType::Lanczos3)
    };
    let geometry_path = entry.join("cover_geometry.json");
    let mut geometry: Option<Geometry> = if geometry_path.is_file() {
        regular(&geometry_path)?;
        Some(serde_json::from_slice(&fs::read(&geometry_path)?)?)
    } else {
        None
    };
    let mut left = geometry
        .as_ref()
        .map(|_| read_png(&entry.join("cover_left.png")))
        .transpose()?;
    let mut right = geometry
        .as_ref()
        .map(|_| read_png(&entry.join("cover_right.png")))
        .transpose()?;
    // A newly imported SGX cover may supply only center artwork. Its neutral
    // generic sleeve carries no invented game-specific lettering or branding.
    // Retail/editor-provided strip inputs always take precedence.
    if geometry.is_none() && game.as_ref().is_some_and(|g| g.display.ccolor == 3) {
        let mut strip = RgbaImage::from_pixel(20, height, Rgba([96, 96, 104, 255]));
        let radius = (height / 20).clamp(4, 10) as i32;
        let mid = height as i32 / 2;
        for y in 0..height {
            for x in 0..20 {
                if (x as i32).pow(2) + (y as i32 - mid).pow(2) <= radius.pow(2) {
                    strip.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        left = Some(strip);
        right = Some(RgbaImage::from_pixel(
            12,
            height,
            Rgba([144, 144, 152, 255]),
        ));
        geometry = Some(Geometry {
            front_origin_x: origin_x - 4.0,
            front_origin_y: origin_y,
            left: Surface {
                label: "left".into(),
                width: 20,
                height,
                origin_x: origin_x - 4.0 + 20.0,
                origin_y,
            },
            right: Surface {
                label: "right".into(),
                width: 12,
                height,
                origin_x: origin_x - 4.0 - f64::from(width),
                origin_y,
            },
        });
    }
    if let Some(g) = &geometry {
        if ![
            g.front_origin_x,
            g.front_origin_y,
            g.left.origin_x,
            g.left.origin_y,
            g.right.origin_x,
            g.right.origin_y,
        ]
        .iter()
        .all(|n| n.is_finite() && n.abs() <= 2048.0)
            || left.as_ref().unwrap().dimensions() != (g.left.width, g.left.height)
            || right.as_ref().unwrap().dimensions() != (g.right.width, g.right.height)
        {
            return Err(invalid("Invalid SGX sleeve image geometry"));
        }
    }
    let fit = imageops::thumbnail(&center, THUMBNAIL_PIXELS, THUMBNAIL_PIXELS);
    let mut thumb = RgbaImage::new(THUMBNAIL_PIXELS, THUMBNAIL_PIXELS);
    imageops::overlay(
        &mut thumb,
        &fit,
        i64::from((THUMBNAIL_PIXELS - fit.width()) / 2),
        i64::from((THUMBNAIL_PIXELS - fit.height()) / 2),
    );
    let mut hash = Md5::new();
    hash.update(GENERATOR);
    hash.update(center.as_raw());
    hash.update(serde_json::to_vec(&(
        width, height, origin_x, origin_y, &geometry,
    ))?);
    if let Some(image) = &left {
        hash.update(image.as_raw());
    }
    if let Some(image) = &right {
        hash.update(image.as_raw());
    }
    let revision = save_format::digest_hex(&hash.finalize().into());
    let runtime = entry.join("runtime");
    safe_directory(&runtime)?;
    let marker = runtime.join("assets.json");
    if fs::symlink_metadata(&marker).is_ok() {
        regular(&marker)?;
    }
    let previous: Option<PreparedCover> = fs::read(&marker)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(old) = &previous {
        if old.revision == revision && valid_cache(entry, old) {
            return Ok(old.clone());
        }
    }
    let cover_path = format!("runtime/cover-{revision}.image.psb");
    let thumb_path = format!("runtime/thumb-{revision}.image.psb");
    let mut images = vec![("cover", &center)];
    if let (Some(left), Some(right)) = (&left, &right) {
        images.extend([("left", left), ("right", right)]);
    }
    let cover_file = entry.join(format!("{cover_path}.m"));
    let thumb_file = entry.join(format!("{thumb_path}.m"));
    let cover_bytes = pack(&cover_file, &image_psb(&images)?)?;
    let thumb_bytes = pack(&thumb_file, &image_psb(&[("thumb", &thumb)])?)?;
    let prepared = PreparedCover {
        schema: 1,
        revision,
        cover_path,
        thumb_path,
        width,
        height,
        origin_x,
        origin_y,
        front_origin_x: geometry
            .as_ref()
            .map(|g| g.front_origin_x)
            .unwrap_or(origin_x),
        front_origin_y: geometry
            .as_ref()
            .map(|g| g.front_origin_y)
            .unwrap_or(origin_y),
        left: geometry.as_ref().map(|g| g.left.clone()),
        right: geometry.as_ref().map(|g| g.right.clone()),
        cover_digest: digest(&cover_bytes),
        thumb_digest: digest(&thumb_bytes),
    };
    write_atomic(&cover_file, &cover_bytes)?;
    write_atomic(&thumb_file, &thumb_bytes)?;
    write_atomic(&marker, &serde_json::to_vec_pretty(&prepared)?)?;
    // Keep the current and previous revision only; errors never delete originals.
    for file in fs::read_dir(&runtime)? {
        let file = file?;
        let name = file.file_name();
        let name = name.to_string_lossy();
        let owned_revision = name
            .strip_prefix("cover-")
            .or_else(|| name.strip_prefix("thumb-"))
            .and_then(|name| name.strip_suffix(".image.psb.m"))
            .filter(|revision| {
                revision.len() == 32
                    && revision
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            });
        if file.file_type()?.is_file()
            && owned_revision.is_some_and(|revision| revision != prepared.revision)
            && !previous
                .as_ref()
                .is_some_and(|old| owned_revision == Some(old.revision.as_str()))
        {
            let _ = fs::remove_file(file.path());
        }
    }
    Ok(prepared)
}

/// Preserve stock SGX left/right artwork and its asymmetric coordinates before
/// the reusable template drops original game pixels. PNGs remain editable inputs.
pub fn prepare_stock_entry_assets(
    entry: &Path,
    stock_motion: &Value,
    index: usize,
) -> Result<PreparedCover, Error> {
    let geometry_file = entry.join("cover_geometry.json");
    if geometry_file.exists() {
        regular(&geometry_file)?;
        let current: Geometry = serde_json::from_slice(&fs::read(&geometry_file)?)?;
        if let (Some((legacy, _, _)), Some((corrected, left, right))) = (
            motion::legacy_stock_sleeve(stock_motion, index)?,
            motion::stock_sleeve(stock_motion, index)?,
        ) {
            // Upgrade only a proven previous stock extraction. Custom geometry
            // and edited strips remain untouched; source PNGs are never rewritten.
            if current == legacy
                && current != corrected
                && read_png(&entry.join("cover_left.png"))? == left
                && read_png(&entry.join("cover_right.png"))? == right
            {
                write_atomic(&geometry_file, &serde_json::to_vec_pretty(&corrected)?)?;
            }
        }
    } else {
        if let Some((geometry, left, right)) = motion::stock_sleeve(stock_motion, index)? {
            let left_bytes = png_bytes(&left)?;
            let right_bytes = png_bytes(&right)?;
            if entry.join("cover_left.png").exists() || entry.join("cover_right.png").exists() {
                return Err(invalid(
                    "SGX strip files exist without their geometry; original files retained",
                ));
            }
            let geometry_bytes = serde_json::to_vec_pretty(&geometry)?;
            let commit = (|| {
                write_atomic(&entry.join("cover_left.png"), &left_bytes)?;
                write_atomic(&entry.join("cover_right.png"), &right_bytes)?;
                write_atomic(&entry.join("cover_geometry.json"), &geometry_bytes)
            })();
            if let Err(error) = commit {
                // A directory fsync can fail after the descriptor is installed.
                // Keep a complete committed set; otherwise remove only newly
                // staged bytes that exactly match this attempt, allowing retry.
                if fs::read(entry.join("cover_geometry.json")).ok().as_deref()
                    != Some(&geometry_bytes)
                {
                    for (name, bytes) in [
                        ("cover_left.png", &left_bytes),
                        ("cover_right.png", &right_bytes),
                    ] {
                        let path = entry.join(name);
                        if fs::read(&path).ok().as_deref() == Some(bytes) {
                            let _ = fs::remove_file(path);
                        }
                    }
                }
                return Err(error);
            }
        }
    }
    prepare_entry_assets(entry)
}

fn stock_directory<'a>(library: &'a Path, stock: Option<&'a Path>) -> PathBuf {
    stock
        .map(Path::to_path_buf)
        .unwrap_or_else(|| library.join("templates"))
}

fn original_resource(library: &Path, stock: &Path, relative: &str) -> Result<PathBuf, Error> {
    let baseline = library.join("templates").join(relative);
    if baseline.is_file() {
        regular(&baseline)?;
        return Ok(baseline);
    }
    let source = crate::fonts::resource_path(library, stock, relative)?;
    regular(&source)?;
    // Preserve before replacing the corresponding live font/UI resource.
    write_atomic(&baseline, &fs::read(source)?)?;
    Ok(baseline)
}

fn install_fixed_motion_template(original: &Value, output: &Path) -> Result<bool, Error> {
    let fixed = motion::fixed_template(original)?;
    let packed = pack(output, &m2_psb::write(&fixed, 4)?)?;
    if fs::read(output).ok().as_deref() == Some(&packed) {
        return Ok(false);
    }
    write_atomic(output, &packed)?;
    Ok(true)
}

/// Refresh a fixed cover sheet without touching library metadata, other assets,
/// scripts or saves. The original motion PSB remains an immutable input.
pub fn prepare_fixed_motion_template(original: &Path, output: &Path) -> Result<bool, Error> {
    regular(original)?;
    let original = m2_psb::read(&templates::load_psb_m(original)?)?;
    install_fixed_motion_template(&original, output)
}

pub fn prepare_library_runtime_assets(
    library_root: &Path,
    game_root: &Path,
    stock_root: Option<&Path>,
) -> Result<AssetReport, Error> {
    let stock = stock_directory(library_root, stock_root);
    let variant = ConsoleVariant::from_directory(&stock)?;
    let library = library::load(library_root)?;
    let mut report = AssetReport::default();
    // Recover original sleeve pixels by declared retail tag, never edited order.
    let top = m2_psb::read(&templates::load_psb_m(&stock.join(variant.templates()[1]))?)?.to_json();
    for (lineup, sheet) in [
        ("jp", variant.templates()[2]),
        ("us", variant.templates()[3]),
    ] {
        let decoded = templates::load_psb_m(&stock.join(sheet))?;
        let original = m2_psb::read(&decoded)?;
        let start = if lineup == "us" { 50 } else { 0 };
        let items = top
            .get("items")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| invalid("Original catalogue lacks items"))?;
        for current in [&library.jp, &library.us] {
            for entry in &current.root_entries {
                let games = match entry {
                    LineupEntry::Game(g) => std::slice::from_ref(g),
                    LineupEntry::Folder(f) => &f.games,
                };
                for game in games.iter().filter(|g| g.data.display.ccolor == 3) {
                    let tag = game
                        .data
                        .rom
                        .region_tag
                        .as_deref()
                        .unwrap_or(&game.dir_name);
                    if let Some(item) = items
                        .iter()
                        .skip(start)
                        .take(50)
                        .find(|i| i["regionTag"].as_str() == Some(tag))
                    {
                        if let Some(index) = item["image"].as_u64() {
                            prepare_stock_entry_assets(
                                &game.source_dir,
                                &original,
                                index as usize,
                            )?;
                        }
                    }
                }
            }
        }
        let output = game_root.join(format!("system/motion/chronos_titleselect_{lineup}.psb.m"));
        if install_fixed_motion_template(&original, &output)? {
            report.templates_prepared += 1;
        }
    }
    // Patch the original inner case, preserving its frame/stencil/transforms.
    let ui_path = original_resource(library_root, &stock, "system/motion/titleselect_ui.psb.m")?;
    let mut ui = m2_psb::read(&templates::load_psb_m(&ui_path)?)?;
    motion::center_inner_cases(&mut ui)?;
    let output = game_root.join("system/motion/titleselect_ui.psb.m");
    let packed = pack(&output, &m2_psb::write(&ui, 4)?)?;
    if fs::read(&output).ok().as_deref() != Some(&packed) {
        write_atomic(&output, &packed)?;
        report.templates_prepared += 1;
    }
    let clear = RgbaImage::new(1, 1);
    let output = game_root.join(format!("{TRANSPARENT_PATH}.m"));
    let bytes = pack(&output, &image_psb(&[("clear", &clear)])?)?;
    if fs::read(&output).ok().as_deref() != Some(&bytes) {
        write_atomic(&output, &bytes)?;
    }
    let back = library_root.join(BACK_DIRECTORY);
    safe_directory(&back)?;
    if !back.join("cover.png").exists() {
        write_atomic(&back.join("cover.png"), &png_bytes(&back_image())?)?;
    }
    prepare_entry_assets(&back)?;
    for lineup in [&library.jp, &library.us] {
        for entry in &lineup.root_entries {
            match entry {
                LineupEntry::Game(g) => {
                    prepare_entry_assets(&g.source_dir)?;
                    report.covers_prepared += 1;
                }
                LineupEntry::Folder(f) => {
                    prepare_entry_assets(&f.source_dir)?;
                    report.covers_prepared += 1;
                    for game in &f.games {
                        prepare_entry_assets(&game.source_dir)?;
                        report.covers_prepared += 1;
                    }
                }
            }
        }
    }
    report.fonts_prepared = prepare_runtime_fonts(library_root, game_root, Some(&stock))?;
    Ok(report)
}

fn back_image() -> RgbaImage {
    let mut image = RgbaImage::from_pixel(240, 240, Rgba([20, 20, 30, 255]));
    for x in 0..240 {
        for t in 0..3 {
            image.put_pixel(x, t, Rgba([180, 180, 200, 255]));
            image.put_pixel(x, 239 - t, Rgba([180, 180, 200, 255]));
        }
    }
    for y in 0..240 {
        for t in 0..3 {
            image.put_pixel(t, y, Rgba([180, 180, 200, 255]));
            image.put_pixel(239 - t, y, Rgba([180, 180, 200, 255]));
        }
    }
    for y in 60..180 {
        let delta = (y as i32 - 120).unsigned_abs();
        for x in 50 + delta..110 {
            image.put_pixel(x, y, Rgba([220, 220, 230, 255]));
        }
    }
    for y in 100..140 {
        for x in 110..190 {
            image.put_pixel(x, y, Rgba([220, 220, 230, 255]));
        }
    }
    image
}

/// Glyph-set cache prevents bitmap regeneration on ordinary title edits whose
/// characters already exist. Genres/settings/folder names participate too.
pub fn prepare_runtime_fonts(
    library_root: &Path,
    game_root: &Path,
    stock_root: Option<&Path>,
) -> Result<usize, Error> {
    // Different editor workers may update games and genre settings together.
    // Read the glyph snapshot under the same lock as its final installation so
    // a slower, older generation cannot replace the newest set of characters.
    static FONT_UPDATE: Mutex<()> = Mutex::new(());
    let _font_update = FONT_UPDATE.lock().map_err(|_| {
        invalid("The previous font preparation stopped unexpectedly; reopen the editor")
    })?;
    let stock = stock_directory(library_root, stock_root);
    let library = library::load(library_root)?;
    let mut chars = BTreeSet::new();
    chars.extend("BACK Back Retour 戻る".chars());
    for lineup in [&library.jp, &library.us] {
        for entry in &lineup.root_entries {
            let games = match entry {
                LineupEntry::Game(g) => std::slice::from_ref(g),
                LineupEntry::Folder(f) => {
                    chars.extend(f.display_name.chars());
                    &f.games
                }
            };
            for game in games {
                for text in [
                    &game.data.display.name,
                    &game.data.display.name_eng,
                    &game.data.display.tname,
                ] {
                    chars.extend(text.chars());
                }
            }
        }
    }
    fn json_strings(value: &serde_json::Value, chars: &mut BTreeSet<char>) {
        match value {
            serde_json::Value::String(s) => chars.extend(s.chars()),
            serde_json::Value::Array(a) => {
                for v in a {
                    json_strings(v, chars);
                }
            }
            serde_json::Value::Object(o) => {
                for v in o.values() {
                    json_strings(v, chars);
                }
            }
            _ => {}
        }
    }
    let settings = library_root.join("settings_games.json");
    if settings.is_file() {
        json_strings(&serde_json::from_slice(&fs::read(settings)?)?, &mut chars);
    }
    chars.retain(|c| !c.is_control());
    static FONT_DIGEST: OnceLock<String> = OnceLock::new();
    let mut key = Md5::new();
    key.update("chronos-fonts-v2-1");
    key.update(FONT_DIGEST.get_or_init(|| digest(crate::fonts::FONT_BYTES)));
    key.update(chars.iter().collect::<String>().as_bytes());
    let mut inputs = Vec::new();
    for &relative in crate::fonts::RESOURCES.iter().take(3) {
        let path = original_resource(library_root, &stock, relative)?;
        let bytes = fs::read(&path)?;
        key.update(&bytes);
        inputs.push((relative, bytes));
    }
    let revision = save_format::digest_hex(&key.finalize().into());
    let marker = game_root.join("system/font/chronos-fonts.json");
    let old: Option<serde_json::Value> = fs::read(&marker)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    if old.as_ref().is_some_and(|old| {
        old["revision"].as_str() == Some(&revision)
            && inputs.iter().all(|(relative, _)| {
                fs::read(game_root.join(relative)).is_ok_and(|bytes| {
                    old["files"][*relative].as_str() == Some(digest(&bytes).as_str())
                })
            })
    }) {
        return Ok(0);
    }
    let mut ready = Vec::new();
    let mut hashes = serde_json::Map::new();
    for ((relative, raw), size) in inputs.iter().zip([24, 18, 32]) {
        let path = game_root.join(relative);
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("Invalid font filename"))?;
        let decoded = m2_mzs::unpack_default(raw, name)?;
        let generated = crate::fonts::generate(&decoded, &chars, size)?;
        let bytes = pack(&path, &generated)?;
        hashes.insert(
            (*relative).into(),
            serde_json::Value::String(digest(&bytes)),
        );
        ready.push((path, bytes));
    }
    for (path, bytes) in &ready {
        write_atomic(path, bytes)?;
    }
    write_atomic(
        &game_root.join("system/font/NotoSansCJK-OFL.txt"),
        crate::fonts::FONT_LICENSE.as_bytes(),
    )?;
    write_atomic(
        &marker,
        &serde_json::to_vec_pretty(
            &serde_json::json!({"schema":1,"revision":revision,"files":hashes}),
        )?,
    )?;
    Ok(ready.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn individual_assets_are_revisioned_center_only_and_keep_previous_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path();
        let image = RgbaImage::from_pixel(240, 240, Rgba([231, 17, 39, 255]));
        image.save(entry.join("cover.png")).unwrap();
        let first = prepare_entry_assets(entry).unwrap();
        assert!(first.left.is_none());
        let path = entry.join(format!("{}.m", first.cover_path));
        let before = fs::read(&path).unwrap();
        assert_eq!(first, prepare_entry_assets(entry).unwrap());
        assert_eq!(fs::read(&path).unwrap(), before);
        let raw = templates::load_psb_m(&path).unwrap();
        let tree = m2_psb::read(&raw).unwrap();
        let json = tree.to_json();
        assert_eq!(json["imageList"][0]["width"], 240);
        fs::write(entry.join("cover.png"), b"bad PNG").unwrap();
        assert!(prepare_entry_assets(entry).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            serde_json::from_slice::<PreparedCover>(
                &fs::read(entry.join("runtime/assets.json")).unwrap()
            )
            .unwrap(),
            first
        );
    }
    #[test]
    fn rgba_chunks_and_thumbnail_do_not_include_sleeve_strips() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path();
        let center = RgbaImage::from_pixel(240, 240, Rgba([255, 0, 0, 255]));
        center.save(entry.join("cover.png")).unwrap();
        let left = RgbaImage::from_pixel(20, 240, Rgba([0, 0, 0, 255]));
        left.save(entry.join("cover_left.png")).unwrap();
        let right = RgbaImage::from_pixel(12, 240, Rgba([0, 0, 255, 255]));
        right.save(entry.join("cover_right.png")).unwrap();
        let geometry = Geometry {
            front_origin_x: 116.0,
            front_origin_y: 120.0,
            left: Surface {
                label: "left".into(),
                width: 20,
                height: 240,
                origin_x: 136.0,
                origin_y: 120.0,
            },
            right: Surface {
                label: "right".into(),
                width: 12,
                height: 240,
                origin_x: -124.0,
                origin_y: 120.0,
            },
        };
        fs::write(
            entry.join("cover_geometry.json"),
            serde_json::to_vec(&geometry).unwrap(),
        )
        .unwrap();
        let asset = prepare_entry_assets(entry).unwrap();
        assert_eq!(asset.front_origin_x, 116.0);
        assert_eq!(asset.origin_x, 120.0);
        assert_eq!(
            f64::from(asset.left.as_ref().unwrap().width) - asset.left.as_ref().unwrap().origin_x,
            -asset.front_origin_x,
        );
        assert_eq!(
            -asset.right.as_ref().unwrap().origin_x,
            f64::from(asset.width) - asset.front_origin_x,
        );
        let tree = m2_psb::read(
            &templates::load_psb_m(&entry.join(format!("{}.m", asset.cover_path))).unwrap(),
        )
        .unwrap();
        let Value::Object(root) = tree else { panic!() };
        let Value::Array(list) = &root["imageList"] else {
            panic!()
        };
        assert_eq!(list.len(), 3);
        let thumb = m2_psb::read(
            &templates::load_psb_m(&entry.join(format!("{}.m", asset.thumb_path))).unwrap(),
        )
        .unwrap();
        assert_eq!(thumb.to_json()["imageList"][0]["width"], 48);
        assert_eq!(thumb.to_json()["imageList"][0]["label"], "thumb");
    }

    #[test]
    fn custom_sgx_has_a_neutral_notched_sleeve_without_changing_source_art() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path();
        let center = RgbaImage::from_pixel(240, 240, Rgba([231, 17, 39, 255]));
        center.save(entry.join("cover.png")).unwrap();
        fs::write(
            entry.join("game.json"),
            br#"{"rom":{"arch":"tg16sgx"},"display":{"ccolor":3,"csize":1}}"#,
        )
        .unwrap();
        let source = fs::read(entry.join("cover.png")).unwrap();
        let asset = prepare_entry_assets(entry).unwrap();
        assert_eq!(asset.front_origin_x, 116.0);
        assert_eq!(asset.origin_x, 120.0);
        assert_eq!(asset.left.as_ref().unwrap().width, 20);
        assert_eq!(asset.left.as_ref().unwrap().origin_x, 136.0);
        assert_eq!(asset.right.as_ref().unwrap().width, 12);
        assert_eq!(asset.right.as_ref().unwrap().origin_x, -124.0);
        assert!(!entry.join("cover_geometry.json").exists());
        assert_eq!(fs::read(entry.join("cover.png")).unwrap(), source);
        let tree = m2_psb::read(
            &templates::load_psb_m(&entry.join(format!("{}.m", asset.cover_path))).unwrap(),
        )
        .unwrap();
        let Value::Object(root) = tree else { panic!() };
        let Value::Array(entries) = &root["imageList"] else {
            panic!()
        };
        let Value::Object(left) = &entries[1] else {
            panic!()
        };
        let Value::Array(parts) = &left["texture"] else {
            panic!()
        };
        let Value::Object(part) = &parts[0] else {
            panic!()
        };
        let Value::Object(image) = &part["image"] else {
            panic!()
        };
        let Value::Stream(pixel) = &image["pixel"] else {
            panic!()
        };
        assert_eq!(
            &pixel.data[(120 * 20) * 4..(120 * 20) * 4 + 4],
            &[0, 0, 0, 255]
        );
        assert_eq!(&pixel.data[..4], &[96, 96, 104, 255]);
    }
}
