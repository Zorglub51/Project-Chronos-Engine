// Generate title_jp_titleselect_<lineup>.psb.m from the stock motion template.
// Append each cover, then replace the front and sg image-indexed tracks.
// sg supplies the SuperGrafx HuCard label during boot_sg and must follow the
// same indices as front, including the leading BACK card in folders.
//
// Keep all stock motions (thumb, plus children, soft31..33) and every sprite
// they reference. Once the cover tracks are replaced, compact their remaining
// RGBA8 atlas fragments and discard unreferenced stock textures. Sprite names,
// origins, attributes and pixels (including filtering borders) are preserved.
// Removing whole JP atlases without preserving these auxiliary sprites breaks
// the native engine. Unknown layouts are left unchanged by the compactor.

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
    /// `040/motion/title_jp_titleselect_us.psb.m`. Other motions are preserved.
    /// Append covers and rebuild `front`/`sg`,
    /// then compact referenced stock sprites without changing their pixels.
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

    crate::atlas::compact_stock_atlases(&mut tree, new_tex_start);

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

    // 2. The carousel and SuperGrafx boot label both use the item's `image`
    // as their `pkg` parameter. Rebuild both lookup tracks, sharing textures.
    let object = expect_obj_field_mut(root, "object")?;
    let pkg = expect_obj_field_mut(object, "pkg")?;
    let motion = expect_obj_field_mut(pkg, "motion")?;
    let front = expect_obj_field_mut(motion, "front")?;
    replace_image_frames(front, new_frames.clone(), last_time)?;

    // Retail US sheets have no sg motion (no stock SGX games). Their first
    // front layer has the same sprite-track schema, so use it as a one-layer
    // sg template. The shared boot_sg animation also asks for pkg/sg in bg_us.
    if !motion.contains_key("sg") {
        let mut sg = expect_obj_field(motion, "front")?.clone();
        let layers = expect_array_field_mut(&mut sg, "layer")?;
        layers.truncate(1);
        let layer = expect_obj(&layers[0], "front.layer[0]")?;
        let label = match layer.get("label") {
            Some(Value::String(label)) => label.clone(),
            _ => return Err(Error::Template("title_select: front layer label missing".into())),
        };
        sg.insert("layerIndexMap".into(), Value::Object(IndexMap::from([
            (label, Value::Int(0)),
        ])));
        sg.insert("priority".into(), Value::Array(vec![
            Value::Object(IndexMap::from([
                ("content".into(), Value::Array(vec![Value::Int(0)])),
                ("time".into(), Value::Int(0)),
                ("type".into(), Value::Int(1)),
            ])),
            Value::Object(IndexMap::from([
                ("content".into(), Value::Null),
                ("time".into(), Value::Int(last_time)),
                ("type".into(), Value::Int(1)),
            ])),
        ]));
        motion.insert("sg".into(), Value::Object(sg));
    }
    let sg = expect_obj_field_mut(motion, "sg")?;
    replace_image_frames(sg, new_frames, last_time)?;

    Ok(())
}

fn replace_image_frames(
    motion: &mut IndexMap<String, Value>,
    new_frames: Vec<Value>,
    last_time: i64,
) -> Result<(), Error> {
    motion.insert("lastTime".into(), Value::Int(last_time));

    if let Some(Value::Array(parameter)) = motion.get_mut("parameter") {
        if let Some(Value::Object(p0)) = parameter.get_mut(0) {
            p0.insert("division".into(), Value::Int(last_time - 1));
            p0.insert("rangeEnd".into(), Value::Int(last_time - 1));
        }
    }
    if let Some(Value::Array(priority)) = motion.get_mut("priority") {
        if let Some(Value::Object(p1)) = priority.get_mut(1) {
            p1.insert("time".into(), Value::Int(last_time));
        }
    }

    let layer = expect_array_field_mut(motion, "layer")?;
    let layer0 = match layer.get_mut(0) {
        Some(Value::Object(o)) => o,
        _ => {
            return Err(Error::Template(
                "title_select template: image motion layer[0] missing or not an object".into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn value(j: serde_json::Value) -> Value {
        match j {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => n.as_i64().map(Value::Int)
                .unwrap_or_else(|| Value::Float(n.as_f64().unwrap())),
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(a) => Value::Array(a.into_iter().map(value).collect()),
            serde_json::Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, value(v))).collect()),
        }
    }

    // Synthetic schema matching the stock lookup tracks; no original assets.
    fn template(with_sg: bool) -> Value {
        let track = json!({
            "lastTime": 51, "loopTime": -1,
            "parameter": [{"id":"pkg", "rangeBegin":0, "rangeEnd":50, "division":50}],
            "priority": [{"time":0,"type":1,"content":[0]}, {"time":51,"type":1,"content":null}],
            "layerIndexMap": {"front_00":0},
            "layer": [{"label":"front_00", "inheritMask":33556476, "frameList":[
                {"time":0,"type":2,"content":{"src":"front","icon":"legacy-unresolved","mask":0}},
                {"time":7,"type":2,"content":{"src":"tex#001","icon":"0045","mask":0}},
                {"time":25,"type":2,"content":{"src":"tex#000","icon":"0000","mask":0}},
                {"time":51,"type":0}
            ]}]
        });
        let mut j = json!({
            "source": {
                "tex#000": {"texture":{"pixel":null}},
                "tex#001": {"texture":{"pixel":null}}
            },
            "object":{"pkg":{"motion":{
                "front":track.clone(), "thumb":{"marker":"keep thumb"},
                "soft31":{"marker":"keep auxiliary motion"}
            }}}
        });
        if with_sg {
            j["object"]["pkg"]["motion"]["sg"] = track;
        }
        // A second carousel layer must never leak into the boot label motion.
        let front = &mut j["object"]["pkg"]["motion"]["front"];
        front["layer"].as_array_mut().unwrap().push(json!({
            "label":"plus", "frameList":[{"time":0,"type":0}]
        }));
        front["layerIndexMap"]["plus"] = json!(1);
        front["priority"][0]["content"] = json!([1,0]);
        let mut tree = value(j);
        let root = expect_obj_mut(&mut tree, "root").unwrap();
        let source = expect_obj_field_mut(root, "source").unwrap();
        for (name, index) in [("tex#000", 1), ("tex#001", 0)] {
            let tex = expect_obj_field_mut(source, name).unwrap();
            expect_obj_field_mut(tex, "texture").unwrap().insert("pixel".into(), Value::Stream(Stream {
                index, data: vec![index as u8, 20, 30, 255],
            }));
        }
        tree
    }

    struct Covers(std::path::PathBuf);
    impl Covers {
        fn new() -> Self {
            static ID: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("chronos-cover-test-{}-{}",
                std::process::id(), ID.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }
        fn entries(&self, n: usize) -> Vec<CoverEntry> {
            (0..n).map(|i| {
                let path = self.0.join(format!("{i}.png"));
                image::RgbaImage::from_pixel(2, 2, image::Rgba([i as u8, 40, 80, 255]))
                    .save(&path).unwrap();
                CoverEntry { cover_path: path, cover_size: None, label: format!("slot {i}") }
            }).collect()
        }
    }
    impl Drop for Covers {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    fn generated(template: &Value, covers: &[CoverEntry]) -> Value {
        let psb = m2_psb::write(template, 4).unwrap();
        let generated = generate(&GenInputs { games: covers, template_psb: &psb }).unwrap();
        // Check the packed artifact too: catches serialization/stream issues.
        let filename = "title_jp_titleselect_jp.psb.m";
        let packed = m2_mzs::pack_default(&generated, filename).unwrap();
        m2_psb::read(&m2_mzs::unpack_default(&packed, filename).unwrap()).unwrap()
    }

    #[test]
    fn sg_labels_follow_folder_indices_and_use_the_same_pixels_as_front() {
        let assets = Covers::new();
        let covers = assets.entries(3); // back, Daimakaimura, Aldynes
        let before = template(true);
        let after = generated(&before, &covers);
        let j = after.to_json();
        let motions = &j["object"]["pkg"]["motion"];
        let front = &motions["front"]["layer"][0]["frameList"];
        let sg = &motions["sg"]["layer"][0]["frameList"];
        for index in [1, 2] {
            assert_eq!(sg[index]["time"], index);
            assert_eq!(sg[index]["content"], front[index]["content"]);
            assert_eq!(sg[index]["content"]["src"], format!("tex#{:03}", index + 2));
        }
        assert_eq!(sg.as_array().unwrap().len(), 4); // no stale 7/25 frames
        assert_eq!(motions["sg"]["lastTime"], motions["front"]["lastTime"]);
        assert_eq!(motions["sg"]["parameter"], motions["front"]["parameter"]);
        assert_eq!(motions["sg"]["priority"][1]["time"], sg[3]["time"]);
        let original = before.to_json();
        assert_eq!(motions["thumb"], original["object"]["pkg"]["motion"]["thumb"]);
        assert_eq!(motions["soft31"], original["object"]["pkg"]["motion"]["soft31"]);
        assert_eq!(motions["front"]["layer"][1], original["object"]["pkg"]["motion"]["front"]["layer"][1]);
        let source = expect_obj_field(expect_obj(&after, "root").unwrap(), "source").unwrap();
        let old_source = expect_obj_field(expect_obj(&before, "root").unwrap(), "source").unwrap();
        assert_eq!(source.len(), 5); // two stock atlases + three covers, no SGX duplicates
        for name in ["tex#000", "tex#001"] { assert_eq!(source[name], old_source[name]); }
        for i in 0..3 {
            let tex = expect_obj_field(source, &format!("tex#{:03}", i + 2)).unwrap();
            let texture = expect_obj_field(tex, "texture").unwrap();
            let Value::Stream(stream) = &texture["pixel"] else { panic!("missing pixels") };
            assert_eq!(stream.data, [i as u8, 40, 80, 255].repeat(4));
        }
    }

    #[test]
    fn moved_games_at_late_indices_do_not_keep_stock_sg_labels() {
        let assets = Covers::new();
        let covers = assets.entries(50);
        let j = generated(&template(true), &covers).to_json();
        let sg = &j["object"]["pkg"]["motion"]["sg"];
        let frames = sg["layer"][0]["frameList"].as_array().unwrap();
        for index in [0, 7, 25, 31, 49] {
            assert_eq!(frames[index]["time"], index);
            assert_eq!(frames[index]["content"]["src"], format!("tex#{:03}", index + 2));
        }
        assert_eq!(frames[50], json!({"time":52,"type":0}));
        assert_eq!(sg["parameter"][0]["rangeEnd"], 51);
    }

    #[test]
    fn template_without_sg_gets_a_single_layer_label_track() {
        let assets = Covers::new();
        let covers = assets.entries(2);
        let j = generated(&template(false), &covers).to_json();
        let motions = &j["object"]["pkg"]["motion"];
        assert_eq!(motions["sg"]["layer"].as_array().unwrap().len(), 1);
        assert_eq!(motions["sg"]["layerIndexMap"], json!({"front_00":0}));
        assert_eq!(motions["sg"]["priority"][0]["content"], json!([0]));
        assert_eq!(motions["sg"]["layer"][0]["frameList"], motions["front"]["layer"][0]["frameList"]);
        assert_eq!(motions["front"]["layer"].as_array().unwrap().len(), 2);
    }
}
