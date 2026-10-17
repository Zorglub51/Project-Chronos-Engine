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

#[cfg(test)]
pub fn generate(inputs: &GenInputs) -> Result<Vec<u8>, Error> {
    generate_with_save_schema(inputs, false)
}

pub fn generate_with_save_schema(inputs: &GenInputs, per_game_saves: bool) -> Result<Vec<u8>, Error> {
    let mut tree = m2_psb::read(inputs.template_psb)?;

    let mut versions = build_version_map(inputs.games);
    let mut game_versions = build_game_versions_map(inputs.games);
    // Native _02_game_regionTag has 15 bytes including its terminator.
    // Navigation uses the full menu tag to locate a folder, but its emulator
    // profile must have a short key for initialization and context reloads.
    // Replace only long navigation keys, without adding or moving save slots.
    let default_index = if per_game_saves {
        inputs.games.iter().position(|g| g.game_id().is_some()).or_else(|| (!inputs.games.is_empty()).then_some(0))
    } else { Some(0) };
    let mut default_tag = default_index.and_then(|index| inputs.games.get(index)).map(|g| g.region_tag.clone());
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
            if Some(index) == default_index { default_tag = Some(alias); }
        }
    }
    if per_game_saves && inputs.games.is_empty() {
        // Internal bootstrap only: no carousel entry, binding or save slot.
        let tag = "CHRONOS_EMPTY";
        let rom = GameRom { arch: "folder".into(), country: "japan".into(),
            preamp: 1.0, rom: "DUMMY.pce".into(), ..Default::default() };
        let mut profile = build_rom_entry(&rom);
        if let Value::Object(fields) = &mut profile {
            fields.insert("chronos_game_id".into(), Value::String(String::new()));
            fields.insert("chronos_save_path".into(), Value::String(String::new()));
        }
        versions.insert(tag.into(), profile);
        game_versions.insert(tag.into(), Value::Array(vec![Value::Int(0), Value::Int(0), Value::String(String::new())]));
        default_tag = Some(tag.into());
    }
    if let Some(default) = default_tag {
        let defaults = ["japan", "usa", "europe", "asia"].into_iter()
            .map(|region| (region.into(), Value::String(default.clone()))).collect();
        set_path(&mut tree, &["root", "game_versions_default"], Value::Object(defaults))?;
    }
    set_path(&mut tree, &["root", "m2epi", "version"], Value::Object(versions))?;
    set_path(&mut tree, &["root", "game_versions"], Value::Object(game_versions))?;
    set_path(&mut tree, &["root", "m2epi", "chronos_save_schema"], Value::Int(if per_game_saves { 2 } else { 1 }))?;
    // Only one native EmuSram is needed. The hook changes its owner before
    // each real game transition and persists the 8448-byte block per game.
    set_path(&mut tree, &["root", "m2epi", "sram_common"], Value::Bool(per_game_saves))?;
    if per_game_saves {
        // Retail disables the general SRAM adapter. V2 uses that adapter for
        // its common working slot; navigation profiles override this to false.
        set_path(&mut tree, &["root", "m2epi", "sram_support"], Value::Bool(true))?;
    }

    Ok(m2_psb::write(&tree, 4)?)
}

fn build_version_map(games: &[Game]) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for g in games {
        let mut primary = build_rom_entry(&g.data.rom);
        if let Value::Object(profile) = &mut primary {
            profile.insert("chronos_game_id".into(), Value::String(g.game_id().unwrap_or("").into()));
            profile.insert("chronos_save_path".into(), Value::String(g.save_path().unwrap_or_default()));
        }
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
    // Preserve the full typed profile, including options unknown to this
    // editor. Null means no per-game override, never an explicit native null.
    for (key, value) in &rom.extra {
        if !value.is_null() && key != "systemdata_sram_data_no" && !key.starts_with("chronos_") {
            obj.insert(key.clone(), crate::parameters::to_psb(value));
        }
    }
    // `folder` is an editor marker, not a machine supported by native M2.
    // The stock engine tolerates a missing ROM with tg16, including at boot.
    // Keep the pseudo-ROM/tag and slot: navigation uses csize + regionTag.
    let arch = if rom.arch == "folder" { "tg16" } else { &rom.arch };
    obj.insert("arch".into(), Value::String(arch.into()));
    obj.insert("country".into(), Value::String(rom.country.clone()));
    obj.insert("preamp".into(), Value::Float(rom.preamp));
    obj.insert("rom".into(), Value::String(format!("roms/{}", basename(&rom.rom))));
    if rom.arch == "folder" {
        // A navigation bootstrap must never capture/apply a real game's SRAM.
        obj.insert("sram_support".into(), Value::Bool(false));
    }
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

    #[test]
    fn expert_metadata_survives_model_psb_and_alternate_profile_roundtrips() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/expert-game.json")).unwrap();
        crate::parameters::validate_game(&original).unwrap();
        let data: GameJson = serde_json::from_value(original.clone()).unwrap();
        let saved = serde_json::to_value(&data).unwrap();
        for block in ["rom", "emulator", "display", "future_metadata"] {
            assert_eq!(saved[block], original[block]);
        }
        let mut g = game("GAME700", "tg16");
        g.data = data;
        let output = generated(&[g]).to_json();
        for tag in ["GAME700", "GAME700E"] {
            let profile = &output["root"]["m2epi"]["version"][tag];
            for (key, expected) in original["rom"].as_object().unwrap() {
                let expected = if key == "rom" { serde_json::json!("roms/fixture.pce") }
                    else { expected.clone() };
                let actual = &profile[key];
                if key == "brightness" || key == "preamp" {
                    assert!((actual.as_f64().unwrap() - expected.as_f64().unwrap()).abs() < 0.00001);
                } else { assert_eq!(actual, &expected, "profile field {key}"); }
            }
        }
    }

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

    #[test]
    fn per_game_save_boot_uses_first_playable_without_moving_native_slots() {
        let mut playable = game("GAME007", "sgx");
        playable.data.game_id = Some("0123456789abcdef0123456789abcdef".into());
        let games = [game("FOLDER_jp_BACK", "folder"), playable];
        let template = m2_psb::write(&Value::Object(IndexMap::from([("root".into(),
            Value::Object(IndexMap::from([("m2epi".into(), Value::Object(IndexMap::new()))])))])), 4).unwrap();
        let output = m2_psb::read(&generate_with_save_schema(&GenInputs {
            template_psb: &template, games: &games
        }, true).unwrap()).unwrap().to_json();
        let root = &output["root"];
        assert_eq!(root["game_versions_default"]["japan"], "GAME007");
        assert_eq!(root["game_versions"]["FOLDER_jp_BACK"][0], 0);
        assert_eq!(root["game_versions"]["GAME007"][0], 1);
        assert_eq!(root["m2epi"]["sram_common"], true);
        assert_eq!(root["m2epi"]["sram_support"], true);
        assert_eq!(root["m2epi"]["version"]["GAME007"]["chronos_game_id"],
            "0123456789abcdef0123456789abcdef");
        assert_eq!(root["m2epi"]["version"]["FOLDER_jp_BACK"]["chronos_game_id"], "");
        assert_eq!(root["m2epi"]["version"]["FOLDER_jp_BACK"]["sram_support"], false);
    }

    #[test]
    fn empty_save_catalogue_has_an_unowned_bootstrap_instead_of_stale_defaults() {
        let template = m2_psb::write(&Value::Object(IndexMap::from([("root".into(),
            Value::Object(IndexMap::from([("m2epi".into(), Value::Object(IndexMap::new()))])))])), 4).unwrap();
        let output = m2_psb::read(&generate_with_save_schema(&GenInputs {
            template_psb: &template, games: &[]
        }, true).unwrap()).unwrap().to_json();
        let root = &output["root"];
        assert_eq!(root["game_versions_default"]["japan"], "CHRONOS_EMPTY");
        assert_eq!(root["m2epi"]["version"]["CHRONOS_EMPTY"]["chronos_game_id"], "");
        assert_eq!(root["m2epi"]["version"]["CHRONOS_EMPTY"]["sram_support"], false);
    }
}
