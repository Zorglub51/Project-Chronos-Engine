//! Linux owns the RNDIS data path. This module only discovers the USB netdev
//! and configures the point-to-point recovery address; it is not a bridge.
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub const HOST: &str = "169.254.13.36";
pub const PEER: &str = "169.254.13.37";

#[derive(Debug, Clone, Serialize)]
pub struct Interface {
    pub name: String,
    pub product: String,
    pub serial: String,
}

fn read(path: PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name.len() < 16
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        && name != "."
        && name != ".."
}

pub fn discover_at(root: &Path) -> io::Result<Vec<Interface>> {
    let mut interfaces = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !valid_name(&name) {
            continue;
        }
        let Ok(device) = entry.path().join("device").canonicalize() else {
            continue;
        };
        let driver = device.join("driver").canonicalize().ok();
        if driver
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            != Some("rndis_host")
        {
            continue;
        }
        for usb in device.ancestors() {
            if read(usb.join("idVendor")) == "04e8" && read(usb.join("idProduct")) == "6863" {
                interfaces.push(Interface {
                    name,
                    product: read(usb.join("product")),
                    serial: read(usb.join("serial")),
                });
                break;
            }
        }
    }
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(interfaces)
}

pub fn discover() -> io::Result<Vec<Interface>> {
    discover_at(Path::new("/sys/class/net"))
}

pub fn configure(name: &str) -> Result<(), String> {
    if !valid_name(name)
        || !discover()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|i| i.name == name)
    {
        return Err("Selected interface is not the recovery USB RNDIS device (04e8:6863).".into());
    }
    let ip = ["/usr/sbin/ip", "/usr/bin/ip", "/sbin/ip"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("Install iproute2 first.")?;
    for args in [
        vec!["link", "set", "dev", name, "up"],
        vec!["address", "replace", "169.254.13.36/32", "dev", name],
        vec![
            "route",
            "replace",
            "169.254.13.37/32",
            "dev",
            name,
            "scope",
            "link",
            "src",
            HOST,
        ],
    ] {
        let out = Command::new(ip)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interface_names_cannot_escape_sysfs_or_inject_arguments() {
        for name in [
            "",
            ".",
            "..",
            "../eth0",
            "--help",
            "eth0;id",
            "eth0 foo",
            "1234567890123456",
        ] {
            assert!(!valid_name(name), "{name}");
        }
        assert!(valid_name("enx123456789abc"));
    }
    #[cfg(unix)]
    #[test]
    fn discovers_only_matching_usb_identity_and_rndis_driver() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let net = tmp.path().join("net");
        fs::create_dir(&net).unwrap();
        let usb = tmp.path().join("usb1/1-2");
        let iface = usb.join("1-2:1.0");
        fs::create_dir_all(&iface).unwrap();
        let driver = tmp.path().join("rndis_host");
        fs::create_dir(&driver).unwrap();
        symlink(&driver, iface.join("driver")).unwrap();
        fs::create_dir(net.join("usb0")).unwrap();
        symlink(&iface, net.join("usb0/device")).unwrap();
        fs::write(usb.join("idVendor"), "04e8\n").unwrap();
        fs::write(usb.join("idProduct"), "6863\n").unwrap();
        assert_eq!(discover_at(&net).unwrap()[0].name, "usb0");
        fs::write(usb.join("idProduct"), "0001").unwrap();
        assert!(discover_at(&net).unwrap().is_empty());
    }
}
