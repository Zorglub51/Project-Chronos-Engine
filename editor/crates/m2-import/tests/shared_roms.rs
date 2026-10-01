use m2_import::{import_library_rom, remove_replaced_rom};
use std::fs;

#[test]
fn shared_import_reuses_rom_across_games_and_preserves_sources() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    fs::create_dir_all(root.join("jp/A")).unwrap();
    fs::create_dir_all(root.join("us/B")).unwrap();
    for l in ["jp", "us"] {
        fs::write(root.join(l).join("gamelist.json"), "[]").unwrap();
    }
    let source = root.join("cart.pce");
    fs::write(&source, b"original cartridge").unwrap();
    let a = import_library_rom(&source, &root.join("jp/A"), None, &|_| {}).unwrap();
    let b = import_library_rom(&source, &root.join("us/B"), None, &|_| {}).unwrap();
    assert_eq!(a.filename, "cart.pce.m");
    assert_eq!(a.filename, b.filename);
    assert_eq!(
        fs::read_dir(root.join("published/roms")).unwrap().count(),
        1
    );
    assert_eq!(fs::read_dir(root.join("jp/A")).unwrap().count(), 0);
    assert_eq!(fs::read(&source).unwrap(), b"original cartridge");
    fs::write(&source, b"replacement cartridge").unwrap();
    let c = import_library_rom(&source, &root.join("jp/A"), None, &|_| {}).unwrap();
    assert_ne!(c.filename, a.filename);
    for (dir, name) in [("jp/A", &c.filename), ("us/B", &b.filename)] {
        fs::write(
            root.join(dir).join("game.json"),
            serde_json::to_vec(&serde_json::json!({"rom":{"rom":name}})).unwrap(),
        )
        .unwrap();
    }
    remove_replaced_rom(&root.join("jp/A"), &a.filename, &c.filename).unwrap();
    assert!(root.join("published/roms").join(&a.filename).exists());
    fs::write(
        root.join("us/B/game.json"),
        serde_json::to_vec(&serde_json::json!({"rom":{"rom":c.filename}})).unwrap(),
    )
    .unwrap();
    remove_replaced_rom(&root.join("us/B"), &b.filename, &c.filename).unwrap();
    assert!(!root.join("published/roms").join(&a.filename).exists());
    assert!(root.join("published/roms").join(&c.filename).exists());
}

#[test]
fn packed_shared_import_preserves_bytes_and_uses_console_suffix() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    fs::create_dir_all(root.join("jp/A")).unwrap();
    fs::write(root.join("jp/gamelist.json"), "[]").unwrap();
    let name = "Original.PCE.M";
    let source = root.join(name);
    let bytes = m2_mzs::pack_default(b"original", name).unwrap();
    fs::write(&source, &bytes).unwrap();
    let first = import_library_rom(&source, &root.join("jp/A"), None, &|_| {}).unwrap();
    assert_eq!(first.filename, "Original.PCE.m");
    let second = import_library_rom(&source, &root.join("jp/A"), None, &|_| {}).unwrap();
    assert_eq!(first.filename, second.filename);
    let stored = root.join("published/roms").join(&first.filename);
    assert_eq!(fs::read(&stored).unwrap(), bytes);
    fs::write(&source, m2_mzs::pack_default(b"different", name).unwrap()).unwrap();
    assert!(import_library_rom(&source, &root.join("jp/A"), None, &|_| {}).is_err());
    assert_eq!(fs::read(stored).unwrap(), bytes);
}
