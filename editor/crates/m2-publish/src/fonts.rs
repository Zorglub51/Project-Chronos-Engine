//! Shared, deterministic bitmap generation for native M2 resources and previews.
//! Only the desktop publisher reads OpenType. A33 still loads ordinary A8 PSBs.
use crate::{
    library::{Library, LineupEntry},
    templates::{load_psb_m, write_psb_m},
    Error,
};
use indexmap::IndexMap;
use m2_psb::{Stream, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

// Share one font allocation across the publisher and the desktop frontend.
// A const can embed the full font again in each consuming crate.
pub static FONT_BYTES: &[u8] = include_bytes!("../fonts/NotoSansCJKjp-Medium.otf");
pub const FONT_LICENSE: &str = include_str!("../fonts/OFL.txt");
pub const RESOURCES: &[&str] = &[
    "system/font/makoto_basefont.psb.m",
    "system/font/makoto_basefont_18pt.psb.m",
    "system/font/makoto_basefont_32pt.psb.m",
    "system/motion/titleselect_ui.psb.m",
];
const SIZES: [u32; 3] = [24, 18, 32];
// Retail fonts use these Unicode positions for controller/UI artwork.
const ICONS: &str = "ΔΕΙΚΛΜБГЖИЛПЦб─│┏┓┛┬┴";
const PAGE: u32 = 1024;

fn invalid(message: impl Into<String>) -> Error {
    Error::Template(message.into())
}
pub(crate) fn object(v: &Value) -> Result<&IndexMap<String, Value>, Error> {
    if let Value::Object(o) = v {
        Ok(o)
    } else {
        Err(invalid("Expected a PSB object"))
    }
}
pub(crate) fn array(v: &Value) -> Result<&[Value], Error> {
    if let Value::Array(a) = v {
        Ok(a)
    } else {
        Err(invalid("Expected a PSB array"))
    }
}
pub(crate) fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value, Error> {
    object(v)?
        .get(key)
        .ok_or_else(|| invalid(format!("Missing PSB field: {key}")))
}
pub(crate) fn number(v: &Value) -> Result<f64, Error> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) if f.is_finite() => Ok(*f),
        _ => Err(invalid("Expected a PSB number")),
    }
}
pub(crate) fn num(v: &Value, key: &str) -> Result<f64, Error> {
    number(field(v, key)?)
}
fn obj(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn int(n: impl Into<i64>) -> Value {
    Value::Int(n.into())
}

/// Old libraries kept only the four pack templates. Read their original game
/// resources too; publication preserves a template before replacing a live font.
pub fn resource_path(library: &Path, stock: &Path, relative: &str) -> Result<PathBuf, Error> {
    let parent = library.parent().unwrap_or(library);
    [stock.join(relative), parent.join("game").join(relative), parent.join("BACKUP/game").join(relative)]
        .into_iter().find(|p| p.is_file())
        .ok_or_else(|| invalid(format!("Original menu resource missing: {relative}. Restore it in library/templates/ from your console dump.")))
}

struct Outline(tiny_skia::PathBuilder);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Glyph {
    pub width: u32,
    pub height: u32,
    pub a: i32,
    pub pixels: Vec<u8>,
}

pub(crate) fn raster(face: &ttf_parser::Face<'_>, c: char, size: u32) -> Result<Glyph, Error> {
    // Retail M2 drops supplementary-plane UTF-8, even with a glyph in its PSB.
    // Reject it instead of publishing a title that silently loses a character.
    if c as u32 > 0xffff {
        return Err(invalid(format!(
            "The original M2 engine cannot display '{c}' (U+{:04X}, outside the Unicode BMP). Use an alternative spelling for this character.",
            c as u32
        )));
    }
    let id = face.glyph_index(c).ok_or_else(|| {
        invalid(format!(
            "Noto Sans CJK JP Medium has no character '{c}' (U+{:04X})",
            c as u32
        ))
    })?;
    let scale = size as f32 / face.units_per_em() as f32;
    let advance = (face.glyph_hor_advance(id).unwrap_or(0) as f32 * scale)
        .round()
        .max(1.0);
    let mut outline = Outline(tiny_skia::PathBuilder::new());
    let Some(bounds) = face.outline_glyph(id, &mut outline) else {
        return Ok(Glyph {
            width: advance as u32,
            height: 1,
            a: 0,
            pixels: vec![0; advance as usize],
        });
    };
    let left = (bounds.x_min as f32 * scale).floor().min(0.0);
    let top = (bounds.y_max as f32 * scale).ceil();
    let bottom = (bounds.y_min as f32 * scale).floor();
    let width = ((bounds.x_max as f32 * scale).ceil().max(advance) - left) as u32;
    let height = (top - bottom) as u32;
    let path = outline
        .0
        .finish()
        .ok_or_else(|| invalid("Empty font outline"))?;
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or_else(|| invalid("Invalid glyph size"))?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    pixmap.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::from_row(scale, 0., 0., -scale, -left, top),
        None,
    );
    Ok(Glyph {
        width,
        height,
        a: top as i32 - (size as f32 * 0.875).round() as i32,
        pixels: pixmap.data().chunks_exact(4).map(|p| p[3]).collect(),
    })
}

pub(crate) fn face() -> Result<ttf_parser::Face<'static>, Error> {
    ttf_parser::Face::parse(FONT_BYTES, 0)
        .map_err(|e| invalid(format!("Cannot read bundled Noto font: {e:?}")))
}

pub(crate) fn title_glyph(
    face: &ttf_parser::Face<'_>,
    stock: &Value,
    c: char,
) -> Result<Glyph, Error> {
    if ICONS.contains(c) {
        if let Some(code) = object(field(stock, "code")?)?.get(&c.to_string()) {
            return extract_glyph(code, array(field(stock, "source")?)?);
        }
    }
    raster(face, c, 32)
}

fn collect_text(library: &Library) -> BTreeSet<char> {
    let mut chars = BTreeSet::new();
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
                for s in [
                    &game.data.display.name,
                    &game.data.display.tname,
                    &game.data.display.name_eng,
                ] {
                    chars.extend(s.chars());
                }
            }
        }
    }
    chars.extend("BACK Back Retour 戻る".chars());
    chars.retain(|c| !c.is_control());
    chars
}

/// Replace every text glyph, retaining M2's special controller symbols. Padded
/// shelves cap every texture at 1024², including unusually large libraries.
pub fn generate(stock: &[u8], extra: &BTreeSet<char>, size: u32) -> Result<Vec<u8>, Error> {
    let original = m2_psb::read(stock)?;
    let mut root = object(&original)?.clone();
    let codes = object(field(&original, "code")?)?;
    let sources = array(field(&original, "source")?)?;
    let mut chars = extra.clone();
    for key in codes.keys() {
        chars.extend(key.chars());
    }
    chars.retain(|c| !c.is_control());
    let face = face()?;
    let mut new_codes = IndexMap::new();
    let mut new_sources = Vec::new();
    // Keep RGBA icon sources and remap their IDs; discard old text pages.
    for (i, src) in sources.iter().enumerate() {
        if field(src, "type")? == &Value::String("RGBA8".into()) {
            let target = new_sources.len();
            new_sources.push(src.clone());
            for (c, code) in codes {
                if num(code, "id")? as usize == i {
                    let mut code = object(code)?.clone();
                    code.insert("id".into(), int(target as i64));
                    new_codes.insert(c.clone(), Value::Object(code));
                }
            }
        }
    }
    let mut page = vec![0u8; (PAGE * PAGE) as usize];
    let (mut x, mut y, mut row) = (1, 1, 0);
    let mut stream_index = 1000;
    let flush = |sources: &mut Vec<Value>, page: &mut Vec<u8>, height: u32, index: u32| {
        let height = (height.div_ceil(4) * 4).min(PAGE);
        let data = page[..(PAGE * height) as usize].to_vec();
        sources.push(obj([
            ("ast", int(0)),
            ("width", int(PAGE)),
            ("height", int(height)),
            ("truncated_width", int(PAGE)),
            ("truncated_height", int(height)),
            ("type", Value::String("A8".into())),
            ("pixel", Value::Stream(Stream { index, data })),
        ]));
        page.fill(0);
    };
    for c in chars {
        if new_codes.contains_key(&c.to_string()) {
            continue;
        }
        let glyph = if ICONS.contains(c) {
            if let Some(code) = codes.get(&c.to_string()) {
                extract_glyph(code, sources)?
            } else {
                raster(&face, c, size)?
            }
        } else {
            raster(&face, c, size)?
        };
        if glyph.width + 2 > PAGE || glyph.height + 2 > PAGE {
            return Err(invalid("Font glyph exceeds texture limit"));
        }
        if x + glyph.width + 1 > PAGE {
            x = 1;
            y += row + 2;
            row = 0;
        }
        if y + glyph.height + 1 > PAGE {
            flush(&mut new_sources, &mut page, y + row + 1, stream_index);
            stream_index += 1;
            x = 1;
            y = 1;
            row = 0;
        }
        for yy in 0..glyph.height {
            let dst = ((y + yy) * PAGE + x) as usize;
            let src = (yy * glyph.width) as usize;
            page[dst..dst + glyph.width as usize]
                .copy_from_slice(&glyph.pixels[src..src + glyph.width as usize]);
        }
        let baseline = (size as f32 * 0.875).round() as i32;
        new_codes.insert(
            c.to_string(),
            obj([
                ("a", int(glyph.a)),
                ("b", int(baseline + glyph.a)),
                ("d", int(size as i32 + glyph.a)),
                ("h", int(glyph.height)),
                ("height", int(size)),
                ("id", int(new_sources.len() as i64)),
                ("w", int(glyph.width)),
                ("width", int(glyph.width)),
                ("x", int(x)),
                ("y", int(y)),
            ]),
        );
        x += glyph.width + 2;
        row = row.max(glyph.height);
    }
    flush(&mut new_sources, &mut page, y + row + 1, stream_index);
    root.insert("code".into(), Value::Object(new_codes));
    root.insert("source".into(), Value::Array(new_sources));
    m2_psb::write(&Value::Object(root), 4).map_err(Into::into)
}

pub(crate) fn extract_glyph(code: &Value, sources: &[Value]) -> Result<Glyph, Error> {
    let src = sources
        .get(num(code, "id")? as usize)
        .ok_or_else(|| invalid("Invalid font page"))?;
    if field(src, "type")? != &Value::String("A8".into()) {
        return Err(invalid("Expected an A8 font page"));
    }
    let Value::Stream(stream) = field(src, "pixel")? else {
        return Err(invalid("Missing font pixels"));
    };
    let (sw, sh) = (num(src, "width")? as u32, num(src, "height")? as u32);
    let (x, y, w, h) = (
        num(code, "x")? as u32,
        num(code, "y")? as u32,
        num(code, "w")? as u32,
        num(code, "h")? as u32,
    );
    if sw > 4096
        || sh > 4096
        || x.saturating_add(w) > sw
        || y.saturating_add(h) > sh
        || stream.data.len() != (sw * sh) as usize
    {
        return Err(invalid("Font glyph outside its texture"));
    }
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for yy in y..y + h {
        pixels.extend_from_slice(&stream.data[(yy * sw + x) as usize..(yy * sw + x + w) as usize]);
    }
    Ok(Glyph {
        width: w,
        height: h,
        a: num(code, "a")? as i32,
        pixels,
    })
}

/// Returns prepared files so missing glyphs/resources fail before publication.
pub fn prepare(
    library: &Library,
    library_root: &Path,
    stock: &Path,
) -> Result<Vec<(&'static str, Vec<u8>)>, Error> {
    let chars = collect_text(library);
    let mut files = Vec::new();
    for (&relative, size) in RESOURCES.iter().zip(SIZES) {
        let path = resource_path(library_root, stock, relative)?;
        let data = load_psb_m(&path)?;
        let generated = generate(&data, &chars, size)?;
        if !stock.join(relative).exists() {
            std::fs::create_dir_all(stock.join("system/font"))?;
            std::fs::copy(&path, stock.join(relative))?;
        }
        files.push((relative, generated));
    }
    Ok(files)
}

pub fn install(files: &[(&str, Vec<u8>)], output: &Path) -> Result<(), Error> {
    for (path, data) in files {
        write_psb_m(&output.join(path), data)?;
    }
    std::fs::write(output.join("system/font/NotoSansCJK-OFL.txt"), FONT_LICENSE)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let glyph = obj([
            ("a", int(-2)),
            ("b", int(26)),
            ("d", int(30)),
            ("h", int(2)),
            ("height", int(32)),
            ("id", int(0)),
            ("w", int(2)),
            ("width", int(2)),
            ("x", int(0)),
            ("y", int(0)),
        ]);
        m2_psb::write(
            &obj([
                (
                    "code",
                    Value::Object(IndexMap::from([
                        ("あ".into(), glyph.clone()),
                        ("Δ".into(), glyph),
                    ])),
                ),
                (
                    "source",
                    Value::Array(vec![obj([
                        ("type", Value::String("A8".into())),
                        ("width", int(2)),
                        ("height", int(2)),
                        (
                            "pixel",
                            Value::Stream(Stream {
                                index: 0,
                                data: vec![10, 20, 30, 40],
                            }),
                        ),
                    ])]),
                ),
                ("minHeight", int(32)),
                ("maxHeight", int(32)),
                ("version", Value::Float(1.08)),
            ]),
            4,
        )
        .unwrap()
    }
    #[test]
    fn replaces_existing_text_adds_kanji_and_preserves_m2_symbols() {
        let stock = fixture();
        let chars = "魍魎髙﨑".chars().collect();
        for size in [18, 24, 32] {
            let generated = generate(&stock, &chars, size).unwrap();
            assert_eq!(generated, generate(&stock, &chars, size).unwrap());
            let tree = m2_psb::read(&generated).unwrap();
            let codes = object(field(&tree, "code").unwrap()).unwrap();
            let sources = array(field(&tree, "source").unwrap()).unwrap();
            let icon = extract_glyph(&codes["Δ"], sources).unwrap();
            assert_eq!(icon.pixels, vec![10, 20, 30, 40]);
            assert_eq!(icon.a, -2);
            for c in "あ魍魎髙﨑".chars() {
                let glyph = extract_glyph(&codes[&c.to_string()], sources).unwrap();
                let expected = raster(&face().unwrap(), c, size).unwrap();
                assert_eq!(glyph.pixels, expected.pixels);
                assert_eq!(glyph.a, expected.a);
                assert!(glyph.pixels.iter().any(|&p| p > 0));
            }
        }
    }
    #[test]
    fn large_alphabets_split_into_bounded_padded_textures() {
        let chars = (0x4e00..0x5700).filter_map(char::from_u32).collect();
        let data = generate(&fixture(), &chars, 32).unwrap();
        let tree = m2_psb::read(&data).unwrap();
        let sources = array(field(&tree, "source").unwrap()).unwrap();
        assert!(sources.len() > 1);
        for src in sources {
            assert!(num(src, "width").unwrap() <= 1024.);
            assert!(num(src, "height").unwrap() <= 1024.);
        }
        for code in object(field(&tree, "code").unwrap()).unwrap().values() {
            extract_glyph(code, sources).unwrap();
            assert!(num(code, "x").unwrap() >= 1.);
            assert!(num(code, "y").unwrap() >= 1.);
        }
    }
    #[test]
    fn unsupported_characters_fail_instead_of_publishing_missing_boxes() {
        let err = generate(&fixture(), &"\u{10ffff}".chars().collect(), 32)
            .unwrap_err()
            .to_string();
        assert!(err.contains("U+10FFFF"));
        assert!(face().unwrap().glyph_index('𠮷').is_some());
        let err = raster(&face().unwrap(), '𠮷', 32).unwrap_err().to_string();
        assert!(err.contains("M2 engine"));
        assert!(err.contains("U+20BB7"));
    }
}
