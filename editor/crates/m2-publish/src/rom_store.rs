//! One console-ready ROM pool per library. Legacy local ROMs remain readable
//! until their verified migration completes; game metadata does not need rewriting.
use crate::Error;
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn console_name(name: &str) -> Result<String, Error> {
    if name.is_empty() || name.contains(['/', '\\', ':', '\0']) || name == "." || name == ".." {
        return Err(Error::Library(
            "ROM references must be local filenames".into(),
        ));
    }
    let lower = name.to_ascii_lowercase();
    Ok(if lower.ends_with(".pce") || lower.ends_with(".sgx") {
        format!("{name}.m")
    } else if lower.ends_with(".pce.m") || lower.ends_with(".sgx.m") {
        format!("{}.m", &name[..name.len() - 2])
    } else {
        name.into()
    })
}

pub fn library_root(directory: &Path) -> Option<PathBuf> {
    directory.ancestors().find_map(|p| {
        if matches!(p.file_name().and_then(|s| s.to_str()), Some("jp" | "us")) {
            let root = p.parent()?;
            if root.join("jp/gamelist.json").is_file() || root.join("us/gamelist.json").is_file() {
                return Some(root.to_path_buf());
            }
        }
        None
    })
}

pub fn resolve(directory: &Path, name: &str) -> PathBuf {
    let local = directory.join(name);
    if local.exists() {
        return local;
    }
    if let (Some(root), Ok(name)) = (library_root(directory), console_name(name)) {
        return root.join("published/roms").join(name);
    }
    local
}

fn regular(path: &Path) -> Result<(), Error> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Library(format!(
            "Expected a regular ROM file: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn checked_pool(root: &Path) -> Result<PathBuf, Error> {
    let mut path = root.to_path_buf();
    for part in ["published", "roms"] {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(m) if !m.is_dir() => {
                return Err(Error::Library(format!(
                    "Expected a directory, not a link: {}",
                    path.display()
                )))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&path)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}

pub fn same_contents(a: &Path, b: &Path) -> Result<bool, Error> {
    regular(a)?;
    regular(b)?;
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let (mut a, mut b) = (fs::File::open(a)?, fs::File::open(b)?);
    let (mut x, mut y) = ([0; 65536], [0; 65536]);
    loop {
        let n = a.read(&mut x)?;
        if n == 0 {
            return Ok(true);
        }
        b.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Ok(false);
        }
    }
}

/// Never overwrite an existing shared file. Temporary writes are verified before
/// becoming visible, and copies use bounded buffers even for large CD images.
pub fn store(source: &Path, pool: &Path, name: &str) -> Result<PathBuf, Error> {
    regular(source)?;
    let name = console_name(name)?;
    let destination = pool.join(&name);
    let raw = source
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("pce") || s.eq_ignore_ascii_case("sgx"));
    let bytes = if raw { Some(fs::read(source)?) } else { None };
    // Detect case aliases on FAT and also on case-sensitive development disks.
    if let Some(existing) = fs::read_dir(pool)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&name))
    {
        let path = existing.path();
        regular(&path)?;
        let same = if let Some(bytes) = &bytes {
            m2_mzs::unpack_default(&fs::read(&path)?, &name)? == *bytes
        } else {
            same_contents(source, &path)?
        };
        if !same || existing.file_name().to_string_lossy() != name {
            return Err(Error::Library(format!(
                "ROM filename collision: {name}; original files were kept"
            )));
        }
        return Ok(path);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(pool)?;
    if let Some(bytes) = &bytes {
        temporary.write_all(&m2_mzs::pack_default(bytes, &name)?)?;
    } else {
        std::io::copy(&mut fs::File::open(source)?, &mut temporary)?;
    }
    temporary.as_file().sync_all()?;
    let valid = if let Some(bytes) = bytes {
        m2_mzs::unpack_default(&fs::read(temporary.path())?, &name)? == bytes
    } else {
        same_contents(source, temporary.path())?
    };
    if !valid {
        return Err(Error::Library(
            "ROM copy verification failed; original kept".into(),
        ));
    }
    temporary
        .persist_noclobber(&destination)
        .map_err(|e| Error::Io(e.error))?;
    Ok(destination)
}

fn walk(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), Error> {
    if !path.exists() {
        return Ok(());
    }
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(Error::Library(format!(
            "Linked library directory: {}",
            path.display()
        )));
    }
    for e in fs::read_dir(path)? {
        let e = e?;
        let ty = e.file_type()?;
        if ty.is_symlink() {
            return Err(Error::Library(format!(
                "Linked library entry: {}",
                e.path().display()
            )));
        }
        if ty.is_dir() {
            walk(&e.path(), files)?;
        } else if e.file_name() == "game.json" {
            files.push(e.path());
        }
    }
    Ok(())
}
fn game_files(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut files = Vec::new();
    for lineup in ["jp", "us"] {
        walk(&root.join(lineup), &mut files)?;
    }
    Ok(files)
}

/// Preflight and verify every source before deleting any local copy. On failure
/// the originals remain; a retry can reuse already verified pool files.
pub fn migrate(root: &Path) -> Result<usize, Error> {
    migrate_with_progress(root, &|_, _, _| {})
}

pub fn migrate_with_progress(
    root: &Path,
    progress: &dyn Fn(&str, usize, usize),
) -> Result<usize, Error> {
    let pool = checked_pool(root)?;
    let mut remove = Vec::new();
    let files = game_files(root)?;
    for (index, file) in files.iter().enumerate() {
        let game: Value = serde_json::from_slice(&fs::read(&file)?)?;
        let Some(name) = game["rom"]["rom"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        if game["rom"]["arch"] == "folder" {
            continue;
        }
        console_name(name)?;
        progress(name, index, files.len());
        let source = file.parent().unwrap().join(name);
        if source.exists() {
            store(&source, &pool, name)?;
            remove.push(source);
        } else {
            regular(&pool.join(console_name(name)?))?;
        }
    }
    // A legacy USB may also contain physical copies under game/system/roms.
    // Leave unique files untouched, and never unlink a bind-mounted pool alias.
    if root.file_name().and_then(|s| s.to_str()) == Some("library") {
        let legacy = root.parent().unwrap().join("game/system/roms");
        if legacy.is_dir() && !fs::symlink_metadata(&legacy)?.file_type().is_symlink() {
            for entry in fs::read_dir(&legacy)? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if ![".pce.m", ".sgx.m", ".pcd"]
                    .iter()
                    .any(|ext| name.to_ascii_lowercase().ends_with(ext))
                {
                    continue;
                }
                let source = entry.path();
                let destination = pool.join(name.as_ref());
                if !entry.file_type()?.is_file() || !destination.is_file() {
                    continue;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let a = fs::metadata(&source)?;
                    let b = fs::metadata(&destination)?;
                    if a.dev() == b.dev() && a.ino() == b.ino() {
                        continue;
                    }
                }
                if same_contents(&source, &destination)? {
                    remove.push(source);
                }
            }
        }
    }
    progress(
        "Verified ROMs; removing duplicate copies…",
        files.len(),
        files.len(),
    );
    for source in &remove {
        fs::remove_file(source)?;
    }
    Ok(remove.len())
}

fn references(value: &Value, name: &str) -> bool {
    match value {
        Value::String(s) => console_name(s).is_ok_and(|n| n.eq_ignore_ascii_case(name)),
        Value::Array(v) => v.iter().any(|v| references(v, name)),
        Value::Object(v) => v.values().any(|v| references(v, name)),
        _ => false,
    }
}

pub fn queue_cleanup(root: &Path, name: &str) -> Result<(), Error> {
    let name = console_name(name)?;
    let path = root.join(".rom-cleanup.json");
    let mut queue: Vec<String> = if path.exists() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        Vec::new()
    };
    if !queue.contains(&name) {
        queue.push(name);
    }
    let mut temp = tempfile::NamedTempFile::new_in(root)?;
    temp.write_all(&serde_json::to_vec(&queue)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}

// Old published packs must also stop referring to a ROM before it can go away.
fn published_references(path: &Path, name: &str) -> Result<bool, Error> {
    if !path.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            return Ok(true);
        } // conservative, never follow links
        if ty.is_dir() {
            if published_references(&entry.path(), name)? {
                return Ok(true);
            }
        } else if entry.file_name().to_string_lossy().ends_with(".psb.m") {
            let filename = entry.file_name();
            let bytes =
                m2_mzs::unpack_default(&fs::read(entry.path())?, &filename.to_string_lossy())?;
            let needle = name.strip_suffix(".m").unwrap_or(name).to_ascii_lowercase();
            if bytes
                .windows(needle.len())
                .any(|s| s.eq_ignore_ascii_case(needle.as_bytes()))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Only explicitly replaced files are eligible; do not sweep user ROMs or BIOS.
pub fn cleanup(root: &Path) -> Result<(), Error> {
    let path = root.join(".rom-cleanup.json");
    if !path.exists() {
        return Ok(());
    }
    let queue: Vec<String> = serde_json::from_slice(&fs::read(&path)?)?;
    let games = game_files(root)?
        .iter()
        .map(|p| Ok(serde_json::from_slice::<Value>(&fs::read(p)?)?))
        .collect::<Result<Vec<_>, Error>>()?;
    let pool = checked_pool(root)?;
    let mut keep = Vec::new();
    for name in queue {
        let name = console_name(&name)?;
        if games.iter().any(|g| references(g, &name))
            || published_references(&root.join("published/folders"), &name)?
        {
            keep.push(name);
            continue;
        }
        let file = pool.join(&name);
        if file.exists() {
            regular(&file)?;
            fs::remove_file(file)?;
        }
    }
    let mut temp = tempfile::NamedTempFile::new_in(root)?;
    temp.write_all(&serde_json::to_vec(&keep)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn library() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for lineup in ["jp", "us"] {
            fs::create_dir(t.path().join(lineup)).unwrap();
            fs::write(t.path().join(lineup).join("gamelist.json"), "[]").unwrap();
        }
        t
    }
    fn game(root: &Path, dir: &str, name: &str, bytes: Option<&[u8]>) -> PathBuf {
        let p = root.join(dir);
        fs::create_dir_all(&p).unwrap();
        fs::write(
            p.join("game.json"),
            serde_json::to_vec(&serde_json::json!({"rom":{"rom":name,"arch":"tg16"}})).unwrap(),
        )
        .unwrap();
        if let Some(bytes) = bytes {
            fs::write(p.join(name), bytes).unwrap();
        }
        p
    }
    #[test]
    fn migration_shares_cd_and_packs_hucard_without_rewriting_metadata() {
        let t = library();
        let root = t.path();
        let a = game(root, "jp/A", "disc.pcd", Some(b"large CD contents"));
        let b = game(
            root,
            "us/FOLDER01/B",
            "disc.pcd",
            Some(b"large CD contents"),
        );
        let c = game(root, "jp/C", "cart.pce", Some(b"cartridge"));
        fs::write(c.join("sram.bin"), b"save me").unwrap();
        let metadata = fs::read(c.join("game.json")).unwrap();
        assert_eq!(migrate(root).unwrap(), 3);
        assert!(!a.join("disc.pcd").exists());
        assert!(!b.join("disc.pcd").exists());
        assert!(!c.join("cart.pce").exists());
        assert_eq!(resolve(&a, "disc.pcd"), resolve(&b, "disc.pcd"));
        assert_eq!(
            m2_mzs::unpack_default(&fs::read(resolve(&c, "cart.pce")).unwrap(), "cart.pce.m")
                .unwrap(),
            b"cartridge"
        );
        assert_eq!(fs::read(c.join("game.json")).unwrap(), metadata);
        assert_eq!(fs::read(c.join("sram.bin")).unwrap(), b"save me");
        assert_eq!(migrate(root).unwrap(), 0);
        assert_eq!(
            fs::read_dir(root.join("published/roms")).unwrap().count(),
            2
        );
        fs::rename(&c, root.join("us/C")).unwrap();
        assert!(resolve(&root.join("us/C"), "cart.pce").is_file());
    }
    #[test]
    fn collision_keeps_every_source_and_existing_output() {
        let t = library();
        let root = t.path();
        let a = game(root, "jp/A", "same.pcd", Some(b"one"));
        let b = game(root, "us/B", "same.pcd", Some(b"two"));
        assert!(migrate(root).is_err());
        assert_eq!(fs::read(a.join("same.pcd")).unwrap(), b"one");
        assert_eq!(fs::read(b.join("same.pcd")).unwrap(), b"two");
    }
    #[test]
    fn existing_packed_hucard_is_reused_even_if_compression_differs() {
        let t = library();
        let root = t.path();
        let a = game(root, "jp/A", "cart.pce", Some(b"cart"));
        let pool = checked_pool(root).unwrap();
        let packed = m2_mzs::pack_default(b"cart", "cart.pce.m").unwrap();
        fs::write(pool.join("cart.pce.m"), &packed).unwrap();
        assert_eq!(migrate(root).unwrap(), 1);
        assert!(!a.join("cart.pce").exists());
        assert_eq!(fs::read(pool.join("cart.pce.m")).unwrap(), packed);
    }
    #[test]
    fn cleanup_waits_for_other_games_and_published_profiles() {
        let t = library();
        let root = t.path();
        let pool = checked_pool(root).unwrap();
        fs::write(pool.join("old.pcd"), b"old").unwrap();
        fs::write(pool.join("unmanaged.pcd"), b"keep").unwrap();
        let a = game(root, "jp/A", "old.pcd", None);
        queue_cleanup(root, "old.pcd").unwrap();
        cleanup(root).unwrap();
        assert!(pool.join("old.pcd").exists());
        fs::remove_file(a.join("game.json")).unwrap();
        let packs = root.join("published/folders/jp/_root");
        fs::create_dir_all(&packs).unwrap();
        let profile = packs.join("title_prof.psb.m");
        fs::write(
            &profile,
            m2_mzs::pack_default(b"roms/old.pcd", "title_prof.psb.m").unwrap(),
        )
        .unwrap();
        cleanup(root).unwrap();
        assert!(pool.join("old.pcd").exists());
        fs::write(
            &profile,
            m2_mzs::pack_default(b"roms/new.pcd", "title_prof.psb.m").unwrap(),
        )
        .unwrap();
        cleanup(root).unwrap();
        assert!(!pool.join("old.pcd").exists());
        assert!(pool.join("unmanaged.pcd").exists());
    }
    #[test]
    fn unsafe_reference_and_missing_source_never_delete_other_roms() {
        let t = library();
        let root = t.path();
        let a = game(root, "jp/A", "good.pcd", Some(b"good"));
        game(root, "us/B", "../bad.pcd", None);
        assert!(migrate(root).is_err());
        assert!(a.join("good.pcd").exists());
    }
    #[test]
    fn only_verified_legacy_usb_duplicates_are_removed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        fs::create_dir_all(root.join("jp")).unwrap();
        fs::write(root.join("jp/gamelist.json"), "[]").unwrap();
        let pool = checked_pool(&root).unwrap();
        let legacy = temp.path().join("game/system/roms");
        fs::create_dir_all(&legacy).unwrap();
        for (name, bytes) in [
            ("same.pcd", b"same".as_slice()),
            ("different.pcd", b"original"),
            ("unique.pcd", b"keep"),
        ] {
            fs::write(legacy.join(name), bytes).unwrap();
        }
        fs::write(pool.join("same.pcd"), b"same").unwrap();
        fs::write(pool.join("different.pcd"), b"changed").unwrap();
        assert_eq!(migrate(&root).unwrap(), 1);
        assert!(!legacy.join("same.pcd").exists());
        assert_eq!(fs::read(legacy.join("different.pcd")).unwrap(), b"original");
        assert_eq!(fs::read(legacy.join("unique.pcd")).unwrap(), b"keep");
        assert_eq!(fs::read(pool.join("same.pcd")).unwrap(), b"same");
    }

    #[cfg(unix)]
    #[test]
    fn linked_pool_is_rejected() {
        let t = library();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("published")).unwrap();
        std::os::unix::fs::symlink(outside.path(), t.path().join("published/roms")).unwrap();
        assert!(migrate(t.path()).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
