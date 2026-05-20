// Compare Rust PSB reader output against Python mzstool.py psb2json output.
// JSON-level comparison: structure + values must match. Stream binary content
// is not compared here (mzstool.py extracts streams to separate files).

use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

/// Run mzstool.py to convert a .psb file to JSON. Returns the JSON text.
fn python_psb_to_json(input: &Path) -> Option<serde_json::Value> {
    let script = project_root().join("tools/psbtool/mzstool.py");
    if !script.exists() {
        return None;
    }
    let tmp_dir = std::env::temp_dir().join(format!(
        "m2psb_test_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&tmp_dir).ok()?;

    let out = Command::new("python3")
        .arg(&script)
        .arg("psb2json")
        .arg(input)
        .arg(&tmp_dir)
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!("python3 psb2json failed: {}", String::from_utf8_lossy(&out.stderr));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }

    let stem = input.file_stem()?.to_string_lossy().to_string();
    // mzstool puts JSON at <out_dir>/<stem>.json
    let json_path = tmp_dir.join(format!("{}.json", stem));
    let txt = std::fs::read_to_string(&json_path).ok()?;
    let _ = std::fs::remove_dir_all(&tmp_dir);
    serde_json::from_str(&txt).ok()
}

fn unpack_to_psb(psb_m_path: &Path) -> Vec<u8> {
    let raw = std::fs::read(psb_m_path).unwrap();
    let fname = psb_m_path.file_name().unwrap().to_string_lossy().to_string();
    m2_mzs::unpack_default(&raw, &fname).unwrap()
}

#[test]
fn psb_read_matches_python_on_one_file() {
    // Pick a small, simple PSB that's likely to round-trip cleanly.
    let target = project_root().join("alldata_wip/system/config/genre_code.psb.m");
    if !target.exists() {
        eprintln!("skipping: {} not present", target.display());
        return;
    }

    let psb_bytes = unpack_to_psb(&target);
    let rust_value = m2_psb::read(&psb_bytes).expect("rust read failed");
    let rust_json = rust_value.to_json();

    // Hand the unpacked .psb to Python via psb2json.
    let tmp_psb = std::env::temp_dir().join("m2psb_genre_code.psb");
    std::fs::write(&tmp_psb, &psb_bytes).unwrap();
    let py_json = match python_psb_to_json(&tmp_psb) {
        Some(j) => j,
        None => {
            eprintln!("skipping JSON parity: python tool not available");
            let _ = std::fs::remove_file(&tmp_psb);
            return;
        }
    };
    let _ = std::fs::remove_file(&tmp_psb);

    if rust_json != py_json {
        // Show a brief diff
        let rs = serde_json::to_string_pretty(&rust_json).unwrap();
        let py = serde_json::to_string_pretty(&py_json).unwrap();
        let r_lines: Vec<&str> = rs.lines().take(40).collect();
        let p_lines: Vec<&str> = py.lines().take(40).collect();
        eprintln!("--- RUST (first 40 lines) ---\n{}", r_lines.join("\n"));
        eprintln!("--- PYTHON (first 40 lines) ---\n{}", p_lines.join("\n"));
        panic!("JSON output differs");
    }
}
