//! Generate a native font set and a reference title strip without publishing ROMs.
use std::{collections::BTreeSet, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(5..=6).contains(&args.len()) {
        return Err("Usage: font_preview <original-game-root> <output> <title> <style 0..12> [test-menu.psb.m]".into());
    }
    let root = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);
    let chars: BTreeSet<char> = args[3].chars().collect();
    let mut fonts = Vec::new();
    for (name, size) in m2_publish::fonts::RESOURCES.iter().zip([24, 18, 32]) {
        let bytes = std::fs::read(root.join(name))?;
        let psb = m2_publish::decode_resource(
            &bytes,
            root.join(name).file_name().unwrap().to_str().unwrap(),
        )?;
        let generated = m2_publish::fonts::generate(&psb, &chars, size)?;
        println!("{name}: {} -> {} decoded bytes", psb.len(), generated.len());
        fonts.push((*name, generated));
    }
    m2_publish::fonts::install(&fonts, &out)?;
    let tree = m2_psb::read(&fonts[2].1)?;
    std::fs::write(
        out.join("font32.json"),
        serde_json::to_vec(&tree.to_json())?,
    )?;
    if let m2_psb::Value::Object(root) = &tree {
        if let Some(m2_psb::Value::Array(sources)) = root.get("source") {
            for source in sources {
                if let m2_psb::Value::Object(source) = source {
                    if let Some(m2_psb::Value::Stream(s)) = source.get("pixel") {
                        std::fs::write(out.join(format!("stream-{}.bin", s.index)), &s.data)?;
                    }
                }
            }
        }
    }
    let ui = m2_publish::decode_resource(
        &std::fs::read(root.join("system/motion/titleselect_ui.psb.m"))?,
        "titleselect_ui.psb.m",
    )?;
    let preview = m2_publish::title_preview::render(&ui, &fonts[2].1, &args[3], args[4].parse()?)?;
    preview.image.save(out.join("title-preview.png"))?;
    // Optional isolated test catalogue. Write only to the output directory;
    // never alter the original menu or any ROM/save/script.
    if let Some(menu) = args.get(5) {
        let data = m2_publish::decode_resource(&std::fs::read(menu)?, "title_mode_top.psb.m")?;
        let mut tree = m2_psb::read(&data)?;
        if let m2_psb::Value::Object(root) = &mut tree {
            if let Some(m2_psb::Value::Array(items)) = root.get_mut("items") {
                for item in items {
                    if let m2_psb::Value::Object(o) = item {
                        o.insert("tname".into(), m2_psb::Value::String(args[3].clone()));
                        o.insert("titlebar".into(), m2_psb::Value::Int(args[4].parse()?));
                    }
                }
            }
        }
        std::fs::write(
            out.join("title_mode_top.psb.m"),
            m2_mzs::pack_default(&m2_psb::write(&tree, 4)?, "title_mode_top.psb.m")?,
        )?;
    }
    println!(
        "Title: {} pixels; scale {}",
        preview.text_width, preview.scale
    );
    Ok(())
}
