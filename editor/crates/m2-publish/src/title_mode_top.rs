// Generate title_mode_top.psb.m for a single folder.
//
// title_mode_top is the master 200-item game list that the menu reads. Per
// the design:
//   slots   0..49   JP lineup, Japanese names
//   slots  50..99   US lineup, Japanese names
//   slots 100..149  JP lineup, English names (only `tname` differs from 0..49)
//   slots 150..199  US lineup, English names
//
// For a folder we populate ONLY its lineup's slots and stamp DUMMY across the
// other lineup. titleNum = JP-active count, titleNumTG = US-active count.

use crate::library::Game;
use crate::Error;
use indexmap::IndexMap;
use m2_psb::Value;

pub enum LineupKind {
    Jp,
    Us,
}

pub struct GenInputs<'a> {
    pub template_psb: &'a [u8],
    pub lineup: LineupKind,
    pub games: &'a [Game],
}

pub fn generate(inputs: &GenInputs) -> Result<Vec<u8>, Error> {
    let mut tree = m2_psb::read(inputs.template_psb)?;

    // Locate the items array on the root of the template
    let root = match &tree {
        Value::Object(o) => o,
        _ => return Err(Error::Template("title_mode_top: root not an object".into())),
    };
    let items = root.get("items")
        .ok_or_else(|| Error::Template("title_mode_top: missing 'items'".into()))?;
    let items = match items {
        Value::Array(a) => a,
        _ => return Err(Error::Template("title_mode_top: 'items' not an array".into())),
    };
    if items.len() != 200 {
        return Err(Error::Template(format!(
            "title_mode_top: expected 200 items in template, got {}", items.len()
        )));
    }

    // Pull a real item and a DUMMY item to use as templates
    let real_template = items[0].clone();
    let dummy_template = items.iter()
        .find(|it| match it {
            Value::Object(o) => matches!(o.get("regionTag"), Some(Value::String(s)) if s == "DUMMY"),
            _ => false,
        })
        .ok_or_else(|| Error::Template("title_mode_top: no DUMMY item in template".into()))?
        .clone();

    let n = inputs.games.len();
    if n > 50 {
        return Err(Error::Library(format!("folder has {} games, max 50 per lineup", n)));
    }

    // Build the four 50-item slot ranges
    let mut new_items: Vec<Value> = Vec::with_capacity(200);

    let (jp_count, us_count) = match inputs.lineup {
        LineupKind::Jp => (n, 0),
        LineupKind::Us => (0, n),
    };

    // Slots 0..49 — JP (Japanese names)
    for i in 0..50 {
        if i < jp_count {
            new_items.push(make_real_item(&real_template, &inputs.games[i], i, /*english=*/ false));
        } else {
            new_items.push(make_dummy(&dummy_template, i));
        }
    }
    // Slots 50..99 — US (Japanese names)
    for i in 0..50 {
        if i < us_count {
            new_items.push(make_real_item(&real_template, &inputs.games[i], i, /*english=*/ false));
        } else {
            new_items.push(make_dummy(&dummy_template, 50 + i));
        }
    }
    // Slots 100..149 — JP (English names)
    for i in 0..50 {
        if i < jp_count {
            new_items.push(make_real_item(&real_template, &inputs.games[i], i, /*english=*/ true));
        } else {
            new_items.push(make_dummy(&dummy_template, 100 + i));
        }
    }
    // Slots 150..199 — US (English names)
    for i in 0..50 {
        if i < us_count {
            new_items.push(make_real_item(&real_template, &inputs.games[i], i, /*english=*/ true));
        } else {
            new_items.push(make_dummy(&dummy_template, 150 + i));
        }
    }

    // Splice back
    if let Value::Object(o) = &mut tree {
        o.insert("items".into(), Value::Array(new_items));
        o.insert("titleNum".into(), Value::Int(jp_count as i64));
        o.insert("titleNumTG".into(), Value::Int(us_count as i64));
    }

    Ok(m2_psb::write(&tree, 4)?)
}

/// Build a real item by cloning the template and overriding game-specific fields.
fn make_real_item(template: &Value, game: &Game, slot_index: usize, english: bool) -> Value {
    let mut obj = match template {
        Value::Object(o) => o.clone(),
        _ => IndexMap::new(),
    };

    let display = &game.data.display;
    let name = if display.name.is_empty() { display.tname.clone() } else { display.name.clone() };
    let tname = if english {
        if !display.name_eng.is_empty() { display.name_eng.clone() }
        else if !display.tname.is_empty() { display.tname.clone() }
        else { name.clone() }
    } else if !display.tname.is_empty() {
        display.tname.clone()
    } else {
        name.clone()
    };

    let alt = &game.data.rom_alt;
    let region_tag_e  = alt.region_tag_e .clone().unwrap_or_else(|| "NO".into());
    let region_tag_s1 = alt.region_tag_s1.clone().unwrap_or_else(|| "NO".into());
    let region_tag_s2 = alt.region_tag_s2.clone().unwrap_or_else(|| "NO".into());

    set(&mut obj, "name", Value::String(name));
    set(&mut obj, "tname", Value::String(tname));
    set(&mut obj, "regionTag", Value::String(game.region_tag.clone()));
    set(&mut obj, "regionTagE", Value::String(region_tag_e));
    set(&mut obj, "regionTagS1", Value::String(region_tag_s1));
    set(&mut obj, "regionTagS2", Value::String(region_tag_s2));

    // Display
    set(&mut obj, "image", Value::Int(slot_index as i64));
    set(&mut obj, "players", Value::Int(display.players as i64));
    set(&mut obj, "titlebar", Value::Int(display.titlebar as i64));
    set(&mut obj, "ccolor", Value::Int(display.ccolor as i64));
    set(&mut obj, "csize", Value::Int(display.csize as i64));
    set(&mut obj, "demo_time", Value::Int(display.demo_time as i64));

    // Emulator
    let emu = &game.data.emulator;
    set(&mut obj, "EmuOfsX", Value::Int(emu.emu_ofs_x as i64));
    set(&mut obj, "EmuOfsY", Value::Int(emu.emu_ofs_y as i64));
    set(&mut obj, "ScreenMode", Value::Int(emu.screen_mode as i64));

    // Sort indices: real values from gamelist.json
    let s = &game.sort;
    set(&mut obj, "sor_date", Value::Int(s.sor_date as i64));
    set(&mut obj, "sor_demo", Value::Int(s.sor_demo as i64));
    set(&mut obj, "sor_genr", Value::Int(s.sor_genr as i64));
    set(&mut obj, "sor_name", Value::Int(s.sor_name as i64));
    set(&mut obj, "sor_pnum", Value::Int(s.sor_pnum as i64));

    Value::Object(obj)
}

/// Build a DUMMY item from the template, with a unique image index.
fn make_dummy(template: &Value, abs_slot: usize) -> Value {
    let mut obj = match template {
        Value::Object(o) => o.clone(),
        _ => IndexMap::new(),
    };
    set(&mut obj, "image", Value::Int(abs_slot as i64));
    Value::Object(obj)
}

fn set(obj: &mut IndexMap<String, Value>, key: &str, value: Value) {
    obj.insert(key.into(), value);
}

