// Generate title_jp_titleselect_<lineup>.psb.m by patching a stock template.
//
// Strategy: take the stock cover sheet (e.g. 040/motion/title_jp_titleselect_jp.psb.m)
// as input, KEEP its existing source/textures (tex#000, tex#001 — referenced
// from motion/sg, motion/soft31..33, the plus layer, thumb), and ADD new
// textures starting at the next free tex#NNN index for each new cover. Replace
// only `front.layer[0].frameList` with frames pointing to the new textures,
// then bump `lastTime` / `parameter[0].{division,rangeEnd}` / `priority[1].time`
// accordingly.
//
// This matches the Project-Chronos-Editor approach (gameList.gd:_on_add_button_pressed).
// See docs/cover_sheet_format.md for the full reference.
//
// Reasoning: writing the cover sheet from scratch is brittle — the JP cover
// sheet has motion sections (sg, soft31..33, plus) whose frames reference the
// stock atlases. Removing or repurposing tex#000 / tex#001 makes m2engage
// crash with "PLEASE SHUTDOWN 003" (shutdown-detection seeing exit status 1)
// after `platform check success.`.

use crate::library::CoverSize;
use crate::Error;
use indexmap::IndexMap;
use m2_psb::{Stream, Value};

const LINEUPMAX: i64 = 50;

/// One entry the cover sheet renders. Used for both real games and folder
/// cards (folders supply their own cover.png at FOLDER<id>/cover.png).
pub struct CoverEntry {
    pub cover_path: std::path::PathBuf,
    pub cover_size: Option<CoverSize>,
    /// Human-readable label, only used for error messages.
    pub label: String,
}

impl CoverEntry {
    fn dimensions(&self) -> (u32, u32, u32, u32) {
        if let Some(cs) = &self.cover_size {
            let w = if cs.width == 0 { 240 } else { cs.width };
            let h = if cs.height == 0 { 240 } else { cs.height };
            let ox = if cs.origin_x == 0 { w / 2 } else { cs.origin_x };
            let oy = if cs.origin_y == 0 { h / 2 } else { cs.origin_y };
            (w, h, ox, oy)
        } else {
            (240, 240, 120, 120)
        }
    }
}

pub struct GenInputs<'a> {
    pub games: &'a [CoverEntry],
    /// Decoded PSB bytes of the stock template. For JP folder packs use
    /// `040/motion/title_jp_titleselect_jp.psb.m`; for US use
    /// `040/motion/title_jp_titleselect_us.psb.m`. The template's textures and
    /// motion graph are preserved verbatim — we only append new textures and
    /// rewrite `front.layer[0].frameList`.
    pub template_psb: &'a [u8],
}

pub fn generate(inputs: &GenInputs) -> Result<Vec<u8>, Error> {
    let mut tree = m2_psb::read(inputs.template_psb)?;

    // Locate the source dict + the front motion in the template.
    let (max_tex_idx, max_stream_idx) = scan_existing_indices(&tree)?;

    // For each new cover, allocate the next free tex# and stream index, build
    // its source entry with the cover RGBA embedded as a Stream.
    let new_tex_start = max_tex_idx + 1;
    let new_stream_start = max_stream_idx + 1;

    let mut new_source_entries: Vec<(String, Value)> = Vec::with_capacity(inputs.games.len());
    let mut new_frames: Vec<Value> = Vec::with_capacity(inputs.games.len() + 1);

    for (i, entry) in inputs.games.iter().enumerate() {
        let tex_idx = new_tex_start + i as i64;
        let stream_idx = new_stream_start + i as u32;
        let tex_name = format!("tex#{:03}", tex_idx);

        let (decl_w, decl_h, decl_ox, decl_oy) = entry.dimensions();
        if !entry.cover_path.exists() {
            return Err(Error::Library(format!(
                "cover image missing for {}: {}",
                entry.label,
                entry.cover_path.display()
            )));
        }
        let (rgba_bgra, png_w, png_h) = load_cover_rgba_bgra(&entry.cover_path)?;

        // Source entry — schema matches singleton/covers.gd in Project-Chronos-Editor.
        let mut tex_obj = IndexMap::new();
        tex_obj.insert("icon".into(), Value::Object({
            let mut icons = IndexMap::new();
            let mut e = IndexMap::new();
            e.insert("attr".into(), Value::Int(2));
            e.insert("left".into(), Value::Int(0));
            e.insert("top".into(), Value::Int(0));
            e.insert("width".into(), Value::Int(decl_w as i64));
            e.insert("height".into(), Value::Int(decl_h as i64));
            e.insert("originX".into(), Value::Int(decl_ox as i64));
            e.insert("originY".into(), Value::Int(decl_oy as i64));
            e.insert("metadata".into(), Value::Null);
            // editor convention: single-icon textures use "0001" as the icon name
            icons.insert("0001".into(), Value::Object(e));
            icons
        }));
        tex_obj.insert("metadata".into(), Value::Null);
        tex_obj.insert("texture".into(), Value::Object({
            let mut t = IndexMap::new();
            t.insert("ast".into(), Value::Int(0));
            t.insert("width".into(), Value::Int(png_w as i64));
            t.insert("height".into(), Value::Int(png_h as i64));
            t.insert("truncated_width".into(), Value::Int(png_w as i64));
            t.insert("truncated_height".into(), Value::Int(png_h as i64));
            t.insert("type".into(), Value::String("RGBA8".into()));
            t.insert("pixel".into(), Value::Stream(Stream {
                index: stream_idx,
                data: rgba_bgra,
            }));
            t
        }));
        tex_obj.insert("type".into(), Value::Int(0));
        new_source_entries.push((tex_name.clone(), Value::Object(tex_obj)));

        // Frame at time=i, content references icon "0001" in the new texture.
        let mut content = IndexMap::new();
        content.insert("icon".into(), Value::String("0001".into()));
        content.insert("src".into(), Value::String(tex_name));
        content.insert("mask".into(), Value::Int(0));
        let mut frame = IndexMap::new();
        frame.insert("time".into(), Value::Int(i as i64));
        frame.insert("type".into(), Value::Int(2));
        frame.insert("content".into(), Value::Object(content));
        new_frames.push(Value::Object(frame));
    }

    let n = inputs.games.len() as i64;
    let last_time = (LINEUPMAX + 2).max(n + 2);

    // Sentinel ending frame (matches editor: {time: lastTime, type: 0}).
    let mut sentinel = IndexMap::new();
    sentinel.insert("time".into(), Value::Int(last_time));
    sentinel.insert("type".into(), Value::Int(0));
    new_frames.push(Value::Object(sentinel));

    // Splice into the tree.
    splice_into_tree(&mut tree, new_source_entries, new_frames, last_time)?;

    Ok(m2_psb::write(&tree, 4)?)
}

/// Walk the existing source dict and return:
///   - the highest `tex#NNN` numeric suffix
///   - the highest stream index used by any texture's `pixel` Stream value
fn scan_existing_indices(tree: &Value) -> Result<(i64, u32), Error> {
    let root = expect_obj(tree, "root")?;
    let source = expect_obj_field(root, "source")?;

    let mut max_tex = -1i64;
    let mut max_stream: i32 = -1;
    for (k, v) in source.iter() {
        if let Some(rest) = k.strip_prefix("tex#") {
            if let Ok(idx) = rest.parse::<i64>() {
                if idx > max_tex {
                    max_tex = idx;
                }
            }
        }
        if let Value::Object(tex_obj) = v {
            if let Some(Value::Object(tex_inner)) = tex_obj.get("texture") {
                if let Some(Value::Stream(s)) = tex_inner.get("pixel") {
                    let i = s.index as i32;
                    if i > max_stream {
                        max_stream = i;
                    }
                }
            }
        }
    }
    let max_tex = if max_tex < 0 { -1 } else { max_tex };
    let max_stream = if max_stream < 0 { 0u32 } else { max_stream as u32 };
    Ok((max_tex, max_stream))
}

fn splice_into_tree(
    tree: &mut Value,
    new_source_entries: Vec<(String, Value)>,
    new_frames: Vec<Value>,
    last_time: i64,
) -> Result<(), Error> {
    let root = expect_obj_mut(tree, "root")?;

    // 1. Append new textures into source.
    let source = expect_obj_field_mut(root, "source")?;
    for (k, v) in new_source_entries {
        source.insert(k, v);
    }

    // 2. Replace front.layer[0].frameList; bump timing fields.
    let object = expect_obj_field_mut(root, "object")?;
    let pkg = expect_obj_field_mut(object, "pkg")?;
    let motion = expect_obj_field_mut(pkg, "motion")?;
    let front = expect_obj_field_mut(motion, "front")?;

    front.insert("lastTime".into(), Value::Int(last_time));

    if let Some(Value::Array(parameter)) = front.get_mut("parameter") {
        if let Some(Value::Object(p0)) = parameter.get_mut(0) {
            p0.insert("division".into(), Value::Int(last_time - 1));
            p0.insert("rangeEnd".into(), Value::Int(last_time - 1));
        }
    }
    if let Some(Value::Array(priority)) = front.get_mut("priority") {
        if let Some(Value::Object(p1)) = priority.get_mut(1) {
            p1.insert("time".into(), Value::Int(last_time));
        }
    }

    let layer = expect_array_field_mut(front, "layer")?;
    let layer0 = match layer.get_mut(0) {
        Some(Value::Object(o)) => o,
        _ => {
            return Err(Error::Template(
                "title_select template: front.layer[0] missing or not an object".into(),
            ))
        }
    };
    layer0.insert("frameList".into(), Value::Array(new_frames));

    Ok(())
}

// ---- helpers ----

fn expect_obj<'a>(v: &'a Value, ctx: &str) -> Result<&'a IndexMap<String, Value>, Error> {
    match v {
        Value::Object(o) => Ok(o),
        _ => Err(Error::Template(format!(
            "title_select template: expected object at {}",
            ctx
        ))),
    }
}

fn expect_obj_mut<'a>(
    v: &'a mut Value,
    ctx: &str,
) -> Result<&'a mut IndexMap<String, Value>, Error> {
    match v {
        Value::Object(o) => Ok(o),
        _ => Err(Error::Template(format!(
            "title_select template: expected object at {}",
            ctx
        ))),
    }
}

fn expect_obj_field<'a>(
    obj: &'a IndexMap<String, Value>,
    key: &str,
) -> Result<&'a IndexMap<String, Value>, Error> {
    match obj.get(key) {
        Some(Value::Object(o)) => Ok(o),
        _ => Err(Error::Template(format!(
            "title_select template: missing or non-object field '{}'",
            key
        ))),
    }
}

fn expect_obj_field_mut<'a>(
    obj: &'a mut IndexMap<String, Value>,
    key: &str,
) -> Result<&'a mut IndexMap<String, Value>, Error> {
    match obj.get_mut(key) {
        Some(Value::Object(o)) => Ok(o),
        _ => Err(Error::Template(format!(
            "title_select template: missing or non-object field '{}'",
            key
        ))),
    }
}

fn expect_array_field_mut<'a>(
    obj: &'a mut IndexMap<String, Value>,
    key: &str,
) -> Result<&'a mut Vec<Value>, Error> {
    match obj.get_mut(key) {
        Some(Value::Array(a)) => Ok(a),
        _ => Err(Error::Template(format!(
            "title_select template: missing or non-array field '{}'",
            key
        ))),
    }
}

/// Load an image as RGBA8 bytes for direct embedding in the PSB stream.
///
/// On this hardware the engine treats the stream as BGRA at GPU upload time,
/// but the PSB stream itself just stores raw bytes — the byte order embedded
/// here goes through unchanged. Empirically (verified on PCE Mini 1006JP)
/// no R↔B swap is needed: feeding the image crate's standard RGBA bytes
/// produces correct on-screen colors. The Project-Chronos-Editor's
/// swap_red_blue() is needed there because it routes through PIL save_png
/// + mzstool re-encode, which apparently inverts somewhere in that chain.
fn load_cover_rgba_bgra(path: &std::path::Path) -> Result<(Vec<u8>, u32, u32), Error> {
    let img = image::open(path)
        .map_err(|e| Error::Library(format!("failed to read {}: {}", path.display(), e)))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((rgba.into_raw(), w, h))
}
