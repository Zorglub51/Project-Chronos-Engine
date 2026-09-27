use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{fs, io::ErrorKind, path::Path};

fn rom_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && !name.contains(['/', '\\', ':', '\0'])
        && [".pce", ".pce.m", ".sgx", ".sgx.m", ".pcd", ".bin"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}

fn references(value: &Value, filename: &str) -> bool {
    match value {
        Value::String(s) => s.eq_ignore_ascii_case(filename),
        Value::Array(values) => values.iter().any(|v| references(v, filename)),
        Value::Object(values) => values.values().any(|v| references(v, filename)),
        _ => false,
    }
}

/// Called only after the editor has saved the new ROM choice. Never sweep the
/// directory: remove exactly the previous ROM, leaving sources, saves and BIOS alone.
pub fn remove_replaced_rom(directory: &Path, previous: &str, replacement: &str) -> Result<()> {
    if previous.is_empty() || previous == replacement {
        return Ok(());
    }
    ensure!(
        rom_filename(previous) && rom_filename(replacement),
        "ROM cleanup requires local ROM filenames"
    );
    ensure!(
        fs::symlink_metadata(directory)?.is_dir(),
        "The game directory must not be a symbolic link"
    );
    let mut game: Value = serde_json::from_slice(&fs::read(directory.join("game.json"))?)
        .context("Cannot check the saved ROM selection")?;
    ensure!(
        game["rom"]["rom"].as_str() == Some(replacement),
        "The new ROM selection has not been saved; keeping the previous ROM"
    );
    let new_path = directory.join(replacement);
    let new_info = fs::symlink_metadata(&new_path).context("The new ROM is missing")?;
    ensure!(
        new_info.is_file() && new_info.len() > 0,
        "The new ROM is not a nonempty regular file"
    );
    // The current ROM is checked by file identity below. On case-sensitive
    // disks, old.pce and OLD.pce can legitimately be different files.
    if let Some(rom) = game.get_mut("rom").and_then(Value::as_object_mut) {
        rom.remove("rom");
    }
    // The previous file may still be referenced as a BIOS or an alternate ROM.
    if references(&game, previous) {
        return Ok(());
    }
    let old_path = directory.join(previous);
    let old_info = match fs::symlink_metadata(&old_path) {
        Ok(info) => info,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        old_info.is_file(),
        "The previous ROM is not a regular file; keeping it"
    );
    // Different spellings can designate the same file on macOS and Windows.
    if fs::canonicalize(&old_path)? == fs::canonicalize(&new_path)? {
        return Ok(());
    }
    fs::remove_file(&old_path)
        .with_context(|| format!("Cannot remove previous ROM {}", old_path.display()))
}
