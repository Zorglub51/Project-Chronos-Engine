use anyhow::{ensure, Context, Result};
use image::{imageops, RgbaImage};
use m2_psb::Value;
use std::path::Path;

fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    if let Value::Object(o) = v {
        o.get(key)
    } else {
        None
    }
}
fn array(v: &Value) -> Option<&[Value]> {
    if let Value::Array(a) = v {
        Some(a)
    } else {
        None
    }
}
fn string(v: &Value) -> Option<&str> {
    if let Value::String(s) = v {
        Some(s)
    } else {
        None
    }
}
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}
fn at<'a>(mut v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    for key in keys {
        v = get(v, key)?;
    }
    Some(v)
}

fn frame<'a>(motion: &'a Value, layer: &str, index: u64) -> Option<&'a Value> {
    let layer = array(get(motion, "layer")?)?
        .iter()
        .find(|l| get(l, "label").and_then(string) == Some(layer))?;
    array(get(layer, "frameList")?)?
        .iter()
        .find(|f| get(f, "time").and_then(num) == Some(index as f64))
        .and_then(|f| get(f, "content"))
}
fn icon(psb: &Value, content: &Value) -> Result<RgbaImage> {
    let src = get(content, "src")
        .and_then(string)
        .context("Cover frame has no texture")?;
    let name = get(content, "icon")
        .and_then(string)
        .context("Cover frame has no icon")?;
    let source = at(psb, &["source", src]).context("Missing cover texture")?;
    let texture = get(source, "texture").context("Missing cover pixels")?;
    let icon = at(source, &["icon", name]).context("Missing cover icon")?;
    let n = |v: &Value, k| {
        get(v, k)
            .and_then(num)
            .map(|n| n as u32)
            .with_context(|| format!("Missing {k}"))
    };
    let (w, h) = (n(texture, "width")?, n(texture, "height")?);
    let (x, y, iw, ih) = (
        n(icon, "left")?,
        n(icon, "top")?,
        n(icon, "width")?,
        n(icon, "height")?,
    );
    ensure!(
        w > 0
            && h > 0
            && w <= 8192
            && h <= 8192
            && iw > 0
            && ih > 0
            && x.checked_add(iw).is_some_and(|v| v <= w)
            && y.checked_add(ih).is_some_and(|v| v <= h),
        "Invalid cover rectangle"
    );
    let pixels = match get(texture, "pixel") {
        Some(Value::Stream(s)) => &s.data,
        _ => anyhow::bail!("Unsupported cover pixel format"),
    };
    ensure!(
        pixels.len() >= w as usize * h as usize * 4,
        "Truncated cover texture"
    );
    let mut out = RgbaImage::new(iw, ih);
    for row in 0..ih as usize {
        let start = ((y as usize + row) * w as usize + x as usize) * 4;
        out.as_mut()[row * iw as usize * 4..(row + 1) * iw as usize * 4]
            .copy_from_slice(&pixels[start..start + iw as usize * 4]);
    }
    Ok(out)
}
pub fn save_cover(psb: &Value, index: u64, destination: &Path) -> Result<(u32, u32)> {
    let motions = at(psb, &["object", "pkg", "motion"]).context("Missing cover motions")?;
    let front = get(motions, "front").context("Missing cover front")?;
    let composite = frame(front, "plus", index)
        .and_then(|f| get(f, "icon"))
        .and_then(string)
        .filter(|s| s.starts_with("soft"));
    let image = if let Some(name) = composite {
        let layers = get(motions, name)
            .and_then(|m| get(m, "layer"))
            .and_then(array)
            .context("Missing composite cover")?;
        let mut out = RgbaImage::new(240, 240);
        let mut count = 0;
        for layer in layers {
            let Some(frames) = get(layer, "frameList").and_then(array) else {
                continue;
            };
            let Some(content) = frames
                .iter()
                .filter_map(|f| get(f, "content"))
                .find(|c| get(c, "src").is_some() && get(c, "icon").is_some())
            else {
                continue;
            };
            let mut strip = icon(psb, content)?;
            match get(content, "angle").and_then(num).unwrap_or(0.0) as i32 {
                0 => {}
                90 => strip = imageops::rotate90(&strip),
                180 => strip = imageops::rotate180(&strip),
                270 => strip = imageops::rotate270(&strip),
                _ => anyhow::bail!("Unsupported cover strip rotation"),
            }
            let x = get(content, "coord")
                .and_then(array)
                .and_then(|a| a.first())
                .and_then(num)
                .unwrap_or(0.0);
            imageops::overlay(
                &mut out,
                &strip,
                (x + 120.0 - strip.width() as f64 / 2.0).round() as i64,
                0,
            );
            count += 1;
        }
        ensure!(count > 0, "Empty composite cover");
        out
    } else {
        icon(
            psb,
            frame(front, "front_00", index)
                .or_else(|| frame(front, "front_e", index))
                .context("Cover frame not found")?,
        )?
    };
    image.save(destination)?;
    Ok(image.dimensions())
}
