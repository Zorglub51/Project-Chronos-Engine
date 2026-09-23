//! Retail package identity is independent of the selected JP/US game lineup.
use crate::Error;
use std::path::Path;

pub const SYSTEM_PROFILE: &str = "system/config/system_prof.psb.m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleVariant {
    Japan,
    World,
}

impl ConsoleVariant {
    pub fn id(self) -> &'static str {
        match self { Self::Japan => "040", Self::World => "041" }
    }

    /// Profile, menu, JP-lineup covers, US-lineup covers (in that order).
    pub fn templates(self) -> [&'static str; 4] {
        match self {
            Self::Japan => [
                "040/config/title_prof.psb.m", "040/config/title_mode_top.psb.m",
                "040/motion/title_jp_titleselect_jp.psb.m",
                "040/motion/title_jp_titleselect_us.psb.m",
            ],
            Self::World => [
                "041/config/title_prof.psb.m", "041/config/title_mode_top.psb.m",
                "041/motion/title_us_titleselect_jp.psb.m",
                "041/motion/title_us_titleselect_us.psb.m",
            ],
        }
    }

    /// Prefer the package dev_id, not region (both retail dumps say "japan")
    /// or title_list (both contain 40 AND 41). Legacy templates can omit it,
    /// but must contain exactly one complete resource set.
    pub fn detect(profile: Option<&[u8]>, exists: impl Fn(&str) -> bool) -> Result<Self, Error> {
        let variant = if let Some(profile) = profile {
            let decoded;
            let bytes = if profile.starts_with(b"mzs\0") {
                decoded = m2_mzs::unpack_default(profile, "system_prof.psb.m")?;
                &decoded[..]
            } else { profile };
            let json = m2_psb::read(bytes)?.to_json();
            match json.pointer("/root/dev_id").and_then(serde_json::Value::as_str) {
                Some("40") => Self::Japan,
                Some("41") => Self::World,
                _ => return Err(Error::Template("Unsupported console package in system_prof.psb.m (expected 40 or 41)".into())),
            }
        } else {
            match (Self::Japan.templates().iter().all(|p| exists(p)),
                   Self::World.templates().iter().all(|p| exists(p))) {
                (true, false) => Self::Japan,
                (false, true) => Self::World,
                (true, true) => return Err(Error::Template("Both console resource sets are present. Include the original system/config/system_prof.psb.m to identify the console.".into())),
                (false, false) => return Err(Error::Template("No complete console resource set found: expected 040 with title_jp_titleselect_{jp,us}.psb.m, or 041 with title_us_titleselect_{jp,us}.psb.m.".into())),
            }
        };
        for path in variant.templates() {
            if !exists(path) {
                return Err(Error::Template(format!("Console {} resource missing: {path}", variant.id())));
            }
        }
        Ok(variant)
    }

    pub fn from_directory(root: &Path) -> Result<Self, Error> {
        let path = root.join(SYSTEM_PROFILE);
        let profile = if path.is_file() { Some(std::fs::read(path)?) } else { None };
        Self::detect(profile.as_deref(), |p| root.join(p).is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(id: &str) -> Vec<u8> {
        let value = m2_psb::Value::Object(indexmap::IndexMap::from([("root".into(),
            m2_psb::Value::Object(indexmap::IndexMap::from([
                ("dev_id".into(), m2_psb::Value::String(id.into())),
                ("region".into(), m2_psb::Value::String("japan".into())),
            ])))]));
        let bytes = m2_psb::write(&value, 4).unwrap();
        m2_mzs::pack_default(&bytes, "system_prof.psb.m").unwrap()
    }
    #[test]
    fn detects_both_retail_and_legacy_layouts() {
        for (id, variant) in [("40", ConsoleVariant::Japan), ("41", ConsoleVariant::World)] {
            let has = |p: &str| variant.templates().contains(&p);
            assert_eq!(ConsoleVariant::detect(Some(&profile(id)), has).unwrap(), variant);
            assert_eq!(ConsoleVariant::detect(None, has).unwrap(), variant);
            // Identity wins even if both directories are present.
            assert_eq!(ConsoleVariant::detect(Some(&profile(id)), |_| true).unwrap(), variant);
            // The actual encrypted basename must be preserved.
            for path in &variant.templates()[2..] {
                let name = Path::new(path).file_name().unwrap().to_str().unwrap();
                let encrypted = m2_mzs::pack_default(b"PSB test", name).unwrap();
                assert_eq!(m2_mzs::unpack_default(&encrypted, name).unwrap(), b"PSB test");
            }
        }
    }
    #[test]
    fn rejects_ambiguous_incomplete_or_mismatched_resources() {
        assert!(ConsoleVariant::detect(None, |_| true).is_err());
        assert!(ConsoleVariant::detect(None, |_| false).is_err());
        assert!(ConsoleVariant::detect(Some(&profile("41")), |p| ConsoleVariant::Japan.templates().contains(&p)).is_err());
        assert!(ConsoleVariant::detect(Some(&profile("42")), |_| true).is_err());
        assert!(ConsoleVariant::detect(Some(b"broken"), |_| true).is_err());
    }
}
