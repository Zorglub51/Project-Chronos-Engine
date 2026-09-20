//! Read game files from a directory, a P9 image or a full NAND MBR image.
use anyhow::{ensure, Context, Result};
use ext4_view::{Ext4, Ext4Read};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
};

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}
enum Tree {
    Directory(PathBuf),
    Image(Ext4, String),
}
pub struct Source {
    tree: Tree,
    files: BTreeMap<String, u64>,
    packed: BTreeMap<String, (u64, u64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn paths_and_partition_bounds_are_checked() {
        for path in ["../escape", "/absolute", "a/../b", "C:\\escape", "", "a\\b"] {
            assert!(safe_relative(path).is_err(), "{path}");
        }
        assert!(safe_relative("system/roms/Game.pcd").is_ok());
        let mut temp = tempfile::tempfile().unwrap();
        temp.write_all(&[1; 1024]).unwrap();
        let mut reader = PartitionReader {
            file: temp,
            offset: 512,
            length: 512,
        };
        let mut bytes = [0; 16];
        assert!(Ext4Read::read(&mut reader, 496, &mut bytes).is_ok());
        assert!(Ext4Read::read(&mut reader, 497, &mut bytes).is_err());
        assert!(Ext4Read::read(&mut reader, u64::MAX, &mut bytes).is_err());
    }
    #[test]
    fn full_dump_uses_ebr_addresses_not_assumed_stock_offsets() {
        let mut bytes = vec![0; 12 * 512];
        let set_entry =
            |bytes: &mut [u8], sector: usize, slot: usize, kind: u8, start: u32, size: u32| {
                let base = sector * 512;
                bytes[base + 510..base + 512].copy_from_slice(&[0x55, 0xaa]);
                let o = base + 446 + slot * 16;
                bytes[o + 4] = kind;
                bytes[o + 8..o + 12].copy_from_slice(&start.to_le_bytes());
                bytes[o + 12..o + 16].copy_from_slice(&size.to_le_bytes());
            };
        set_entry(&mut bytes, 0, 2, 5, 1, 11);
        for i in 0..5 {
            set_entry(&mut bytes, 1 + i, 0, 0x83, 1, 1);
            if i < 4 {
                set_entry(&mut bytes, 1 + i, 1, 5, (i + 1) as u32, 10);
            }
        }
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&bytes).unwrap();
        assert_eq!(partition9(&mut file).unwrap(), (6 * 512, 512));
        // Point the second EBR back to itself.
        set_entry(&mut bytes, 2, 1, 5, 1, 10);
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&bytes).unwrap();
        assert!(partition9(&mut file)
            .unwrap_err()
            .to_string()
            .contains("Cyclic"));
    }
}

pub(crate) fn safe_relative(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    ensure!(
        !name.is_empty()
            && !name.contains(['\\', ':'])
            && path.components().all(|p| matches!(p, Component::Normal(_))),
        "Invalid archive path: {name}"
    );
    Ok(path)
}

struct PartitionReader {
    file: File,
    offset: u64,
    length: u64,
}
impl Ext4Read for PartitionReader {
    fn read(
        &mut self,
        start: u64,
        dst: &mut [u8],
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if start
            .checked_add(dst.len() as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }
        self.file.seek(SeekFrom::Start(self.offset + start))?;
        self.file.read_exact(dst)?;
        Ok(())
    }
}

fn sector(file: &mut File, lba: u64) -> Result<[u8; 512]> {
    let mut bytes = [0; 512];
    file.seek(SeekFrom::Start(
        lba.checked_mul(512).context("Partition address overflow")?,
    ))?;
    file.read_exact(&mut bytes)?;
    ensure!(
        bytes[510..] == [0x55, 0xaa],
        "No valid MBR/EBR partition table at sector {lba}"
    );
    Ok(bytes)
}
fn entry(bytes: &[u8], slot: usize) -> (u8, u64, u64) {
    let o = 446 + slot * 16;
    (
        bytes[o + 4],
        u32::from_le_bytes(bytes[o + 8..o + 12].try_into().unwrap()) as u64,
        u32::from_le_bytes(bytes[o + 12..o + 16].try_into().unwrap()) as u64,
    )
}
fn partition9(file: &mut File) -> Result<(u64, u64)> {
    let mbr = sector(file, 0)?;
    let base = (0..4)
        .map(|i| entry(&mbr, i))
        .find(|e| matches!(e.0, 0x05 | 0x0f | 0x85))
        .context("Full dump has no extended partition table; select the P9 image instead")?
        .1;
    let mut current = base;
    let mut visited = HashSet::new();
    for id in 5..=9 {
        ensure!(visited.insert(current), "Cyclic partition table");
        let ebr = sector(file, current)?;
        let (kind, start, size) = entry(&ebr, 0);
        ensure!(kind != 0 && size > 0, "Partition P{id} is missing");
        if id == 9 {
            return Ok(((current + start) * 512, size * 512));
        }
        let (kind, next, _) = entry(&ebr, 1);
        ensure!(
            matches!(kind, 0x05 | 0x0f | 0x85) && next > 0,
            "P9 is missing from the full dump"
        );
        current = base + next;
    }
    unreachable!()
}
fn p9_name(path: &Path) -> bool {
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    ["mmcblk0p9", "p9", "partition9", "partition_9"].contains(&name.as_str())
        && ["bin", "img", "ext4", "raw"].contains(&extension.as_str())
}

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        let tree = if path.is_dir() {
            let candidates = ["", "game", "usr/game", "p9", "P9", "BACKUP/game"];
            if let Some(root) = candidates
                .iter()
                .map(|s| path.join(s))
                .find(|p| p.join("m2engage").is_file())
            {
                Tree::Directory(root.canonicalize()?)
            } else {
                let mut images = fs::read_dir(path)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && p9_name(p))
                    .collect::<Vec<_>>();
                images.sort();
                ensure!(images.len() == 1, "Select an extracted P9/game folder, or a folder containing exactly one mmcblk0p9.bin (found {}).", images.len());
                return Self::open(&images[0]);
            }
        } else {
            let mut file =
                File::open(path).with_context(|| format!("Open dump {}", path.display()))?;
            let total = file.metadata()?.len();
            ensure!(total >= 2048, "Dump is too short");
            file.seek(SeekFrom::Start(1080))?;
            let mut magic = [0; 2];
            file.read_exact(&mut magic)?;
            let (offset, length) = if magic == [0x53, 0xef] {
                (0, total)
            } else {
                partition9(&mut file)?
            };
            ensure!(
                offset.checked_add(length).is_some_and(|end| end <= total),
                "Truncated dump: P9 extends beyond end of file"
            );
            let fs = Ext4::load(Box::new(PartitionReader {
                file,
                offset,
                length,
            }))
            .context("Cannot read P9 filesystem")?;
            let root = ["", "/game", "/usr/game"]
                .into_iter()
                .find(|p| fs.exists(format!("{p}/m2engage").as_str()).unwrap_or(false))
                .context(
                    "This image contains no m2engage. Select the original Games partition (P9).",
                )?;
            Tree::Image(fs, root.into())
        };
        let mut source = Source {
            tree,
            files: BTreeMap::new(),
            packed: BTreeMap::new(),
        };
        source.walk("", 0)?;
        if source.files.contains_key("alldata.bin") {
            ensure!(
                source.files.contains_key("alldata.psb.m"),
                "alldata.bin requires its alldata.psb.m index"
            );
            let index = source.read_loose("alldata.psb.m", 16 * 1024 * 1024)?;
            let index = m2_mzs::unpack_default(&index, "alldata.psb.m")?;
            let index = m2_psb::read(&index)?.to_json();
            let entries = index["file_info"]
                .as_object()
                .context("alldata index has no file_info table")?;
            let archive_size = source.files["alldata.bin"];
            for (name, range) in entries {
                safe_relative(name)?;
                let range = range
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .context("Invalid alldata range")?;
                let start = range[0].as_u64().context("Invalid alldata offset")?;
                let length = range[1].as_u64().context("Invalid alldata length")?;
                ensure!(
                    start
                        .checked_add(length)
                        .is_some_and(|end| end <= archive_size),
                    "Truncated alldata.bin: {name}"
                );
                ensure!(
                    !source.files.contains_key(name),
                    "Ambiguous source: {name} exists both loose and in alldata.bin"
                );
                source.packed.insert(name.clone(), (start, length));
            }
        }
        Ok(source)
    }
    fn walk(&mut self, relative: &str, depth: usize) -> Result<()> {
        ensure!(depth < 32, "Too many nested directories in dump");
        let mut children = Vec::new();
        match &self.tree {
            Tree::Directory(root) => {
                for entry in fs::read_dir(root.join(relative))? {
                    let entry = entry?;
                    let name = entry
                        .file_name()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("Non-UTF8 dump filename"))?;
                    let rel = if relative.is_empty() {
                        name
                    } else {
                        format!("{relative}/{name}")
                    };
                    if rel == "save" || rel == "lost+found" || rel == ".DS_Store" {
                        continue;
                    }
                    let metadata = fs::metadata(entry.path())?;
                    // Reject escaping symlinks, allow stock libopus.so.0 -> libopus.so.0.x.
                    ensure!(
                        entry.path().canonicalize()?.starts_with(root),
                        "Link leaves the source folder: {rel}"
                    );
                    children.push((rel, metadata.is_dir(), metadata.is_file(), metadata.len()));
                }
            }
            Tree::Image(fs, root) => {
                for entry in fs.read_dir(format!("{root}/{relative}").as_str())? {
                    let entry = entry?;
                    let name = entry.file_name().as_str()?.to_owned();
                    if name == "." || name == ".." || name == "lost+found" {
                        continue;
                    }
                    let rel = if relative.is_empty() {
                        name
                    } else {
                        format!("{relative}/{name}")
                    };
                    if rel == "save" {
                        continue;
                    }
                    let metadata = fs
                        .metadata(format!("{root}/{rel}").as_str())
                        .with_context(|| format!("Read P9 entry {rel}"))?;
                    children.push((
                        rel,
                        metadata.is_dir(),
                        metadata.file_type().is_regular_file(),
                        metadata.len(),
                    ));
                }
            }
        }
        for (rel, dir, file, size) in children {
            safe_relative(&rel)?;
            if dir {
                self.walk(&rel, depth + 1)?;
            } else if file {
                self.files.insert(rel, size);
            }
            ensure!(self.files.len() < 100_000, "Dump contains too many files");
        }
        Ok(())
    }
    fn open_loose(&self, relative: &str) -> Result<Box<dyn ReadSeek>> {
        safe_relative(relative)?;
        match &self.tree {
            Tree::Directory(root) => Ok(Box::new(File::open(root.join(relative))?)),
            Tree::Image(fs, root) => Ok(Box::new(fs.open(format!("{root}/{relative}").as_str())?)),
        }
    }
    fn read_loose(&self, relative: &str, limit: u64) -> Result<Vec<u8>> {
        ensure!(
            self.files.get(relative).is_some_and(|size| *size <= limit),
            "Missing or oversized file: {relative}"
        );
        let mut result = Vec::new();
        self.open_loose(relative)?
            .take(limit + 1)
            .read_to_end(&mut result)?;
        Ok(result)
    }
    pub fn size(&self, relative: &str) -> Option<u64> {
        self.packed
            .get(relative)
            .map(|r| r.1)
            .or_else(|| self.files.get(relative).copied())
    }
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.files.keys().chain(self.packed.keys())
    }
    pub fn reader(&self, relative: &str) -> Result<Box<dyn Read>> {
        if let Some((offset, length)) = self.packed.get(relative) {
            let mut reader = self.open_loose("alldata.bin")?;
            reader.seek(SeekFrom::Start(*offset))?;
            Ok(Box::new(reader.take(*length)))
        } else {
            Ok(Box::new(self.open_loose(relative)?))
        }
    }
    pub fn read(&self, relative: &str, limit: u64) -> Result<Vec<u8>> {
        ensure!(
            self.size(relative).is_some_and(|size| size <= limit),
            "Missing or oversized file: {relative}"
        );
        let mut bytes = Vec::new();
        self.reader(relative)?
            .take(limit + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == self.size(relative).unwrap(),
            "Short read: {relative}"
        );
        Ok(bytes)
    }
    pub fn copy(&self, relative: &str, destination: &Path) -> Result<()> {
        let length = self
            .size(relative)
            .with_context(|| format!("Missing dump file: {relative}"))?;
        fs::create_dir_all(destination.parent().unwrap())?;
        let mut output = File::create(destination)?;
        let copied = std::io::copy(&mut self.reader(relative)?, &mut output)?;
        ensure!(copied == length, "Short read: {relative}");
        Ok(())
    }
}
