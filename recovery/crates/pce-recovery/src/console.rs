//! Read-only classification of the console's root filesystem.
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    RamRecovery,
    NotRamRecovery,
    Unknown,
}

pub fn classify_mounts(mounts: &str) -> Environment {
    let roots: Vec<&str> = mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            fields.next()?;
            if fields.next()? != "/" {
                return None;
            }
            fields.next()
        })
        .collect();
    if roots.is_empty() {
        Environment::Unknown
    } else if roots
        .iter()
        .all(|fs| ["rootfs", "tmpfs", "ramfs"].contains(fs))
    {
        Environment::RamRecovery
    } else {
        Environment::NotRamRecovery
    }
}

pub fn inspect(host: &str) -> Result<Environment, String> {
    let output = crate::ssh::exec(host, "test -b /dev/mmcblk0 && cat /proc/mounts")
        .map_err(|e| e.to_string())?;
    if output.exit_status != Some(0) {
        return Err(format!(
            "Console inspection failed (exit {:?}): {}",
            output.exit_status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(classify_mounts(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ram_root_is_distinct_from_normal_boot_and_missing_data() {
        assert_eq!(
            classify_mounts("rootfs / rootfs rw 0 0\nproc /proc proc rw 0 0\n"),
            Environment::RamRecovery
        );
        assert_eq!(
            classify_mounts("/dev/root / ext4 rw 0 0\n"),
            Environment::NotRamRecovery
        );
        assert_eq!(
            classify_mounts("proc /proc proc rw 0 0\n"),
            Environment::Unknown
        );
    }
    #[test]
    fn an_underlying_rootfs_entry_does_not_hide_a_disk_root() {
        assert_eq!(
            classify_mounts("rootfs / rootfs rw 0 0\n/dev/mmcblk0p7 / ext4 rw 0 0\n"),
            Environment::NotRamRecovery
        );
    }
}
