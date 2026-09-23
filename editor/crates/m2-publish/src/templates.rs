// Stock PSB templates from a reference alldata directory. We patch these
// rather than authoring PSBs from scratch — the schemas are non-trivial and
// we'd risk drift from what m2engage expects.

use crate::Error;
use std::path::{Path, PathBuf};

pub struct Templates {
    pub stock_root: PathBuf,
    pub console: crate::console::ConsoleVariant,
}

impl Templates {
    pub fn new(stock_root: impl Into<PathBuf>) -> Result<Self, Error> {
        let stock_root = stock_root.into();
        let console = crate::console::ConsoleVariant::from_directory(&stock_root)?;
        Ok(Self { stock_root, console })
    }

    /// Read and unpack a stock .psb.m file to its decoded PSB bytes.
    pub fn load_stock_psb(&self, relative_path: &str) -> Result<Vec<u8>, Error> {
        let path = self.stock_root.join(relative_path);
        if !path.exists() {
            return Err(Error::Template(format!(
                "stock PSB template not found: {} (stock_root='{}', relative='{}')",
                path.display(),
                self.stock_root.display(),
                relative_path
            )));
        }
        load_psb_m(&path)
    }
}

pub fn load_psb_m(path: &Path) -> Result<Vec<u8>, Error> {
    let raw = std::fs::read(path).map_err(|e| {
        std::io::Error::new(e.kind(), format!("read {}: {}", path.display(), e))
    })?;
    let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    Ok(m2_mzs::unpack_default(&raw, &fname)?)
}

pub fn write_psb_m(path: &Path, psb: &[u8]) -> Result<(), Error> {
    let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let m = m2_mzs::pack_default(psb, &fname)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, m)?;
    Ok(())
}
