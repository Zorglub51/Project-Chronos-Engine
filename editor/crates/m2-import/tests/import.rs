use m2_import::{bios_status, configure_bios, import_rom};
use pcd_core::PcdArchive;
use std::{cell::RefCell, fs};

#[test]
fn cue_conversion_bios_extraction_and_direct_pcd_copy() {
    let temp = tempfile::Builder::new()
        .prefix("Chronos CD é ")
        .tempdir()
        .unwrap();
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
    // Real stereo signal: silence alone cannot detect a reset of the Opus
    // encoder at each one-second PCD boundary.
    let audio: Vec<i16> = (0..44100 * 3)
        .flat_map(|n| {
            [437.37, 997.23].map(|frequency| {
                (0.4 * (n as f64 / 44100.0 * std::f64::consts::TAU * frequency).sin() * 32768.0)
                    as i16
            })
        })
        .collect();
    let audio_bytes: Vec<u8> = audio.iter().flat_map(|v| v.to_le_bytes()).collect();
    fs::write(root.join("Track 02.bin"), &audio_bytes).unwrap();
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
    let extracted = pcd_core::extract_pecd(pcd_core::ExtractOptions {
        input: &output,
        out_dir: &root.join("audio-check"),
        write_empty: false,
        dump: false,
        progress: None,
    })
    .unwrap();
    let restored_bin = fs::read(&extracted[0].bin).unwrap();
    assert_eq!(restored_bin.len(), raw.len() + audio_bytes.len());
    let restored_audio = &restored_bin[raw.len()..];
    for second in 1..=3 {
        let boundary = second * 44100 - 16 * 588;
        for channel in 0..2 {
            let (mut error, mut energy) = (0.0f64, 0.0f64);
            for frame in boundary - 441..boundary + 441 {
                let i = frame * 2 + channel;
                let actual = i16::from_le_bytes([restored_audio[i * 2], restored_audio[i * 2 + 1]]);
                error += (f64::from(actual) - f64::from(audio[i])).powi(2);
                energy += f64::from(audio[i]).powi(2);
            }
            assert!(
                error / energy < 0.025,
                "CD audio discontinuity at {second}s, channel {channel}: {}",
                error / energy
            );
        }
    }
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
