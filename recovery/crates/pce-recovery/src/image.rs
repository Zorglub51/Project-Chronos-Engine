//! Local restore-image preparation. Never opens a connection to the console.
//!
//! ZIP members are read in memory, never extracted to paths from the archive.
//! The complete payload (including CRC and length) is validated before it can
//! be handed to the partition writer.

use std::fs::File;
use std::io::{self, Read, Seek};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zip::ZipArchive;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageEntry {
    /// Zero-based archive index; None denotes a raw image.
    pub zip_index: Option<usize>,
    pub name: String,
    pub size: u64,
    pub crc32: Option<u32>,
}

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("image I/O: {0}")]
    Io(#[from] io::Error),
    #[error("invalid or unsupported ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("size mismatch: image is {actual} bytes, partition expects {expected} bytes")]
    Size { actual: u64, expected: u64 },
    #[error("ZIP contains no regular image matching {0} bytes (uncompressed)")]
    NoMatchingImage(u64),
    #[error("multiple images match; choose an entry explicitly: {0}")]
    Ambiguous(String),
    #[error("selected image is no longer present or has changed; select the file again")]
    SelectionChanged,
    #[error("image is too large to load in memory")]
    TooLarge,
    #[error("empty images cannot be restored")]
    Empty,
}

enum Source {
    Raw(File, ImageEntry),
    Zip(ZipArchive<File>),
}

impl Source {
    fn open(path: &Path) -> Result<Self, ImageError> {
        let mut file = File::open(path)?;
        let mut signature = [0; 4];
        let count = file.read(&mut signature)?;
        file.rewind()?;
        let zip_extension = path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("zip"));
        let zip_signature =
            count == 4 && matches!(&signature, b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08");
        if zip_extension || zip_signature {
            Ok(Self::Zip(ZipArchive::new(file)?))
        } else {
            let entry = ImageEntry {
                zip_index: None,
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                size: file.metadata()?.len(),
                crc32: None,
            };
            Ok(Self::Raw(file, entry))
        }
    }

    fn candidates(
        &mut self,
        expected: u64,
        allow_mismatch: bool,
    ) -> Result<Vec<ImageEntry>, ImageError> {
        match self {
            Self::Raw(_, entry) => {
                check_size(entry.size, expected, allow_mismatch)?;
                Ok(vec![entry.clone()])
            }
            Self::Zip(archive) => {
                let mut entries = Vec::new();
                for index in 0..archive.len() {
                    // Metadata only: do not decompress unrelated members.
                    let file = archive.by_index_raw(index)?;
                    let name = file.name();
                    if !file.is_file()
                        || file.enclosed_name().is_none()
                        || name
                            .split('/')
                            .any(|part| part == "__MACOSX" || part.starts_with("._"))
                        || file.size() == 0
                    {
                        continue;
                    }
                    if allow_mismatch || file.size() == expected {
                        entries.push(ImageEntry {
                            zip_index: Some(index),
                            name: name.to_owned(),
                            size: file.size(),
                            crc32: Some(file.crc32()),
                        });
                    }
                }
                if entries.is_empty() {
                    return Err(ImageError::NoMatchingImage(expected));
                }
                Ok(entries)
            }
        }
    }
}

fn check_size(actual: u64, expected: u64, allow_mismatch: bool) -> Result<(), ImageError> {
    if actual == 0 {
        return Err(ImageError::Empty);
    }
    if actual != expected && !allow_mismatch {
        return Err(ImageError::Size { actual, expected });
    }
    Ok(())
}

/// Inspect local metadata before asking the user to confirm a restore.
/// Matching uses the uncompressed size, independent of the member extension.
pub fn inspect_image(
    path: &Path,
    expected: u64,
    allow_mismatch: bool,
) -> Result<Vec<ImageEntry>, ImageError> {
    Source::open(path)?.candidates(expected, allow_mismatch)
}

/// Recheck the selection, then read the complete image and validate its CRC and
/// exact length. A caller must wait for success before writing any bytes.
/// `selection = None` is accepted only when precisely one image matches.
pub fn load_image(
    path: &Path,
    expected: u64,
    allow_mismatch: bool,
    selection: Option<&ImageEntry>,
    on_progress: impl FnMut(u64),
) -> Result<Vec<u8>, ImageError> {
    let mut source = Source::open(path)?;
    let candidates = source.candidates(expected, allow_mismatch)?;
    let selected = match selection {
        Some(entry) => candidates
            .iter()
            .find(|item| *item == entry)
            .ok_or(ImageError::SelectionChanged)?,
        None if candidates.len() == 1 => &candidates[0],
        None => {
            return Err(ImageError::Ambiguous(
                candidates
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}: {} ({} bytes)",
                            entry.zip_index.unwrap(),
                            entry.name,
                            entry.size
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ))
        }
    };
    match source {
        Source::Raw(file, _) => read_verified(file, selected.size, on_progress),
        Source::Zip(mut archive) => {
            let file = archive.by_index(selected.zip_index.ok_or(ImageError::SelectionChanged)?)?;
            read_verified(file, selected.size, on_progress)
        }
    }
}

fn read_verified(
    mut reader: impl Read,
    size: u64,
    mut on_progress: impl FnMut(u64),
) -> Result<Vec<u8>, ImageError> {
    let capacity = usize::try_from(size).map_err(|_| ImageError::TooLarge)?;
    let mut data = Vec::new();
    data.try_reserve_exact(capacity)
        .map_err(|_| ImageError::TooLarge)?;
    let mut buffer = [0; 64 * 1024];
    on_progress(0);
    loop {
        // One extra byte allows us to detect dishonest ZIP sizes, without
        // unbounded decompression or allocating a second image-sized buffer.
        let remaining = size - data.len() as u64;
        let limit = remaining.saturating_add(1).min(buffer.len() as u64) as usize;
        let count = match reader.read(&mut buffer[..limit]) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        } // Also forces the ZIP reader's EOF CRC check.
        let actual = data.len() as u64 + count as u64;
        if actual > size {
            return Err(ImageError::Size {
                actual,
                expected: size,
            });
        }
        data.extend_from_slice(&buffer[..count]);
        on_progress(actual);
    }
    check_size(data.len() as u64, size, false)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    fn archive(entries: &[(&str, &[u8])], method: CompressionMethod, zip64: bool) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .large_file(zip64);
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn fixture(name: &str, bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn raw_images_keep_working_and_mismatch_is_rejected() {
        let (_dir, path) = fixture("p7.mod", b"image");
        assert_eq!(load_image(&path, 5, false, None, |_| {}).unwrap(), b"image");
        assert!(matches!(
            inspect_image(&path, 6, false),
            Err(ImageError::Size { .. })
        ));
        assert_eq!(load_image(&path, 6, true, None, |_| {}).unwrap(), b"image");
    }

    #[test]
    fn reads_stored_deflate_and_zip64_and_ignores_metadata() {
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            for zip64 in [false, true] {
                let bytes = archive(
                    &[
                        ("LIRE-MOI.txt", b"instructions"),
                        ("nested/p7.bin", b"image"),
                        ("__MACOSX/._p7.bin", b"other"),
                    ],
                    method,
                    zip64,
                );
                // Signature detection also works without a .zip extension.
                let (_dir, path) = fixture("backup", &bytes);
                let entries = inspect_image(&path, 5, false).unwrap();
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "nested/p7.bin");
                let mut last = 0;
                assert_eq!(
                    load_image(&path, 5, false, Some(&entries[0]), |n| last = n).unwrap(),
                    b"image"
                );
                assert_eq!(last, 5);
            }
        }
    }

    #[test]
    fn multiple_images_require_an_explicit_selection() {
        let bytes = archive(
            &[("stock.bin", b"stock"), ("mod.bin", b"mod!!")],
            CompressionMethod::Deflated,
            false,
        );
        let (_dir, path) = fixture("p7.ZIP", &bytes);
        assert!(matches!(
            load_image(&path, 5, false, None, |_| {}),
            Err(ImageError::Ambiguous(_))
        ));
        let entries = inspect_image(&path, 5, false).unwrap();
        assert_eq!(
            load_image(&path, 5, false, Some(&entries[1]), |_| {}).unwrap(),
            b"mod!!"
        );
    }

    #[test]
    fn rejects_wrong_partition_and_bad_archive_without_raw_fallback() {
        let bytes = archive(
            &[("p8.bin", b"wrong partition")],
            CompressionMethod::Stored,
            false,
        );
        let (_dir, path) = fixture("p8.zip", &bytes);
        assert!(matches!(
            inspect_image(&path, 5, false),
            Err(ImageError::NoMatchingImage(5))
        ));
        std::fs::write(&path, b"not a zip").unwrap();
        assert!(matches!(
            inspect_image(&path, 9, false),
            Err(ImageError::Zip(_))
        ));
    }

    #[test]
    fn crc_errors_are_detected_even_after_all_payload_bytes_were_read() {
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let mut bytes = archive(&[("p7.bin", b"image")], method, false);
            let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
            bytes[14] ^= 1; // Local CRC.
            bytes[central + 16] ^= 1; // Central CRC.
            let (_dir, path) = fixture("corrupt.zip", &bytes);
            assert!(load_image(&path, 5, false, None, |_| {}).is_err());
        }
    }

    #[test]
    fn rejects_truncated_archives() {
        let mut bytes = archive(&[("p7.bin", b"image")], CompressionMethod::Deflated, false);
        bytes.truncate(bytes.len() - 10);
        let (_dir, path) = fixture("truncated.zip", &bytes);
        assert!(load_image(&path, 5, false, None, |_| {}).is_err());
    }

    #[test]
    fn rejects_changed_selection_before_loading() {
        let (_dir, path) = fixture(
            "p7.zip",
            &archive(&[("p7.bin", b"image")], CompressionMethod::Stored, false),
        );
        let selected = inspect_image(&path, 5, false).unwrap().remove(0);
        std::fs::write(
            &path,
            archive(&[("p7.bin", b"other")], CompressionMethod::Stored, false),
        )
        .unwrap();
        assert!(matches!(
            load_image(&path, 5, false, Some(&selected), |_| {}),
            Err(ImageError::SelectionChanged)
        ));
    }

    #[test]
    fn bounds_decompression_and_rejects_truncated_payloads() {
        let mut last = 0;
        assert!(matches!(
            read_verified(Cursor::new(vec![1; 100_000]), 10, |n| last = n),
            Err(ImageError::Size { .. })
        ));
        assert_eq!(last, 0);
        assert!(matches!(
            read_verified(Cursor::new(b"short"), 10, |_| {}),
            Err(ImageError::Size { .. })
        ));
    }

    #[test]
    fn dishonest_uncompressed_size_is_rejected() {
        for declared in [4u32, 6u32] {
            let mut bytes = archive(&[("p7.bin", b"image")], CompressionMethod::Deflated, false);
            let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
            bytes[22..26].copy_from_slice(&declared.to_le_bytes());
            bytes[central + 24..central + 28].copy_from_slice(&declared.to_le_bytes());
            let (_dir, path) = fixture("dishonest.zip", &bytes);
            assert!(load_image(&path, declared as u64, false, None, |_| {}).is_err());
        }
    }

    #[test]
    fn excludes_symlinks_directories_and_unsafe_paths() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.add_directory("directory/", options).unwrap();
        writer.add_symlink("link.bin", "image", options).unwrap();
        writer.start_file("../escape.bin", options).unwrap();
        writer.write_all(b"image").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let (_dir, path) = fixture("paths.zip", &bytes);
        assert!(matches!(
            inspect_image(&path, 5, false),
            Err(ImageError::NoMatchingImage(5))
        ));
    }
}
