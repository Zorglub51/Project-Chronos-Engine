//! Native 1280-pixel title strip: stock motion sprites plus the very same A8
//! glyphs emitted to M2. Never ask a browser/OS text renderer to draw the title.
use crate::{
    fonts::{self, array, field, num, object},
    Error,
};
use image::{Rgba, RgbaImage};
use m2_psb::Value;

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 64;
const TITLE_Y: f64 = 15.; // 360 + PKGNAME_TEXTOFFSET_Y(169) - font offset(18) - crop top(496)
const BAR_Y: f64 = 32.; // 360 + motion offset(168) - crop top(496)

fn error(message: &str) -> Error {
    Error::Template(message.into())
}
fn optional(v: &Value, key: &str, default: f64) -> f64 {
    num(v, key).unwrap_or(default)
}
fn name(v: &Value) -> Result<&str, Error> {
    if let Value::String(s) = v {
        Ok(s)
    } else {
        Err(error("Expected sprite name"))
    }
}

fn blend(dst: &mut Rgba<u8>, src: [f64; 4]) {
    let a = src[3] / 255.;
    let old = dst[3] as f64 / 255.;
    let out = a + old * (1. - a);
    if out <= 0. {
        return;
    }
    for i in 0..3 {
        dst[i] = ((src[i] * a + dst[i] as f64 * old * (1. - a)) / out)
            .round()
            .clamp(0., 255.) as u8;
    }
    dst[3] = (out * 255.).round() as u8;
}

/// The retail titlebar motions are static sprites/nested groups at time zero.
/// Keep their true origins, tint, flips and scales, rather than stretching a
/// thumbnail of the platform badge to fill the title strip.
fn layer(out: &mut RgbaImage, layer: &Value, sources: &Value) -> Result<(), Error> {
    let frames = array(field(layer, "frameList")?)?;
    let content = field(
        frames
            .first()
            .ok_or_else(|| error("Empty titlebar motion"))?,
        "content",
    )?;
    if optional(layer, "type", 2.) == 0. {
        let src = field(sources, name(field(content, "src")?)?)?;
        let icon = field(field(src, "icon")?, name(field(content, "icon")?)?)?;
        let texture = field(src, "texture")?;
        let Value::Stream(pixels) = field(texture, "pixel")? else {
            return Err(error("Missing titlebar pixels"));
        };
        let (tw, th) = (
            num(texture, "width")? as usize,
            num(texture, "height")? as usize,
        );
        if tw > 4096
            || th > 4096
            || pixels.data.len() != tw * th * 4
            || name(field(texture, "type")?)? != "RGBA8"
        {
            return Err(error("Unsupported titlebar texture"));
        }
        let coords = object(content)?.get("coord").map(array).transpose()?;
        let cx = coords
            .and_then(|c| c.first())
            .map(fonts::number)
            .transpose()?
            .unwrap_or(0.);
        let cy = coords
            .and_then(|c| c.get(1))
            .map(fonts::number)
            .transpose()?
            .unwrap_or(0.);
        let (sx, sy) = (optional(content, "zx", 1.), optional(content, "zy", 1.));
        let (ox, oy) = (
            num(icon, "originX")? + optional(content, "ox", 0.),
            num(icon, "originY")? + optional(content, "oy", 0.),
        );
        let (iw, ih) = (num(icon, "width")?, num(icon, "height")?);
        let (flipx, flipy) = (
            optional(content, "fx", 0.) != 0.,
            optional(content, "fy", 0.) != 0.,
        );
        let (left, top) = (
            640. + cx - if flipx { (iw - ox) * sx } else { ox * sx },
            BAR_Y + cy - if flipy { (ih - oy) * sy } else { oy * sy },
        );
        let rgba = (optional(content, "color", u32::MAX as f64) as u32).to_be_bytes();
        // M2's default color mode doubles an explicit RGB weight: NAMCOT's
        // 0x7f7f7fff is nearly neutral, not a 50% darkening. Explicit bm=0
        // uses ordinary modulation (the other colored titlebar shapes).
        let color_scale = if object(content)?.contains_key("color")
            && optional(content, "bm", 1.) == 1.
        {
            2.
        } else {
            1.
        };
        let opacity = optional(content, "opa", 255.) / 255.;
        let (ix, iy) = (num(icon, "left")?, num(icon, "top")?);
        for y in (top.floor() as i32).max(0)..((top + ih * sy).ceil() as i32).min(HEIGHT as i32) {
            for x in
                (left.floor() as i32).max(0)..((left + iw * sx).ceil() as i32).min(WIDTH as i32)
            {
                let mut u = (x as f64 + 0.5 - left) / sx;
                let mut v = (y as f64 + 0.5 - top) / sy;
                if u < 0. || v < 0. || u >= iw || v >= ih {
                    continue;
                }
                if flipx {
                    u = iw - u;
                }
                if flipy {
                    v = ih - v;
                }
                let (tx, ty) = ((ix + u).floor() as usize, (iy + v).floor() as usize);
                if tx >= tw || ty >= th {
                    return Err(error("Titlebar sprite outside its atlas"));
                }
                let p = &pixels.data[(ty * tw + tx) * 4..][..4];
                blend(
                    out.get_pixel_mut(x as u32, y as u32),
                    [
                        (p[0] as f64 * rgba[0] as f64 * color_scale / 255.).min(255.),
                        (p[1] as f64 * rgba[1] as f64 * color_scale / 255.).min(255.),
                        (p[2] as f64 * rgba[2] as f64 * color_scale / 255.).min(255.),
                        p[3] as f64 * rgba[3] as f64 / 255. * opacity,
                    ],
                );
            }
        }
    }
    for child in array(field(layer, "children")?)? {
        layer_fn(out, child, sources)?;
    }
    Ok(())
}
fn layer_fn(out: &mut RgbaImage, v: &Value, sources: &Value) -> Result<(), Error> {
    layer(out, v, sources)
}

pub fn background(ui: &[u8], style: u32) -> Result<RgbaImage, Error> {
    if style > 12 {
        return Err(error("Unsupported titlebar style"));
    }
    let ui = m2_psb::read(ui)?;
    let motion = field(
        field(field(field(&ui, "object")?, "common_parts")?, "motion")?,
        &format!("button_title{style:02}"),
    )?;
    let mut out = RgbaImage::new(WIDTH, HEIGHT);
    for l in array(field(motion, "layer")?)? {
        layer(&mut out, l, field(&ui, "source")?)?;
    }
    Ok(out)
}

fn coverage(g: &fonts::Glyph, x: f64, y: f64) -> f64 {
    // Native M2 samples the already-antialiased font atlas with nearest
    // filtering, including when IndicatorSetXScale compresses a long title.
    let (x, y) = (x.round() as i32, y.round() as i32);
    if x < 0 || y < 0 || x >= g.width as i32 || y >= g.height as i32 {
        0.
    } else {
        g.pixels[(y as u32 * g.width + x as u32) as usize] as f64
    }
}

pub struct Preview {
    pub image: RgbaImage,
    pub text_width: u32,
    pub scale: f64,
}

pub fn render(ui: &[u8], font: &[u8], text: &str, style: u32) -> Result<Preview, Error> {
    if text.chars().count() > 2048 {
        return Err(error("Title is too long (maximum 2048 characters)"));
    }
    let face = fonts::face()?;
    let stock = m2_psb::read(font)?;
    let mut glyphs = Vec::new();
    let mut color = [0., 0., 0., 255.];
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' && chars.peek() == Some(&'{') {
            chars.next();
            let tag: String = chars.by_ref().take_while(|c| *c != '}').collect();
            if let Some(hex) = tag.strip_prefix("color,") {
                if hex.len() != 8 {
                    return Err(error("Invalid title color tag"));
                }
                let rgba = u32::from_str_radix(hex, 16)
                    .map_err(|_| error("Invalid title color tag"))?
                    .to_be_bytes();
                color = rgba.map(|n| n as f64);
                continue;
            }
            return Err(error("This title contains an unsupported M2 text tag"));
        }
        if c.is_control() {
            return Err(error("Use a single line for the game title"));
        }
        glyphs.push((fonts::title_glyph(&face, &stock, c)?, color));
    }
    let text_width = glyphs.iter().map(|(g, _)| g.width).sum::<u32>();
    let scale = if text_width > 700 {
        700. / text_width as f64
    } else {
        1.
    };
    let mut out = background(ui, style)?;
    let mut pen = 640. - (text_width.min(700) / 2) as f64;
    for (g, color) in glyphs {
        let top = TITLE_Y - g.a as f64;
        // The native VM's GLES rasterizer snaps quad edges to an 8-bit
        // subpixel grid before interpolating UVs. This matters at the few
        // nearest-sample boundaries in horizontally compressed titles.
        let left = (pen * 256.).round() / 256.;
        let right = ((pen + g.width as f64 * scale) * 256.).round() / 256.;
        let glyph_scale = (right - left) / g.width as f64;
        for y in
            (top.floor() as i32).max(0)..((top + g.height as f64).ceil() as i32).min(HEIGHT as i32)
        {
            for x in (left.floor() as i32).max(0)..(right.ceil() as i32).min(WIDTH as i32) {
                let alpha = coverage(
                    &g,
                    (x as f64 + 0.5 - left) / glyph_scale - 0.5,
                    y as f64 - top,
                );
                blend(
                    out.get_pixel_mut(x as u32, y as u32),
                    [color[0], color[1], color[2], color[3] * alpha / 255.],
                );
            }
        }
        pen += g.width as f64 * scale;
    }
    Ok(Preview {
        image: out,
        text_width,
        scale,
    })
}
