//! Preserve native layer topology while replacing game-image frame lookups.
use super::{
    invalid, Geometry, Surface, FULL_COVER_SLOTS, SOURCE_NAME, THUMBNAIL_PIXELS, THUMBNAIL_SLOTS,
};
use crate::Error;
use image::RgbaImage;
use indexmap::IndexMap;
use m2_psb::Value;
use std::collections::{BTreeMap, BTreeSet};

fn object(v: &Value) -> Result<&IndexMap<String, Value>, Error> {
    if let Value::Object(v) = v {
        Ok(v)
    } else {
        Err(invalid("Expected a native motion object"))
    }
}
fn object_mut(v: &mut Value) -> Result<&mut IndexMap<String, Value>, Error> {
    if let Value::Object(v) = v {
        Ok(v)
    } else {
        Err(invalid("Expected a native motion object"))
    }
}
fn array(v: &Value) -> Result<&[Value], Error> {
    if let Value::Array(v) = v {
        Ok(v)
    } else {
        Err(invalid("Expected a native motion array"))
    }
}
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a Value, Error> {
    object(v)?
        .get(k)
        .ok_or_else(|| invalid(format!("Missing native motion field: {k}")))
}
fn field_mut<'a>(v: &'a mut Value, k: &str) -> Result<&'a mut Value, Error> {
    object_mut(v)?
        .get_mut(k)
        .ok_or_else(|| invalid(format!("Missing native motion field: {k}")))
}
fn text(v: &Value) -> Option<&str> {
    if let Value::String(v) = v {
        Some(v)
    } else {
        None
    }
}
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(n) if n.is_finite() => Some(*n),
        _ => None,
    }
}
fn obj(values: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(values.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn int(n: usize) -> Value {
    Value::Int(n as i64)
}
fn empty(time: usize) -> Value {
    obj([("time", int(time)), ("type", int(0))])
}
fn image_frames(count: usize, icon: impl Fn(usize) -> String, scale: Option<f64>) -> Value {
    let mut frames = Vec::new();
    for index in 0..count {
        let mut content = IndexMap::from([
            ("src".into(), Value::String(SOURCE_NAME.into())),
            ("icon".into(), Value::String(icon(index))),
            ("mask".into(), int(if scale.is_some() { 96 } else { 0 })),
        ]);
        if let Some(scale) = scale {
            content.insert("zx".into(), Value::Float(scale));
            content.insert("zy".into(), Value::Float(scale));
        }
        frames.push(obj([
            ("time", int(index)),
            ("type", int(2)),
            ("content", Value::Object(content)),
        ]));
    }
    frames.push(empty(count));
    Value::Array(frames)
}

fn parameters(motion: &mut Value, count: usize) -> Result<(), Error> {
    let root = object_mut(motion)?;
    root.insert("lastTime".into(), int(count));
    root.insert(
        "parameter".into(),
        Value::Array(vec![obj([
            ("discretization", int(1)),
            ("division", int(count.saturating_sub(1))),
            ("enabled", int(1)),
            ("id", Value::String("pkg".into())),
            ("rangeBegin", int(0)),
            ("rangeEnd", int(count.saturating_sub(1))),
        ])]),
    );
    Ok(())
}
fn topology(motion: &mut Value, layers: Vec<Value>, count: usize) -> Result<(), Error> {
    let mut names = IndexMap::new();
    fn enumerate(layer: &Value, names: &mut IndexMap<String, Value>) -> Result<(), Error> {
        let label =
            text(field(layer, "label")?).ok_or_else(|| invalid("Invalid native layer label"))?;
        if names.contains_key(label) {
            return Err(invalid("Duplicate native layer label"));
        }
        let index = names.len();
        names.insert(label.into(), int(index));
        for child in array(field(layer, "children")?)? {
            enumerate(child, names)?;
        }
        Ok(())
    }
    for layer in &layers {
        enumerate(layer, &mut names)?;
    }
    let priority = Value::Array((0..names.len()).rev().map(int).collect());
    let root = object_mut(motion)?;
    root.insert("layer".into(), Value::Array(layers));
    root.insert("layerIndexMap".into(), Value::Object(names));
    root.insert(
        "priority".into(),
        Value::Array(vec![
            obj([("content", priority), ("time", int(0)), ("type", int(1))]),
            obj([
                ("content", Value::Null),
                ("time", int(count)),
                ("type", int(1)),
            ]),
        ]),
    );
    parameters(motion, count)
}
fn strip_layer(template: &Value, label: &str, kind: &str) -> Result<Value, Error> {
    let mut layer = template.clone();
    let obj = object_mut(&mut layer)?;
    obj.insert("children".into(), Value::Array(Vec::new()));
    obj.insert("label".into(), Value::String(label.into()));
    obj.insert("type".into(), int(0));
    obj.insert(
        "frameList".into(),
        image_frames(FULL_COVER_SLOTS, |i| format!("slot_{i}_{kind}"), None),
    );
    Ok(layer)
}

pub(super) fn fixed_template(original: &Value) -> Result<Value, Error> {
    let mut tree = original.clone();
    let motions = object_mut(field_mut(
        field_mut(field_mut(&mut tree, "object")?, "pkg")?,
        "motion",
    )?)?;
    let mut front = motions
        .get("front")
        .ok_or_else(|| invalid("Original cover resource lacks pkg/front"))?
        .clone();
    let original_layers = array(field(&front, "layer")?)?.to_vec();
    let first = original_layers
        .first()
        .ok_or_else(|| invalid("Empty native front motion"))?
        .clone();
    let children = array(field(&first, "children")?)?;
    let child = |name| {
        children
            .iter()
            .find(|v| object(v).ok().and_then(|o| o.get("label")).and_then(text) == Some(name))
            .unwrap_or(&first)
    };
    let mut main = first.clone();
    let main_obj = object_mut(&mut main)?;
    main_obj.insert(
        "frameList".into(),
        image_frames(FULL_COVER_SLOTS, |i| format!("slot_{i}_cover"), None),
    );
    main_obj.insert(
        "children".into(),
        Value::Array(vec![
            strip_layer(child("R"), "R", "right")?,
            strip_layer(child("L"), "L", "left")?,
        ]),
    );
    let mut plus = original_layers
        .iter()
        .find(|v| object(v).ok().and_then(|o| o.get("label")).and_then(text) == Some("plus"))
        .unwrap_or(&first)
        .clone();
    let plus_obj = object_mut(&mut plus)?;
    plus_obj.insert("label".into(), Value::String("plus".into()));
    plus_obj.insert("children".into(), Value::Array(Vec::new()));
    plus_obj.insert(
        "frameList".into(),
        Value::Array(vec![empty(0), empty(FULL_COVER_SLOTS)]),
    );
    topology(&mut front, vec![main, plus], FULL_COVER_SLOTS)?;
    // SG must have a matching empty-child map. Cloning front's R/L tree into
    // an SG motion whose index map has one entry crashes the native renderer.
    let mut center = motions.get("sg").unwrap_or(&front).clone();
    let mut layer = array(field(&center, "layer")?)?
        .first()
        .ok_or_else(|| invalid("Empty SG motion"))?
        .clone();
    let o = object_mut(&mut layer)?;
    o.insert("children".into(), Value::Array(Vec::new()));
    o.insert(
        "frameList".into(),
        image_frames(FULL_COVER_SLOTS, |i| format!("slot_{i}_center"), None),
    );
    topology(&mut center, vec![layer], FULL_COVER_SLOTS)?;
    let mut thumb = motions
        .get("thumb")
        .ok_or_else(|| invalid("Original resource lacks pkg/thumb"))?
        .clone();
    // Native bitmap layers have a different attribute set from nested-motion
    // layers (notably objTriPriority). Copy a proven bitmap layer instead of
    // changing the old nested thumbnail's type and leaving its schema mixed.
    let mut layer = array(field(&center, "layer")?)?
        .first()
        .ok_or_else(|| invalid("Empty center bitmap motion"))?
        .clone();
    let o = object_mut(&mut layer)?;
    o.insert("label".into(), Value::String("front".into()));
    o.insert("children".into(), Value::Array(Vec::new()));
    o.insert("type".into(), int(0));
    o.insert(
        "frameList".into(),
        image_frames(
            THUMBNAIL_SLOTS,
            |i| format!("thumb_{i}"),
            Some(240.0 * 0.115 / f64::from(THUMBNAIL_PIXELS)),
        ),
    );
    topology(&mut thumb, vec![layer], THUMBNAIL_SLOTS)?;
    motions.insert("front".into(), front);
    motions.insert("sg".into(), center.clone());
    motions.insert("center".into(), center);
    motions.insert("thumb".into(), thumb);
    // These three retail motions assemble artwork already flattened into the
    // editor's canonical PNG. Keeping them would retain the old cover atlas.
    for name in ["soft31", "soft32", "soft33"] {
        motions.shift_remove(name);
    }
    let mut used: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    fn references(v: &Value, used: &mut BTreeMap<String, BTreeSet<String>>) -> Result<(), Error> {
        match v {
            Value::Object(o) => {
                if let Some(src) = o.get("src") {
                    let src =
                        text(src).ok_or_else(|| invalid("Unknown dynamic native image source"))?;
                    let icon = o
                        .get("icon")
                        .and_then(text)
                        .ok_or_else(|| invalid("Unknown dynamic native image icon"))?;
                    used.entry(src.into()).or_default().insert(icon.into());
                }
                for value in o.values() {
                    references(value, used)?;
                }
            }
            Value::Array(a) => {
                for value in a {
                    references(value, used)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for (key, value) in object(&tree)? {
        if key != "source" {
            references(value, &mut used)?;
        }
    }
    let source = object_mut(field_mut(&mut tree, "source")?)?;
    source.retain(|name, _| used.contains_key(name));
    crate::atlas::compact_stock_atlases(&mut tree, i64::MAX);
    // Registering images requires keys absent from the static source map.
    if object(field(&tree, "source")?)?.contains_key(SOURCE_NAME) {
        return Err(invalid(
            "Dynamic image namespace collides with a stock source",
        ));
    }
    Ok(tree)
}

pub(super) fn center_inner_cases(tree: &mut Value) -> Result<(), Error> {
    let motions = object_mut(field_mut(
        field_mut(field_mut(tree, "object")?, "common_parts")?,
        "motion",
    )?)?;
    fn replace(v: &mut Value) {
        match v {
            Value::Object(o) => {
                if o.get("src").and_then(text) == Some("pkg")
                    && o.get("icon").and_then(text) == Some("front")
                {
                    o.insert("icon".into(), Value::String("center".into()));
                }
                for value in o.values_mut() {
                    replace(value);
                }
            }
            Value::Array(a) => {
                for value in a {
                    replace(value);
                }
            }
            _ => {}
        }
    }
    // SG sleeve stays unmasked/front. Ordinary regional cases are centered.
    for name in [
        "pkg",
        "pkg_us",
        "pkg_eu",
        "pkg_white",
        "pkg_wide",
        "pkg_tg16",
    ] {
        if let Some(motion) = motions.get_mut(name) {
            replace(motion);
        }
    }
    // Packaging and launch type are independent: normal HuCard labels must
    // never inherit the SGX sleeve's strips or asymmetric front offset.
    let objects = object_mut(field_mut(tree, "object")?)?;
    for name in ["bg", "bg_us", "bg_eu"] {
        if let Some(bg) = objects.get_mut(name) {
            let motions = object_mut(field_mut(bg, "motion")?)?;
            for label in ["hucard", "hucard_fade"] {
                if let Some(motion) = motions.get_mut(label) {
                    replace(motion);
                }
            }
        }
    }
    Ok(())
}

fn layer_named<'a>(motion: &'a Value, label: &str) -> Option<&'a Value> {
    fn find<'a>(layer: &'a Value, label: &str) -> Option<&'a Value> {
        if object(layer).ok()?.get("label").and_then(text) == Some(label) {
            return Some(layer);
        }
        for child in array(field(layer, "children").ok()?).ok()? {
            if let Some(value) = find(child, label) {
                return Some(value);
            }
        }
        None
    }
    for layer in array(field(motion, "layer").ok()?).ok()? {
        if let Some(value) = find(layer, label) {
            return Some(value);
        }
    }
    None
}
fn selected<'a>(layer: &'a Value, index: usize) -> Option<&'a Value> {
    let frame = array(field(layer, "frameList").ok()?)
        .ok()?
        .iter()
        .filter(|f| {
            object(f)
                .ok()
                .and_then(|o| o.get("time"))
                .and_then(number)
                .is_some_and(|n| n <= index as f64)
        })
        .last()?;
    if number(field(frame, "type").ok()?)? != 2.0 {
        return None;
    }
    field(frame, "content").ok()
}
fn extract(tree: &Value, content: &Value, label: &str) -> Result<(Surface, RgbaImage), Error> {
    let src = text(field(content, "src")?).ok_or_else(|| invalid("Invalid sleeve texture"))?;
    let icon = text(field(content, "icon")?).ok_or_else(|| invalid("Invalid sleeve icon"))?;
    let source = field(field(tree, "source")?, src)?;
    let rectangle = field(field(source, "icon")?, icon)?;
    let texture = field(source, "texture")?;
    let n = |v: &Value, key| -> Result<u32, Error> {
        let x = number(field(v, key)?).ok_or_else(|| invalid("Invalid sprite dimensions"))?;
        if x < 0.0 || x.fract() != 0.0 || x > 8192.0 {
            return Err(invalid("Invalid sprite dimensions"));
        }
        Ok(x as u32)
    };
    let (w, h, x, y, iw, ih) = (
        n(texture, "width")?,
        n(texture, "height")?,
        n(rectangle, "left")?,
        n(rectangle, "top")?,
        n(rectangle, "width")?,
        n(rectangle, "height")?,
    );
    if iw == 0
        || ih == 0
        || iw > 512
        || ih > 512
        || x.checked_add(iw).is_none_or(|n| n > w)
        || y.checked_add(ih).is_none_or(|n| n > h)
        || text(field(texture, "type")?) != Some("RGBA8")
    {
        return Err(invalid("Invalid original sleeve rectangle"));
    }
    let Value::Stream(stream) = field(texture, "pixel")? else {
        return Err(invalid("Missing sleeve RGBA pixels"));
    };
    if stream.data.len() != w as usize * h as usize * 4 {
        return Err(invalid("Invalid original sleeve pixel length"));
    }
    let mut image = RgbaImage::new(iw, ih);
    for row in 0..ih as usize {
        let from = ((y as usize + row) * w as usize + x as usize) * 4;
        image.as_mut()[row * iw as usize * 4..(row + 1) * iw as usize * 4]
            .copy_from_slice(&stream.data[from..from + iw as usize * 4]);
    }
    let coordinate = object(content)?.get("coord").and_then(|v| array(v).ok());
    let coord = |i| {
        coordinate
            .and_then(|v| v.get(i))
            .and_then(number)
            .unwrap_or(0.0)
    };
    let ox =
        number(field(rectangle, "originX")?).ok_or_else(|| invalid("Invalid sleeve origin"))?;
    let oy =
        number(field(rectangle, "originY")?).ok_or_else(|| invalid("Invalid sleeve origin"))?;
    Ok((
        Surface {
            label: label.into(),
            width: iw,
            height: ih,
            origin_x: ox - coord(0),
            origin_y: oy - coord(1),
        },
        image,
    ))
}
pub(super) fn stock_sleeve(
    tree: &Value,
    index: usize,
) -> Result<Option<(Geometry, RgbaImage, RgbaImage)>, Error> {
    stock_sleeve_with_inheritance(tree, index, true)
}

pub(super) fn legacy_stock_sleeve(
    tree: &Value,
    index: usize,
) -> Result<Option<(Geometry, RgbaImage, RgbaImage)>, Error> {
    stock_sleeve_with_inheritance(tree, index, false)
}

fn stock_sleeve_with_inheritance(
    tree: &Value,
    index: usize,
    inherit_center: bool,
) -> Result<Option<(Geometry, RgbaImage, RgbaImage)>, Error> {
    let front = field(
        field(field(field(tree, "object")?, "pkg")?, "motion")?,
        "front",
    )?;
    let content = |name| layer_named(front, name).and_then(|layer| selected(layer, index));
    let (Some(left), Some(right)) = (content("L"), content("R")) else {
        return Ok(None);
    };
    let main = content("front_00")
        .or_else(|| content("front_e"))
        .ok_or_else(|| invalid("SGX sleeve lacks center artwork"))?;
    let (center, _) = extract(tree, main, "cover")?;
    let (mut left, left_pixels) = extract(tree, left, "left")?;
    let (mut right, right_pixels) = extract(tree, right, "right")?;
    if inherit_center {
        // R/L are children of the center layer. Moving its coordinate into
        // the registration origin must flatten that inherited move into the
        // strips too; otherwise stock SGX sleeves acquire a 4/10-pixel gap.
        if let Some(Value::Array(coordinate)) = object(main)?.get("coord") {
            let x = coordinate.first().and_then(number).unwrap_or(0.0);
            let y = coordinate.get(1).and_then(number).unwrap_or(0.0);
            left.origin_x -= x;
            left.origin_y -= y;
            right.origin_x -= x;
            right.origin_y -= y;
        }
    }
    Ok(Some((
        Geometry {
            front_origin_x: center.origin_x,
            front_origin_y: center.origin_y,
            left,
            right,
        },
        left_pixels,
        right_pixels,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_sleeve_children_inherit_center_coordinates_without_a_gap() {
        for (shift, left_width, right_width) in [(4.0, 20, 12), (10.0, 26, 6)] {
            let rectangle = |left: usize, width: usize| {
                obj([
                    ("left", int(left)),
                    ("top", int(0)),
                    ("width", int(width)),
                    ("height", int(240)),
                    ("originX", Value::Float(width as f64 / 2.0)),
                    ("originY", Value::Float(120.0)),
                ])
            };
            let layer = |label: &str, icon: &str, x: f64, children: Vec<Value>| {
                obj([
                    ("label", Value::String(label.into())),
                    ("children", Value::Array(children)),
                    (
                        "frameList",
                        Value::Array(vec![obj([
                            ("time", int(0)),
                            ("type", int(2)),
                            (
                                "content",
                                obj([
                                    ("src", Value::String("stock".into())),
                                    ("icon", Value::String(icon.into())),
                                    (
                                        "coord",
                                        Value::Array(vec![
                                            Value::Float(x),
                                            Value::Float(0.0),
                                            Value::Float(0.0),
                                        ]),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ])
            };
            let pixels = RgbaImage::from_pixel(272, 240, image::Rgba([17, 31, 47, 255]));
            let tree = obj([
                (
                    "source",
                    obj([(
                        "stock",
                        obj([
                            (
                                "texture",
                                obj([
                                    ("width", int(272)),
                                    ("height", int(240)),
                                    ("type", Value::String("RGBA8".into())),
                                    (
                                        "pixel",
                                        Value::Stream(m2_psb::Stream {
                                            index: 0,
                                            data: pixels.as_raw().clone(),
                                        }),
                                    ),
                                ]),
                            ),
                            (
                                "icon",
                                obj([
                                    ("cover", rectangle(0, 240)),
                                    ("left", rectangle(240, left_width)),
                                    ("right", rectangle(240 + left_width, right_width)),
                                ]),
                            ),
                        ]),
                    )]),
                ),
                (
                    "object",
                    obj([(
                        "pkg",
                        obj([(
                            "motion",
                            obj([(
                                "front",
                                obj([(
                                    "layer",
                                    Value::Array(vec![layer(
                                        "front_00",
                                        "cover",
                                        shift,
                                        vec![
                                            layer(
                                                "R",
                                                "right",
                                                120.0 + right_width as f64 / 2.0,
                                                vec![],
                                            ),
                                            layer(
                                                "L",
                                                "left",
                                                -120.0 - left_width as f64 / 2.0,
                                                vec![],
                                            ),
                                        ],
                                    )]),
                                )]),
                            )]),
                        )]),
                    )]),
                ),
            ]);
            let (geometry, left, right) = stock_sleeve(&tree, 0).unwrap().unwrap();
            assert_eq!(geometry.front_origin_x, 120.0 - shift);
            assert_eq!(
                left_width as f64 - geometry.left.origin_x,
                -geometry.front_origin_x
            );
            assert_eq!(-geometry.right.origin_x, 240.0 - geometry.front_origin_x);
            assert_eq!(
                geometry.left.origin_x + right_width as f64 - geometry.right.origin_x,
                272.0
            );
            assert_eq!(left.get_pixel(0, 0).0, [17, 31, 47, 255]);
            assert_eq!(right.get_pixel(0, 0).0, [17, 31, 47, 255]);
        }
    }

    fn fixture() -> Value {
        let content = obj([
            ("src", Value::String("tex#000".into())),
            ("icon", Value::String("art".into())),
        ]);
        let frame = obj([("time", int(0)), ("type", int(2)), ("content", content)]);
        let layer = obj([
            ("label", Value::String("front_00".into())),
            ("children", Value::Array(vec![])),
            ("type", int(0)),
            ("objTriPriority", int(2)),
            ("frameList", Value::Array(vec![frame])),
        ]);
        let motion = obj([
            ("layer", Value::Array(vec![layer])),
            ("parameter", Value::Array(vec![])),
            ("priority", Value::Array(vec![])),
            ("layerIndexMap", obj([])),
            ("lastTime", int(1)),
        ]);
        let mut thumb = motion.clone();
        let Value::Array(layers) = field_mut(&mut thumb, "layer").unwrap() else {
            panic!()
        };
        let attributes = object_mut(&mut layers[0]).unwrap();
        attributes.insert("type".into(), int(3));
        attributes.shift_remove("objTriPriority");
        attributes.insert("motionIndependentLayerInherit".into(), int(0));
        obj([
            (
                "object",
                obj([(
                    "pkg",
                    obj([(
                        "motion",
                        obj([
                            ("front", motion.clone()),
                            ("thumb", thumb),
                            ("soft31", motion),
                        ]),
                    )]),
                )]),
            ),
            ("source", obj([("tex#000", Value::Null)])),
        ])
    }
    #[test]
    fn all_slots_have_consistent_topology_and_no_original_cover_pixels() {
        let fixed = fixed_template(&fixture()).unwrap();
        let motions = field(
            field(field(&fixed, "object").unwrap(), "pkg").unwrap(),
            "motion",
        )
        .unwrap();
        assert!(object(field(&fixed, "source").unwrap()).unwrap().is_empty());
        for name in ["center", "sg"] {
            let motion = field(motions, name).unwrap();
            assert_eq!(
                object(field(motion, "layerIndexMap").unwrap())
                    .unwrap()
                    .len(),
                1
            );
            assert!(array(
                field(
                    &array(field(motion, "layer").unwrap()).unwrap()[0],
                    "children"
                )
                .unwrap()
            )
            .unwrap()
            .is_empty());
        }
        let front = field(motions, "front").unwrap();
        assert_eq!(
            object(field(front, "layerIndexMap").unwrap())
                .unwrap()
                .len(),
            4
        );
        let thumb = field(motions, "thumb").unwrap();
        let bitmap = &array(field(thumb, "layer").unwrap()).unwrap()[0];
        assert_eq!(field(bitmap, "type").unwrap(), &int(0));
        assert_eq!(field(bitmap, "objTriPriority").unwrap(), &int(2));
        assert!(!object(bitmap)
            .unwrap()
            .contains_key("motionIndependentLayerInherit"));
        let frames = array(
            field(
                &array(field(thumb, "layer").unwrap()).unwrap()[0],
                "frameList",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(frames.len(), THUMBNAIL_SLOTS + 1);
        assert_eq!(
            text(field(field(&frames[99], "content").unwrap(), "icon").unwrap()),
            Some("thumb_99")
        );
    }
}
