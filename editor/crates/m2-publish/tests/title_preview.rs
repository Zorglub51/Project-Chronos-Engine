//! Golden strips captured from the original ARM32 M2 engine in the Linux VM.
//! These crops contain only our Noto text and a flat background, no M2 artwork.
use indexmap::IndexMap;
use m2_psb::{Stream, Value};
fn obj(items: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(items.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn n(v: i64) -> Value {
    Value::Int(v)
}
fn s(v: &str) -> Value {
    Value::String(v.into())
}

fn flat_strip(style: u32, color: [u8; 4]) -> Vec<u8> {
    flat_strip_with_tint(style, color, None)
}

fn flat_strip_with_tint(style: u32, color: [u8; 4], tint: Option<(u32, Option<u32>)>) -> Vec<u8> {
    let mut content = obj([
        ("src", s("tex")),
        ("icon", s("bar")),
        ("zx", n(1280)),
        ("zy", n(42)),
    ]);
    if let (Some((color, mode)), Value::Object(fields)) = (tint, &mut content) {
        fields.insert("color".into(), n(color.into()));
        if let Some(mode) = mode {
            fields.insert("bm".into(), n(mode.into()));
        }
    }
    let layer = obj([
        ("type", n(0)),
        ("frameList", Value::Array(vec![obj([("content", content)])])),
        ("children", Value::Array(vec![])),
    ]);
    let motions = Value::Object(IndexMap::from([(
        format!("button_title{style:02}"),
        obj([("layer", Value::Array(vec![layer]))]),
    )]));
    let icon = obj([
        ("left", n(0)),
        ("top", n(0)),
        ("width", n(1)),
        ("height", n(1)),
        ("originX", Value::Float(0.5)),
        ("originY", Value::Float(0.5)),
    ]);
    let texture = obj([
        ("width", n(1)),
        ("height", n(1)),
        ("type", s("RGBA8")),
        (
            "pixel",
            Value::Stream(Stream {
                index: 0,
                data: color.to_vec(),
            }),
        ),
    ]);
    m2_psb::write(
        &obj([
            (
                "object",
                obj([("common_parts", obj([("motion", motions)]))]),
            ),
            (
                "source",
                obj([(
                    "tex",
                    obj([("texture", texture), ("icon", obj([("bar", icon)]))]),
                )]),
            ),
        ]),
        4,
    )
    .unwrap()
}

#[test]
fn namcot_default_color_weight_matches_native_instead_of_darkening_the_tip() {
    // The native NAMCOT tip is RGB(156,17,43) from an RGB(157,17,43)
    // texture and a 0x7f7f7fff weight. Explicit bm=0 is a different mode.
    for (mode, expected) in [
        (None, [156, 17, 43, 255]),
        (Some(1), [156, 17, 43, 255]),
        (Some(0), [78, 8, 21, 255]),
    ] {
        let ui = flat_strip_with_tint(12, [157, 17, 43, 255], Some((0x7f7f7fff, mode)));
        let image = m2_publish::title_preview::background(&ui, 12).unwrap();
        assert_eq!(image.get_pixel(640, 32).0, expected, "mode: {mode:?}");
    }
}

#[test]
fn native_title_pixels_match_including_compression_centering_and_kanji() {
    let font = m2_psb::write(&obj([]), 4).unwrap();
    for (title, style, color, png) in [
        (
            "The Kung Fu",
            4,
            [255, 255, 255, 255],
            include_bytes!("fixtures/noto-latin.png").as_slice(),
        ),
        (
            "悪魔城ドラキュラＸ 魍魎戦記MADARA",
            4,
            [255, 255, 255, 255],
            include_bytes!("fixtures/noto-kanji.png").as_slice(),
        ),
        (
            "銀河婦警伝説サファイア イースIV 悪魔城ドラキュラＸ 魍魎戦記MADARA 天外魔境II",
            9,
            [221, 98, 67, 255],
            include_bytes!("fixtures/noto-long.png").as_slice(),
        ),
        (
            "ニュートピアII",
            0,
            [255, 255, 255, 255],
            include_bytes!("fixtures/noto-none.png").as_slice(),
        ),
    ] {
        let p = m2_publish::title_preview::render(&flat_strip(style, color), &font, title, style)
            .unwrap();
        let crop = image::imageops::crop_imm(&p.image, 290, 14, 700, 36).to_image();
        let golden = image::load_from_memory(png).unwrap().to_rgba8();
        let differences = crop
            .pixels()
            .zip(golden.pixels())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(differences, 0, "Native title differs: {title}");
    }
}

#[test]
fn unsupported_titles_report_errors_instead_of_showing_a_false_preview() {
    let font = m2_psb::write(&obj([]), 4).unwrap();
    let ui = flat_strip(0, [255; 4]);
    for text in ["\u{10ffff}", "𠮷", "first\nsecond", "#{unknown,1}"] {
        assert!(m2_publish::title_preview::render(&ui, &font, text, 0).is_err());
    }
    assert!(m2_publish::title_preview::render(&ui, &font, "title", 13).is_err());
}
