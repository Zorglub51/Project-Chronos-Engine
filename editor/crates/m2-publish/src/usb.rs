//! Files that accompany a published library on a FAT32 USB stick.
//! Original M2 binaries/resources are supplied by the user, never bundled.

use crate::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const ASSETS: &[(&str, &[u8])] = &[
    (
        "system/script/const.nut.m",
        include_bytes!("../../../../mod-assets/scripts-built/const.nut.m"),
    ),
    (
        "system/script/mode_demo.nut.m",
        include_bytes!("../../../../mod-assets/scripts-built/mode_demo.nut.m"),
    ),
    (
        "system/script/mode_title_select.nut.m",
        include_bytes!("../../../../mod-assets/scripts-built/mode_title_select.nut.m"),
    ),
    (
        "system/script/utils.nut.m",
        include_bytes!("../../../../mod-assets/scripts-built/utils.nut.m"),
    ),
    (
        "lib/m2hook_print.so",
        include_bytes!("../../../../mod-assets/lib/m2hook_print.so"),
    ),
];

const ORIGINAL_FILES: &[&str] = &[
    "m2engage",
    "libopus.so.0",
    "version",
    "shutdown.png",
    "system/script/init.nut.m",
    "system/config/system_prof.psb.m",
    "040/config/title_prof.psb.m",
    "040/config/title_mode_top.psb.m",
    "040/motion/title_jp_titleselect_jp.psb.m",
    "040/motion/title_jp_titleselect_us.psb.m",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbPreparation {
    pub game_root: PathBuf,
    pub assets_updated: usize,
    /// This is a presence check, not a certification of the stock runtime.
    pub missing_original_files: Vec<String>,
}

/// Only the console's canonical layout implies a sibling game/ directory.
/// A standalone/legacy export must not write outside its output directory.
pub fn usb_root(output: &Path) -> Option<&Path> {
    let library = output.parent()?;
    if output.file_name()? != "published" || library.file_name()? != "library" {
        return None;
    }
    library.parent()
}

fn checked_directory(path: &Path) -> Result<(), Error> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(Error::Library(format!(
                "expected a directory, not a link or file: {}",
                path.display()
            )));
        }
    }
    fs::create_dir_all(path)?;
    Ok(())
}

pub fn prepare_usb(output: &Path) -> Result<Option<UsbPreparation>, Error> {
    let Some(root) = usb_root(output) else {
        return Ok(None);
    };
    let game = root.join("game");
    for relative in ["", "system", "system/script", "system/roms", "lib", "save"] {
        checked_directory(&game.join(relative))?;
    }

    let mut assets_updated = 0;
    for (relative, bytes) in ASSETS {
        let destination = game.join(relative);
        if fs::symlink_metadata(&destination).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Library(format!(
                "refusing to replace a linked asset: {}",
                destination.display()
            )));
        }
        if fs::read(&destination).is_ok_and(|existing| existing == *bytes) {
            continue;
        }
        // Readers see either the previous complete asset or the new one.
        let temporary = destination.with_extension("chronos-tmp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        use std::io::Write;
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = written.and_then(|()| fs::rename(&temporary, &destination)) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        assets_updated += 1;
    }

    // No symlinks and no ROM copies: the console mounts published/roms here.
    // Keep any legacy files and existing saves; never delete user content.
    let missing_original_files = ORIGINAL_FILES
        .iter()
        .filter(|name| !game.join(name).is_file())
        .map(|name| format!("game/{name}"))
        .collect();
    Ok(Some(UsbPreparation {
        game_root: game,
        assets_updated,
        missing_original_files,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "chronos-usb-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn canonical_publish_installs_assets_without_copying_roms_or_changing_saves() {
        let tmp = Temp::new();
        let output = tmp.0.join("library/published");
        fs::create_dir_all(output.join("roms")).unwrap();
        fs::write(output.join("roms/test.pce.m"), b"published ROM").unwrap();
        fs::create_dir_all(tmp.0.join("game/save")).unwrap();
        fs::write(tmp.0.join("game/save/data_008_0000.bin"), b"user save").unwrap();
        let first = prepare_usb(&output).unwrap().unwrap();
        assert_eq!(first.assets_updated, 5);
        assert!(first
            .missing_original_files
            .contains(&"game/m2engage".to_owned()));
        assert_eq!(
            fs::read_dir(tmp.0.join("game/system/roms"))
                .unwrap()
                .count(),
            0
        );
        for (relative, bytes) in ASSETS {
            assert_eq!(fs::read(tmp.0.join("game").join(relative)).unwrap(), *bytes);
        }
        let second = prepare_usb(&output).unwrap().unwrap();
        assert_eq!(second.assets_updated, 0);
        assert_eq!(
            fs::read(output.join("roms/test.pce.m")).unwrap(),
            b"published ROM"
        );
        assert_eq!(
            fs::read(tmp.0.join("game/save/data_008_0000.bin")).unwrap(),
            b"user save"
        );
        // A legacy ROM directory is preserved too; its files will be hidden
        // by the console mount, not removed by publication.
        fs::write(tmp.0.join("game/system/roms/old.pce.m"), b"old ROM").unwrap();
        prepare_usb(&output).unwrap();
        assert_eq!(
            fs::read(tmp.0.join("game/system/roms/old.pce.m")).unwrap(),
            b"old ROM"
        );
    }

    #[test]
    fn legacy_export_does_not_create_a_game_directory() {
        let tmp = Temp::new();
        assert!(prepare_usb(&tmp.0.join("published")).unwrap().is_none());
        assert!(!tmp.0.join("game").exists());
    }

    #[cfg(unix)]
    #[test]
    fn linked_rom_directory_is_rejected_without_following_it() {
        let tmp = Temp::new();
        let outside = Temp::new();
        fs::create_dir_all(tmp.0.join("game/system")).unwrap();
        std::os::unix::fs::symlink(&outside.0, tmp.0.join("game/system/roms")).unwrap();
        assert!(prepare_usb(&tmp.0.join("library/published")).is_err());
        assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 0);
    }
}
