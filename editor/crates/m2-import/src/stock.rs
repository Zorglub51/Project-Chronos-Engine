use crate::{
    cd::extract_bios,
    covers::save_cover,
    report,
    source::{safe_relative, Source},
    BiosConfig, Reporter,
};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{fs, path::Path};

const TEMPLATES: &[&str] = &[
    "040/config/title_prof.psb.m",
    "040/config/title_mode_top.psb.m",
    "040/motion/title_jp_titleselect_jp.psb.m",
    "040/motion/title_jp_titleselect_us.psb.m",
];

#[derive(Debug, Serialize)]
pub struct NewLibraryResult {
    pub root: String,
    pub games: usize,
    pub bios: Option<BiosConfig>,
    pub warnings: Vec<String>,
}

fn psb(source: &Source, name: &str) -> Result<m2_psb::Value> {
    let bytes = source.read(name, 128 * 1024 * 1024)?;
    let bytes = if bytes.starts_with(b"mzs\0") {
        m2_mzs::unpack_default(
            &bytes,
            Path::new(name).file_name().unwrap().to_str().unwrap(),
        )?
    } else {
        bytes
    };
    Ok(m2_psb::read(&bytes)?)
}

pub fn create_library(
    input: &Path,
    destination: &Path,
    include_games: bool,
    bios_cache: &Path,
    progress: Reporter<'_>,
) -> Result<NewLibraryResult> {
    ensure!(
        !destination.exists(),
        "Destination already exists. Choose a new library folder: {}",
        destination.display()
    );
    let parent = destination
        .parent()
        .context("Choose a destination folder")?;
    ensure!(parent.is_dir(), "Destination parent does not exist");
    report(progress, "Reading console dump…", None);
    let source = Source::open(input)?;
    // Check the full set up front, before a destination is visible.
    for name in TEMPLATES.iter().copied().chain([
        "m2engage",
        "libopus.so.0",
        "version",
        "shutdown.png",
        "system/script/init.nut.m",
        "system/config/system_prof.psb.m",
    ]) {
        ensure!(
            source.size(name).is_some(),
            "The dump is missing {name}. Select the original P9, not only alldata.bin."
        );
    }
    let temporary = tempfile::Builder::new()
        .prefix(".chronos-library-")
        .tempdir_in(parent)?;
    let root = temporary.path();
    let game = root.join("game");
    let library = root.join("library");
    let files = source
        .names()
        .filter(|n| {
            !n.starts_with("system/roms/")
                && !n.starts_with("save/")
                && !n.starts_with("alldata.")
                && !n.starts_with("lost+found/")
        })
        .cloned()
        .collect::<Vec<_>>();
    for (i, name) in files.iter().enumerate() {
        report(
            progress,
            format!("Preparing menu resources: {name}"),
            Some((i as u64, files.len() as u64)),
        );
        source.copy(name, &game.join(safe_relative(name)?))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(game.join("m2engage"), fs::Permissions::from_mode(0o755))?;
    }
    for path in [
        game.join("save"),
        game.join("system/roms"),
        library.join("published"),
    ] {
        fs::create_dir_all(path)?;
    }
    for name in TEMPLATES.iter().chain(m2_publish::fonts::RESOURCES) {
        source.copy(name, &library.join("templates").join(name))?;
    }
    let mut warnings = Vec::new();
    let mut bios = None;
    report(progress, "Extracting BIOS from an original CD game…", None);
    for name in source
        .names()
        .filter(|n| n.starts_with("system/roms/") && n.to_ascii_lowercase().ends_with(".pcd"))
    {
        match extract_bios(
            source.reader(name)?,
            &format!("{}: {name}", input.display()),
            bios_cache,
        ) {
            Ok(found) => {
                bios = Some(found);
                break;
            }
            Err(e) => {
                warnings.push(format!("{name}: {e:#}"));
            }
        }
    }
    if bios.is_none() {
        warnings.push(
            "No compatible BIOS pair found. Configure BIOS in Settings before importing a CUE."
                .into(),
        );
    }
    let mut games = 0;
    if include_games {
        games = extract_games(&source, &library, progress)?;
    } else {
        for lineup in ["jp", "us"] {
            fs::create_dir_all(library.join(lineup))?;
            fs::write(library.join(lineup).join("gamelist.json"), "[]\n")?;
        }
    }
    if include_games {
        report(progress, "Publishing original games…", None);
        m2_publish::publish(&m2_publish::PublishOptions {
            library_root: library.clone(),
            stock_data_root: library.join("templates"),
            output_root: library.join("published"),
            incremental: false,
        })?;
    }
    fs::write(
        root.join("source.json"),
        serde_json::to_vec_pretty(
            &json!({"source":input,"include_original_games":include_games,"games":games,"warnings":warnings}),
        )?,
    )?;
    ensure!(
        !destination.exists(),
        "Destination was created by another operation; choose another folder"
    );
    fs::rename(root, destination).context("Cannot finish creating the library")?;
    report(progress, "Library created", Some((1, 1)));
    Ok(NewLibraryResult {
        root: destination.to_string_lossy().into(),
        games,
        bios,
        warnings,
    })
}

fn extract_games(source: &Source, library: &Path, progress: Reporter<'_>) -> Result<usize> {
    let menu = psb(source, TEMPLATES[1])?.to_json();
    let items = menu
        .get("items")
        .or_else(|| menu.get("root").and_then(|r| r.get("items")))
        .and_then(Value::as_array)
        .context("Original menu has no game list")?;
    ensure!(
        items.len() >= 200,
        "Unsupported original menu: expected the JP/US lineups"
    );
    let profiles = psb(source, TEMPLATES[0])?.to_json();
    let versions = profiles
        .pointer("/root/m2epi/version")
        .and_then(Value::as_object)
        .context("Original ROM profiles not found")?;
    let mut count = 0;
    for (lineup, start, motion) in [("jp", 0, TEMPLATES[2]), ("us", 50, TEMPLATES[3])] {
        let covers = psb(source, motion)?;
        let dir = library.join(lineup);
        fs::create_dir_all(&dir)?;
        let mut list = Vec::new();
        for (slot, item) in items.iter().enumerate().skip(start).take(50) {
            let tag = item["regionTag"]
                .as_str()
                .context("Game has no regionTag")?;
            if tag == "DUMMY" {
                continue;
            }
            ensure!(
                safe_relative(tag)?.components().count() == 1 && !tag.starts_with("FOLDER"),
                "Invalid game identifier: {tag}"
            );
            report(
                progress,
                format!("Importing original game: {tag}"),
                Some(((slot - start) as u64, 50)),
            );
            let profile = versions
                .get(tag)
                .with_context(|| format!("No ROM profile for {tag}"))?;
            let original = profile["rom"]
                .as_str()
                .context("Game has no ROM filename")?;
            let filename = Path::new(original)
                .file_name()
                .and_then(|n| n.to_str())
                .context("Invalid ROM filename")?;
            safe_relative(filename)?;
            let dest = dir.join(tag);
            fs::create_dir(&dest)?;
            let rom_name = format!("system/roms/{filename}");
            if source.size(&rom_name).is_some() {
                source.copy(&rom_name, &dest.join(filename))?;
            } else {
                let packed = format!("{rom_name}.m");
                let bytes = source.read(&packed, 32 * 1024 * 1024)?;
                let data = m2_mzs::unpack_default(&bytes, &format!("{filename}.m"))?;
                fs::write(dest.join(filename), data)?;
            }
            let (width, height) = save_cover(
                &covers,
                item["image"].as_u64().unwrap_or(slot as u64),
                &dest.join("cover.png"),
            )
            .with_context(|| format!("Extract cover for {tag}"))?;
            let english = items
                .iter()
                .skip(start + 100)
                .take(50)
                .find(|i| i["regionTag"].as_str() == Some(tag))
                .unwrap_or(item);
            let mut display = serde_json::Map::new();
            for key in [
                "name",
                "tname",
                "titlebar",
                "ccolor",
                "csize",
                "players",
                "demo_time",
            ] {
                display.insert(
                    key.into(),
                    item.get(key).cloned().unwrap_or_else(|| {
                        if key == "name" || key == "tname" {
                            json!("")
                        } else {
                            json!(0)
                        }
                    }),
                );
            }
            display.insert(
                "name_eng".into(),
                english.get("tname").cloned().unwrap_or_else(|| json!("")),
            );
            let mut rom = profile.as_object().context("Invalid ROM profile")?.clone();
            rom.insert("rom".into(), json!(filename));
            rom.insert("regionTag".into(), json!(tag));
            let mut data = json!({"display":display,"rom":rom,"cover":"cover.png","emulator":{
                "EmuOfsX":item.get("EmuOfsX").cloned().unwrap_or(json!(0)),
                "EmuOfsY":item.get("EmuOfsY").cloned().unwrap_or(json!(0)),
                "ScreenMode":item.get("ScreenMode").cloned().unwrap_or(json!(0))}});
            if (width, height) != (240, 240) {
                data["cover_size"] =
                    json!({"width":width,"height":height,"originX":width/2,"originY":height/2});
            }
            fs::write(dest.join("game.json"), serde_json::to_vec_pretty(&data)?)?;
            let mut entry = json!({"folder":tag});
            for key in ["sor_date", "sor_demo", "sor_genr", "sor_name", "sor_pnum"] {
                entry[key] = item.get(key).cloned().unwrap_or(json!(0));
            }
            list.push(entry);
            count += 1;
        }
        fs::write(dir.join("gamelist.json"), serde_json::to_vec_pretty(&list)?)?;
    }
    Ok(count)
}
