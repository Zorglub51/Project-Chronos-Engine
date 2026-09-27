use m2_import::{import_rom, remove_replaced_rom};
use serde_json::json;
use std::{fs, path::Path};

fn select(directory: &Path, filename: &str) {
    fs::write(
        directory.join("game.json"),
        json!({"rom":{"rom":filename}}).to_string(),
    )
    .unwrap();
}

#[test]
fn successful_replacement_removes_only_the_previous_local_rom() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let game = root.join("game");
    fs::create_dir(&game).unwrap();
    let source = root.join("new.pce");
    fs::write(&source, vec![0x5a; 8192]).unwrap();
    for name in [
        "cover.png",
        "sram.bin",
        "data_000.bin",
        "bios.pce",
        "another.pcd",
    ] {
        fs::write(game.join(name), b"keep").unwrap();
    }
    for old in ["old.pce", "old.PCE.m", "old.sgx", "old.pcd", "old.bin"] {
        fs::write(game.join(old), b"old ROM").unwrap();
        select(&game, old);
        let imported = import_rom(&source, &game, None, &|_| {}).unwrap();
        assert!(game.join(old).exists());
        select(&game, &imported.filename);
        remove_replaced_rom(&game, old, &imported.filename).unwrap();
        assert!(!game.join(old).exists());
        assert_eq!(
            fs::read(game.join(&imported.filename)).unwrap(),
            fs::read(&source).unwrap()
        );
        for name in [
            "cover.png",
            "sram.bin",
            "data_000.bin",
            "bios.pce",
            "another.pcd",
        ] {
            assert_eq!(fs::read(game.join(name)).unwrap(), b"keep");
        }
        // Cleanup remains safe to retry when the previous file is already gone.
        remove_replaced_rom(&game, old, &imported.filename).unwrap();
    }
}

#[test]
fn failed_save_or_missing_replacement_cannot_delete_the_old_rom() {
    let temp = tempfile::tempdir().unwrap();
    let game = temp.path();
    fs::write(game.join("old.pce"), b"keep").unwrap();
    fs::write(game.join("new.pce"), b"new").unwrap();
    select(game, "old.pce");
    assert!(remove_replaced_rom(game, "old.pce", "new.pce").is_err());
    fs::write(game.join("game.json"), b"broken JSON").unwrap();
    assert!(remove_replaced_rom(game, "old.pce", "new.pce").is_err());
    select(game, "new.pce");
    fs::remove_file(game.join("new.pce")).unwrap();
    assert!(remove_replaced_rom(game, "old.pce", "new.pce").is_err());
    fs::write(game.join("new.pce"), b"").unwrap();
    assert!(remove_replaced_rom(game, "old.pce", "new.pce").is_err());
    assert_eq!(fs::read(game.join("old.pce")).unwrap(), b"keep");
}

#[test]
fn same_rom_and_files_still_used_as_bios_or_alternates_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let game = temp.path();
    fs::write(game.join("old.pce"), b"keep").unwrap();
    fs::write(game.join("new.pce"), b"new").unwrap();
    select(game, "old.pce");
    remove_replaced_rom(game, "old.pce", "old.pce").unwrap();
    for metadata in [
        json!({"rom":{"rom":"new.pce", "tg16cd_systemcard":"old.pce"}}),
        json!({"rom":{"rom":"new.pce"}, "rom_alt":{"s1":{"rom":"old.pce"}}}),
    ] {
        fs::write(game.join("game.json"), metadata.to_string()).unwrap();
        remove_replaced_rom(game, "old.pce", "new.pce").unwrap();
        assert_eq!(fs::read(game.join("old.pce")).unwrap(), b"keep");
    }
    remove_replaced_rom(game, "", "new.pce").unwrap();
}

#[test]
fn cleanup_rejects_paths_non_rom_files_and_directories() {
    let temp = tempfile::tempdir().unwrap();
    let game = temp.path().join("game");
    fs::create_dir(&game).unwrap();
    fs::write(temp.path().join("outside.pce"), b"source").unwrap();
    fs::write(game.join("new.pce"), b"new").unwrap();
    select(&game, "new.pce");
    for old in [
        "../outside.pce",
        "..\\outside.pce",
        "/outside.pce",
        "C:\\outside.pce",
        "file:stream.pce",
        "game.json",
        "cover.png",
    ] {
        assert!(remove_replaced_rom(&game, old, "new.pce").is_err(), "{old}");
    }
    fs::create_dir(game.join("folder.pce")).unwrap();
    assert!(remove_replaced_rom(&game, "folder.pce", "new.pce").is_err());
    assert!(game.join("folder.pce").is_dir());
    assert_eq!(
        fs::read(temp.path().join("outside.pce")).unwrap(),
        b"source"
    );
}

#[cfg(unix)]
#[test]
fn symbolic_links_do_not_allow_removing_source_roms() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let game = root.join("game");
    fs::create_dir(&game).unwrap();
    fs::write(root.join("source.pce"), b"source").unwrap();
    fs::write(game.join("new.pce"), b"new").unwrap();
    symlink(root.join("source.pce"), game.join("old.pce")).unwrap();
    select(&game, "new.pce");
    assert!(remove_replaced_rom(&game, "old.pce", "new.pce").is_err());
    fs::remove_file(game.join("old.pce")).unwrap();
    fs::write(game.join("old.pce"), b"old").unwrap();
    fs::remove_file(game.join("new.pce")).unwrap();
    symlink(root.join("source.pce"), game.join("new.pce")).unwrap();
    assert!(remove_replaced_rom(&game, "old.pce", "new.pce").is_err());
    assert_eq!(fs::read(game.join("old.pce")).unwrap(), b"old");
    assert_eq!(fs::read(root.join("source.pce")).unwrap(), b"source");
}

#[test]
fn case_variants_preserve_aliases_but_remove_distinct_files() {
    let temp = tempfile::tempdir().unwrap();
    let game = temp.path();
    fs::write(game.join("old.pce"), b"old").unwrap();
    let alias = game.join("OLD.PCE").exists();
    if !alias {
        fs::write(game.join("OLD.PCE"), b"new").unwrap();
    }
    select(game, "OLD.PCE");
    remove_replaced_rom(game, "old.pce", "OLD.PCE").unwrap();
    assert!(game.join("OLD.PCE").exists());
    assert_eq!(game.join("old.pce").exists(), alias);
}
