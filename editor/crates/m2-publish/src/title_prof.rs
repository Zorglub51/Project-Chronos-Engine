// Generate title_prof.psb.m from a stock template + a folder's game list.
//
// title_prof structure:
//   root.controller.*             (UI/input config — preserved from template)
//   root.game_versions[regionTag] (string-table + version triple — REPLACED)
//   root.m2epi.version[regionTag] (per-game ROM mapping — REPLACED)
//
// The version map is keyed by regionTag, so each alt regionTag (E/S1/S2)
// becomes its own entry referencing the same ROM. Alt entries inherit the
// primary's arch/country/preamp/extras unless the user provides overrides
// (TODO: per-alt overrides).

use crate::library::{Game, GameRom};
use crate::Error;
use indexmap::IndexMap;
use m2_psb::Value;

pub struct GenInputs<'a> {
    pub template_psb: &'a [u8],
    pub games: &'a [Game],
}

pub fn generate(inputs: &GenInputs) -> Result<Vec<u8>, Error> {
    let mut tree = m2_psb::read(inputs.template_psb)?;

    set_path(&mut tree, &["root", "m2epi", "version"], Value::Object(build_version_map(inputs.games)))?;
    set_path(&mut tree, &["root", "game_versions"], Value::Object(build_game_versions_map(inputs.games)))?;

    Ok(m2_psb::write(&tree, 4)?)
}

fn build_version_map(games: &[Game]) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for g in games {
        let primary = build_rom_entry(&g.data.rom);
        map.insert(g.region_tag.clone(), primary.clone());

        // Alts inherit primary config — same ROM, same arch/country/preamp/extras
        let alt = &g.data.rom_alt;
        for tag in [&alt.region_tag_e, &alt.region_tag_s1, &alt.region_tag_s2].into_iter().flatten() {
            if tag == "NO" || tag.is_empty() { continue; }
            map.insert(tag.clone(), primary.clone());
        }
    }
    map
}

fn build_rom_entry(rom: &GameRom) -> Value {
    let mut obj = IndexMap::new();
    obj.insert("arch".into(), Value::String(rom.arch.clone()));
    obj.insert("country".into(), Value::String(rom.country.clone()));
    obj.insert("preamp".into(), Value::Float(rom.preamp));
    obj.insert("rom".into(), Value::String(format!("roms/{}", basename(&rom.rom))));
    if let Some(sc) = &rom.tg16cd_systemcard {
        obj.insert("tg16cd_systemcard".into(), Value::String(sc.clone()));
    }
    if let Some(opt) = rom.tg16_option {
        obj.insert("tg16_option".into(), Value::Int(opt as i64));
    }
    if let Some(pad) = rom.tg16pad_type {
        obj.insert("tg16pad_type".into(), Value::Int(pad as i64));
    }
    Value::Object(obj)
}

fn build_game_versions_map(games: &[Game]) -> IndexMap<String, Value> {
    // The string table that maps regionTag → display label key. We assign the
    // base label per primary regionTag and reuse the same label for any alts
    // (they share UI labels in stock M2 too).
    let mut map = IndexMap::new();
    for (i, g) in games.iter().enumerate() {
        let label = format!("MenuItemText__TITLE_GAME_VERSION_{:03}", i);
        let arr = vec![Value::Int(i as i64), Value::Int(0), Value::String(label.clone())];
        map.insert(g.region_tag.clone(), Value::Array(arr.clone()));
        let alt = &g.data.rom_alt;
        for tag in [&alt.region_tag_e, &alt.region_tag_s1, &alt.region_tag_s2].into_iter().flatten() {
            if tag == "NO" || tag.is_empty() { continue; }
            map.insert(tag.clone(), Value::Array(arr.clone()));
        }
    }
    map
}

fn basename(p: &str) -> String {
    let bn = p.rsplit('/').next().unwrap_or(p);
    // Stock title_prof stores ROM paths *without* the `.m` suffix; m2engage
    // appends `.m` internally when reading. HuCard ROMs in the library are
    // hardlinked as the encrypted `.pce.m` form, so strip that here.
    // CD-ROM (`.pcd`) files have no `.m` form and pass through unchanged.
    bn.strip_suffix(".m").unwrap_or(bn).to_string()
}

fn set_path(tree: &mut Value, path: &[&str], new_value: Value) -> Result<(), Error> {
    let mut node = tree;
    for (i, &key) in path.iter().enumerate() {
        let is_last = i == path.len() - 1;
        match node {
            Value::Object(obj) => {
                if is_last {
                    obj.insert(key.into(), new_value);
                    return Ok(());
                }
                node = obj.get_mut(key).ok_or_else(|| Error::Template(
                    format!("template missing path key '{}'", key)
                ))?;
            }
            _ => return Err(Error::Template(format!(
                "template type mismatch at '{}': expected object", key
            ))),
        }
    }
    Ok(())
}
