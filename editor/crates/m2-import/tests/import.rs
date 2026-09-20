use m2_import::{bios_status, configure_bios, import_rom};
use pcd_core::PcdArchive;
use std::{cell::RefCell, fs};

#[test]
fn cue_conversion_bios_extraction_and_direct_pcd_copy() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let super_path = root.join("super.pce");
    let system_path = root.join("system.pce");
    fs::write(&super_path, vec![0x12; 262144]).unwrap();
    fs::write(&system_path, vec![0x34; 262144]).unwrap();
    let bios = configure_bios(
        None,
        Some(&super_path),
        Some(&system_path),
        &root.join("bios"),
    )
    .unwrap();
    fs::remove_file(&super_path).unwrap();
    fs::remove_file(&system_path).unwrap();
    assert!(bios_status(Some(&bios)).ready); // Independent of removable source.
    let cue = root.join("Game.CUE");
    fs::write(&cue,"FILE \"Track 01.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\nFILE \"Track 02.bin\" BINARY\n TRACK 02 AUDIO\n INDEX 01 00:00:00\n").unwrap();
    let data: Vec<u8> = (0..2048 * 16).map(|i| (i % 251) as u8).collect();
    let mut raw = vec![0; 2352 * 16];
    for (n, sector) in data.chunks(2048).enumerate() {
        raw[n * 2352 + 16..n * 2352 + 2064].copy_from_slice(sector);
    }
    fs::write(root.join("Track 01.bin"), &raw).unwrap();
    fs::write(root.join("Track 02.bin"), vec![0; 2352 * 75]).unwrap();
    let progress = RefCell::new(Vec::new());
    let first = import_rom(&cue, &root.join("game"), Some(&bios), &|p| {
        progress.borrow_mut().push(p)
    })
    .unwrap();
    assert!(first.converted && first.cd);
    assert_eq!(first.filename, "Game.pcd");
    let output = root.join("game").join(&first.filename);
    let archive = PcdArchive::open(&output).unwrap();
    assert_eq!(archive.info()[0].tracks.len(), 2);
    assert_eq!(archive.data_chunk(0).unwrap(), data);
    assert!(progress
        .borrow()
        .iter()
        .any(|p| p.message.contains("audio")));
    let restored = configure_bios(Some(&output), None, None, &root.join("other-bios")).unwrap();
    assert_eq!(fs::read(restored.super_path).unwrap(), vec![0x12; 262144]);
    assert_eq!(fs::read(restored.system_path).unwrap(), vec![0x34; 262144]);
    // PCD import requires no BIOS and preserves the exact archive bytes.
    let copied = import_rom(&output, &root.join("copy"), None, &|_| {}).unwrap();
    assert!(!copied.converted && copied.cd);
    assert_eq!(
        fs::read(root.join("copy").join(copied.filename)).unwrap(),
        fs::read(&output).unwrap()
    );
    let previous = fs::read(&output).unwrap();
    let second = import_rom(&cue, &root.join("game"), Some(&bios), &|_| {}).unwrap();
    assert_eq!(second.filename, "Game (2).pcd");
    assert_eq!(fs::read(output).unwrap(), previous);
}

#[test]
fn errors_preserve_existing_files_and_leave_no_partial_import() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let destination = root.join("game");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep.pcd"), "old ROM").unwrap();
    let cue = root.join("game.cue");
    fs::write(
        &cue,
        "FILE \"missing.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n",
    )
    .unwrap();
    assert!(import_rom(&cue, &destination, None, &|_| {})
        .unwrap_err()
        .to_string()
        .contains("BIOS"));
    let a = root.join("a");
    fs::write(&a, vec![1; 262144]).unwrap();
    let bios = configure_bios(None, Some(&a), Some(&a), &root.join("bios")).unwrap();
    assert!(format!(
        "{:#}",
        import_rom(&cue, &destination, Some(&bios), &|_| {}).unwrap_err()
    )
    .contains("missing.bin"));
    fs::write(root.join("missing.bin"), vec![1; 31]).unwrap();
    assert!(import_rom(&cue, &destination, Some(&bios), &|_| {}).is_err());
    let invalid = root.join("invalid.pcd");
    fs::write(&invalid, "not a PCD").unwrap();
    assert!(import_rom(&invalid, &destination, None, &|_| {}).is_err());
    assert!(
        import_rom(&root.join("missing.bin"), &destination, None, &|_| {})
            .unwrap_err()
            .to_string()
            .contains(".cue")
    );
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    assert_eq!(fs::read(destination.join("keep.pcd")).unwrap(), b"old ROM");
    fs::write(&a, vec![0; 262144]).unwrap();
    assert!(configure_bios(None, Some(&a), Some(&a), &root.join("bios")).is_err());
    fs::write(&a, vec![1; 10]).unwrap();
    assert!(configure_bios(None, Some(&a), Some(&a), &root.join("bios")).is_err());
}

#[test]
fn invalid_library_sources_do_not_create_or_replace_destinations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("p9.bin");
    fs::write(&source, vec![0; 4096]).unwrap();
    let dest = root.join("new");
    assert!(m2_import::create_library(&source, &dest, false, &root.join("bios"), &|_| {}).is_err());
    assert!(!dest.exists());
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("keep"), "data").unwrap();
    assert!(
        m2_import::create_library(&source, &dest, false, &root.join("bios"), &|_| {})
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    assert_eq!(fs::read_to_string(dest.join("keep")).unwrap(), "data");
}
