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

    let mut versions = build_version_map(inputs.games);
    let mut game_versions = build_game_versions_map(inputs.games);
    // Native _02_game_regionTag has 15 bytes including its terminator.
    // Navigation uses the full menu tag to locate a folder, but its emulator
    // profile must have a short key for initialization and context reloads.
    // Replace only long navigation keys, without adding or moving save slots.
    let mut default_tag = inputs.games.first().map(|g| g.region_tag.clone());
    for (index, game) in inputs.games.iter().enumerate() {
        if game.data.rom.arch == "folder" && game.region_tag.len() > 14 {
            let alias = format!("FOLDER_{index:03}");
            if game_versions.contains_key(&alias) {
                return Err(Error::Library(format!("{alias} is reserved for native folder initialization")));
            }
            let profile = versions.swap_remove(&game.region_tag).unwrap();
            let version = game_versions.swap_remove(&game.region_tag).unwrap();
            versions.insert(alias.clone(), profile);
            game_versions.insert(alias.clone(), version);
            if index == 0 { default_tag = Some(alias); }
        }
    }
    if let Some(default) = default_tag {
        let defaults = ["japan", "usa", "europe", "asia"].into_iter()
            .map(|region| (region.into(), Value::String(default.clone()))).collect();
        set_path(&mut tree, &["root", "game_versions_default"], Value::Object(defaults))?;
    }
    set_path(&mut tree, &["root", "m2epi", "version"], Value::Object(versions))?;
    set_path(&mut tree, &["root", "game_versions"], Value::Object(game_versions))?;

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
    // `folder` is an editor marker, not a machine supported by native M2.
    // The stock engine tolerates a missing ROM with tg16, including at boot.
    // Keep the pseudo-ROM/tag and slot: navigation uses csize + regionTag.
    let arch = if rom.arch == "folder" { "tg16" } else { &rom.arch };
    obj.insert("arch".into(), Value::String(arch.into()));
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
    if bn.to_ascii_lowercase().ends_with(".m") {
        bn[..bn.len() - 2].to_string()
    } else {
        bn.to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::GameJson;

    fn game(tag: &str, arch: &str) -> Game {
        Game {
            dir_name: tag.into(), region_tag: tag.into(),
            data: GameJson { rom: GameRom { arch: arch.into(), rom: format!("{tag}.pce.m"),
                                         ..Default::default() }, ..Default::default() },
            sort: Default::default(), source_dir: Default::default(),
        }
    }

    fn member<'a>(value: &'a Value, key: &str) -> &'a Value {
        match value { Value::Object(map) => &map[key], _ => panic!("expected object") }
    }

    fn generated(games: &[Game]) -> Value {
        let tree = Value::Object(IndexMap::from([("root".into(), Value::Object(IndexMap::from([
            ("m2epi".into(), Value::Object(IndexMap::new())),
            ("controller".into(), Value::String("keep-stock-controller".into())),
        ])))]));
        let template = m2_psb::write(&tree, 4).unwrap();
        m2_psb::read(&generate(&GenInputs { template_psb: &template, games }).unwrap()).unwrap()
    }

    #[test]
    fn long_first_folder_tag_has_a_short_boot_alias_without_changing_slots() {
        let tag = "FOLDER_jp_FOLDER_TEST";
        let output = generated(&[game(tag, "folder"), game("GAME000", "tg16")]).to_json();
        let root = &output["root"];
        for region in ["japan", "usa", "europe", "asia"] {
            assert_eq!(root["game_versions_default"][region], "FOLDER_000");
        }
        assert!(root["game_versions"].get(tag).is_none());
        assert_eq!(root["m2epi"]["version"]["FOLDER_000"]["rom"], format!("roms/{tag}.pce"));
        assert_eq!(root["game_versions"]["FOLDER_000"][0], 0);
        assert_eq!(root["game_versions"]["GAME000"][0], 1);
        let short = generated(&[game("FOLDER_jp_BACK", "folder")]).to_json();
        assert_eq!(short["root"]["game_versions_default"]["japan"], "FOLDER_jp_BACK");
        assert!(short["root"]["game_versions"].get("FOLDER_000").is_none());
    }

    #[test]
    fn folder_uses_native_machine_without_moving_back_or_save_slots() {
        let games = [game("FOLDER_jp_BACK", "folder"), game("GAME053", "tg16"), game("GAME007", "tg16")];
        let output = generated(&games);
        let root = member(&output, "root");
        let machines = member(member(root, "m2epi"), "version");
        assert!(matches!(member(member(machines, "FOLDER_jp_BACK"), "arch"), Value::String(s) if s == "tg16"));
        assert!(matches!(member(member(machines, "FOLDER_jp_BACK"), "rom"), Value::String(s) if s == "roms/FOLDER_jp_BACK.pce"));
        let versions = member(root, "game_versions");
        for (tag, slot) in [("FOLDER_jp_BACK", 0), ("GAME053", 1), ("GAME007", 2)] {
            assert!(matches!(member(versions, tag), Value::Array(v) if matches!(v.first(), Some(Value::Int(n)) if *n == slot)));
        }
        assert!(matches!(member(root, "controller"), Value::String(s) if s == "keep-stock-controller"));
    }

    #[test]
    fn folder_only_pack_is_native_compatible_and_real_cd_settings_are_preserved() {
        let mut folder = game("FOLDER_us_TEST", "folder");
        folder.data.rom.rom = "FOLDER_us_TEST".into();
        let output = generated(&[folder]);
        let versions = member(member(member(&output, "root"), "m2epi"), "version");
        assert!(matches!(member(member(versions, "FOLDER_us_TEST"), "arch"), Value::String(s) if s == "tg16"));
        assert!(matches!(member(member(versions, "FOLDER_us_TEST"), "rom"), Value::String(s) if s == "roms/FOLDER_us_TEST"));
        let mut cd = game("GAME002", "tg16cd");
        cd.data.rom.tg16cd_systemcard = Some("syscard.pce".into());
        cd.data.rom.tg16_option = Some(2);
        let output = generated(&[cd]);
        let versions = member(member(member(&output, "root"), "m2epi"), "version");
        let entry = member(versions, "GAME002");
        assert!(matches!(member(entry, "arch"), Value::String(s) if s == "tg16cd"));
        assert!(matches!(member(entry, "tg16cd_systemcard"), Value::String(s) if s == "syscard.pce"));
        assert!(matches!(member(entry, "tg16_option"), Value::Int(2)));
    }
}
