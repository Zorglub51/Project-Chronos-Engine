// Integration tests against real .psb.m / .nut.m files from the project.
// These tests confirm the Rust impl agrees with the Python mzstool.py output
// byte-for-byte on actual game data.

use std::path::PathBuf;
use std::process::Command;

fn project_root() -> PathBuf {
    // crates/m2-mzs/tests -> ../../.. -> pce-game-editor/.. -> pce/
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .parent().unwrap()
        .to_path_buf()
}

fn python_unpack(input: &PathBuf) -> Option<Vec<u8>> {
    let script = project_root().join("tools/psbtool/mzstool.py");
    if !script.exists() { return None; }
    let tmp = std::env::temp_dir().join(format!(
        "m2mzs_test_{}.bin",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let out = Command::new("python3")
        .arg(&script)
        .arg("unpack")
        .arg(input)
        .arg(&tmp)
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let bytes = std::fs::read(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    Some(bytes)
}

#[test]
fn unpack_matches_python_on_real_file() {
    let target = project_root().join("alldata_wip/system/config/font_icon_code.psb.m");
    if !target.exists() {
        eprintln!("skipping: {} not present", target.display());
        return;
    }
    let raw = std::fs::read(&target).unwrap();
    let filename = target.file_name().unwrap().to_string_lossy().to_string();

    let rust_out = m2_mzs::unpack_default(&raw, &filename)
        .expect("rust unpack failed");

    let Some(py_out) = python_unpack(&target) else {
        eprintln!("skipping byte-equality check: python3 / mzstool.py not available");
        return;
    };

    assert_eq!(rust_out.len(), py_out.len(), "size differs");
    assert_eq!(rust_out, py_out, "content differs");
}

#[test]
fn roundtrip_real_file_through_rust() {
    let target = project_root().join("alldata_wip/system/config/font_icon_code.psb.m");
    if !target.exists() {
        eprintln!("skipping: {} not present", target.display());
        return;
    }
    let raw = std::fs::read(&target).unwrap();
    let filename = target.file_name().unwrap().to_string_lossy().to_string();

    let plain = m2_mzs::unpack_default(&raw, &filename).unwrap();
    let repacked = m2_mzs::pack_default(&plain, &filename).unwrap();
    let plain2 = m2_mzs::unpack_default(&repacked, &filename).unwrap();
    assert_eq!(plain, plain2);
}
