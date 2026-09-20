//! Compact retained stock atlas fragments at publication time. Motions, sprite
//! names, origins and pixels stay identical; the console has less data to load.
use indexmap::IndexMap;
use m2_psb::Value;
use std::collections::{BTreeMap, BTreeSet};

type References = BTreeMap<String, BTreeSet<String>>;

fn references(value: &Value, used: &mut References) -> bool {
    match value {
        Value::Object(obj) => {
            if let Some(src) = obj.get("src") {
                let (Value::String(src), Some(Value::String(icon))) = (src, obj.get("icon")) else {
                    return false; // Unknown/dynamic reference: keep original atlases.
                };
                used.entry(src.clone()).or_default().insert(icon.clone());
            }
            obj.values().all(|v| references(v, used))
        }
        Value::Array(values) => values.iter().all(|v| references(v, used)),
        _ => true,
    }
}

fn number(obj: &IndexMap<String, Value>, key: &str) -> Option<usize> {
    match obj.get(key)? {
        Value::Int(n) => usize::try_from(*n).ok(),
        Value::Float(n)
            if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n < usize::MAX as f64 =>
        {
            Some(*n as usize)
        }
        _ => None,
    }
}

struct Sprite {
    name: String,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

/// Greedy shelves, trying power-of-two widths. One original border pixel is
/// copied around every sprite to preserve bilinear filtering at its edges.
fn layout(sprites: &[Sprite], width: usize) -> Option<(usize, Vec<(usize, usize)>)> {
    let (mut x, mut y, mut row_h) = (0, 0, 0);
    let mut positions = Vec::with_capacity(sprites.len());
    for s in sprites {
        let (w, h) = (s.w + 2, s.h + 2);
        if w > width {
            return None;
        }
        if x + w > width {
            y += row_h;
            x = 0;
            row_h = 0;
        }
        positions.push((x + 1, y + 1));
        x += w;
        row_h = row_h.max(h);
    }
    Some(((y + row_h).checked_next_power_of_two()?, positions))
}

// None means unsupported or no smaller layout; the caller keeps it verbatim.
fn compact(source: &Value, names: &BTreeSet<String>) -> Option<Value> {
    let Value::Object(src) = source else {
        return None;
    };
    let Value::Object(texture) = src.get("texture")? else {
        return None;
    };
    if texture.get("type") != Some(&Value::String("RGBA8".into()))
        || number(texture, "ast") != Some(0)
    {
        return None;
    }
    let (width, height) = (number(texture, "width")?, number(texture, "height")?);
    let Value::Stream(pixels) = texture.get("pixel")? else {
        return None;
    };
    if width == 0 || height == 0 || width.checked_mul(height)?.checked_mul(4)? != pixels.data.len()
    {
        return None;
    }
    let Value::Object(icons) = src.get("icon")? else {
        return None;
    };

    // An empty source can be dropped only after validating the known schema.
    if names.is_empty() {
        return Some(Value::Null);
    }
    let mut sprites = Vec::with_capacity(names.len());
    for name in names {
        let Value::Object(icon) = icons.get(name)? else {
            return None;
        };
        let s = Sprite {
            name: name.clone(),
            x: number(icon, "left")?,
            y: number(icon, "top")?,
            w: number(icon, "width")?,
            h: number(icon, "height")?,
        };
        if s.w == 0 || s.h == 0 || s.x.checked_add(s.w)? > width || s.y.checked_add(s.h)? > height {
            return None;
        }
        sprites.push(s);
    }
    sprites.sort_by(|a, b| b.h.cmp(&a.h).then(b.w.cmp(&a.w)).then(a.name.cmp(&b.name)));
    let mut best = None;
    let mut area = width * height;
    let mut new_w = 1;
    while new_w <= width {
        if let Some((new_h, positions)) = layout(&sprites, new_w) {
            if new_h <= height && new_w * new_h < area {
                area = new_w * new_h;
                best = Some((new_w, new_h, positions));
            }
        }
        new_w = new_w.checked_mul(2)?;
    }
    let (new_w, new_h, positions) = best?;
    let mut data = vec![0; new_w * new_h * 4];
    let mut kept = IndexMap::new();
    for (s, &(nx, ny)) in sprites.iter().zip(&positions) {
        for dy in 0..s.h + 2 {
            let sy = (s.y + dy).saturating_sub(1).min(height - 1);
            for dx in 0..s.w + 2 {
                let sx = (s.x + dx).saturating_sub(1).min(width - 1);
                let from = (sy * width + sx) * 4;
                let to = ((ny + dy - 1) * new_w + nx + dx - 1) * 4;
                data[to..to + 4].copy_from_slice(&pixels.data[from..from + 4]);
            }
        }
        let Value::Object(mut icon) = icons[&s.name].clone() else {
            return None;
        };
        for (key, coordinate) in [("left", nx), ("top", ny)] {
            let value = if matches!(icon[key], Value::Float(_)) {
                Value::Float(coordinate as f64)
            } else {
                Value::Int(coordinate as i64)
            };
            icon.insert(key.into(), value);
        }
        kept.insert(s.name.clone(), Value::Object(icon));
    }
    let mut texture = texture.clone();
    for (key, value) in [
        ("width", new_w),
        ("height", new_h),
        ("truncated_width", new_w),
        ("truncated_height", new_h),
    ] {
        texture.insert(key.into(), Value::Int(value as i64));
    }
    texture.insert(
        "pixel".into(),
        Value::Stream(m2_psb::Stream {
            index: pixels.index,
            data,
        }),
    );
    let mut src = src.clone();
    src.insert("icon".into(), Value::Object(kept));
    src.insert("texture".into(), Value::Object(texture));
    Some(Value::Object(src))
}

pub(crate) fn compact_stock_atlases(tree: &mut Value, first_new_texture: i64) {
    let Value::Object(root) = tree else { return };
    let mut used = References::new();
    for (key, value) in root.iter() {
        if key != "source" && !references(value, &mut used) {
            return;
        }
    }
    let Some(Value::Object(source)) = root.get_mut("source") else {
        return;
    };
    let names: Vec<_> = source
        .keys()
        .filter(|name| {
            name.strip_prefix("tex#")
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|i| i < first_new_texture)
        })
        .cloned()
        .collect();
    for name in names {
        let empty = BTreeSet::new();
        match compact(&source[&name], used.get(&name).unwrap_or(&empty)) {
            Some(Value::Null) => {
                source.shift_remove(&name);
            }
            Some(value) => {
                source.insert(name, value);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use m2_psb::Stream;

    fn obj(fields: Vec<(&str, Value)>) -> Value {
        Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }
    fn icon(x: i64, y: i64) -> Value {
        obj(vec![
            ("left", Value::Float(x as f64)),
            ("top", Value::Float(y as f64)),
            ("width", Value::Int(3)),
            ("height", Value::Int(2)),
            ("originX", Value::Int(2)),
            ("originY", Value::Int(1)),
            ("attr", Value::Int(2)),
        ])
    }
    fn atlas() -> Value {
        obj(vec![
            (
                "icon",
                obj(vec![("keep", icon(2, 3)), ("unused", icon(10, 10))]),
            ),
            (
                "texture",
                obj(vec![
                    ("width", Value::Int(16)),
                    ("height", Value::Int(16)),
                    ("ast", Value::Int(0)),
                    ("type", Value::String("RGBA8".into())),
                    (
                        "pixel",
                        Value::Stream(Stream {
                            index: 7,
                            data: (0..1024).map(|i| (i * 17) as u8).collect(),
                        }),
                    ),
                ]),
            ),
        ])
    }
    fn tree(content: Value) -> Value {
        let mut cover = atlas();
        let Value::Object(source) = &mut cover else {
            panic!()
        };
        let Value::Object(texture) = source.get_mut("texture").unwrap() else {
            panic!()
        };
        let Value::Stream(pixels) = texture.get_mut("pixel").unwrap() else {
            panic!()
        };
        pixels.index = 10;
        obj(vec![
            (
                "source",
                obj(vec![("tex#000", atlas()), ("tex#001", cover)]),
            ),
            (
                "object",
                obj(vec![("children", Value::Array(vec![content]))]),
            ),
        ])
    }
    fn reference() -> Value {
        obj(vec![
            ("src", Value::String("tex#000".into())),
            ("icon", Value::String("keep".into())),
        ])
    }
    fn object(v: &Value) -> &IndexMap<String, Value> {
        let Value::Object(o) = v else {
            panic!("object required")
        };
        o
    }

    #[test]
    fn keeps_nested_sprite_pixels_border_origins_and_motion_with_smaller_atlas() {
        let mut after = tree(reference());
        let before = after.clone();
        compact_stock_atlases(&mut after, 1); // tex#001 represents a new cover: untouched
        assert_eq!(object(&after)["object"], object(&before)["object"]);
        let old_sources = object(&object(&before)["source"]);
        let new_sources = object(&object(&after)["source"]);
        assert_eq!(new_sources["tex#001"], old_sources["tex#001"]);
        let old = object(&old_sources["tex#000"]);
        let new = object(&new_sources["tex#000"]);
        let icons = object(&new["icon"]);
        assert_eq!(icons.len(), 1);
        let icon = object(&icons["keep"]);
        let old_icon = object(&object(&old["icon"])["keep"]);
        for key in ["attr", "originX", "originY", "width", "height"] {
            assert_eq!(icon[key], old_icon[key]);
        }
        let texture = object(&new["texture"]);
        let Value::Stream(pixels) = &texture["pixel"] else {
            panic!()
        };
        let Value::Stream(original) = &object(&old["texture"])["pixel"] else {
            panic!()
        };
        assert!(pixels.data.len() < original.data.len());
        let (x, y, width) = (
            number(icon, "left").unwrap(),
            number(icon, "top").unwrap(),
            number(texture, "width").unwrap(),
        );
        for dy in 0..4 {
            for dx in 0..5 {
                let from = ((2 + dy) * 16 + 1 + dx) * 4;
                let to = ((y + dy - 1) * width + x + dx - 1) * 4;
                assert_eq!(&pixels.data[to..to + 4], &original.data[from..from + 4]);
            }
        }
        // PSB stream indices need not be dense after removing atlases.
        let encoded = m2_psb::write(&after, 4).unwrap();
        let decoded = m2_psb::read(&encoded).unwrap();
        let d = object(&object(&object(&decoded)["source"])["tex#000"]);
        let Value::Stream(p) = &object(&d["texture"])["pixel"] else {
            panic!()
        };
        assert_eq!(p.data, pixels.data);
        let once = after.clone();
        compact_stock_atlases(&mut after, 1);
        assert_eq!(after, once);
    }

    #[test]
    fn drops_only_unreferenced_stock_sources() {
        let mut t = tree(reference());
        compact_stock_atlases(&mut t, 2);
        let source = object(&object(&t)["source"]);
        assert!(source.contains_key("tex#000"));
        assert!(!source.contains_key("tex#001"));
    }

    #[test]
    fn unknown_or_dynamic_references_preserve_everything() {
        for content in [
            obj(vec![("src", Value::Int(3))]),
            obj(vec![("src", Value::String("tex#000".into()))]),
        ] {
            let mut t = tree(content);
            let before = t.clone();
            compact_stock_atlases(&mut t, 2);
            assert_eq!(t, before);
        }
    }

    #[test]
    fn unsupported_or_invalid_textures_are_left_untouched() {
        let keep = BTreeSet::from(["keep".into()]);
        for (key, value) in [
            ("ast", Value::Int(1)),
            ("type", Value::String("DXT5".into())),
            ("width", Value::Int(0)),
            ("height", Value::Int(8)),
        ] {
            let mut a = atlas();
            let Value::Object(o) = &mut a else { panic!() };
            let Value::Object(t) = o.get_mut("texture").unwrap() else {
                panic!()
            };
            t.insert(key.into(), value);
            assert!(compact(&a, &keep).is_none());
        }
        assert!(compact(&atlas(), &BTreeSet::from(["missing".into()])).is_none());
    }
}
