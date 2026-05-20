use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn python_psb_to_json(psb_path: &Path) -> Option<serde_json::Value> {
    let script = project_root().join("tools/psbtool/mzstool.py");
    let tmp_dir = std::env::temp_dir().join(format!(
        "m2psb_diff_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&tmp_dir).ok()?;
    let out = Command::new("python3")
        .arg(&script).arg("psb2json").arg(psb_path).arg(&tmp_dir)
        .output().ok()?;
    if !out.status.success() { return None; }
    let stem = psb_path.file_stem()?.to_string_lossy().to_string();
    let txt = std::fs::read_to_string(tmp_dir.join(format!("{}.json", stem))).ok();
    let _ = std::fs::remove_dir_all(&tmp_dir);
    txt.and_then(|t| serde_json::from_str(&t).ok())
}

#[test]
#[ignore]
fn diff_title_prof() {
    let target = project_root().join("alldata_wip/040/config/title_prof.psb.m");
    if !target.exists() { return; }
    let raw = std::fs::read(&target).unwrap();
    let fname = target.file_name().unwrap().to_string_lossy().to_string();
    let psb_bytes = m2_mzs::unpack_default(&raw, &fname).unwrap();
    let rust_value = m2_psb::read(&psb_bytes).unwrap();
    let rust_json = rust_value.to_json();

    let tmp_psb = std::env::temp_dir().join("title_prof.psb");
    std::fs::write(&tmp_psb, &psb_bytes).unwrap();
    let py_json = python_psb_to_json(&tmp_psb).unwrap();
    let _ = std::fs::remove_file(&tmp_psb);

    // Walk both trees, find paths where they differ.
    let mut diffs = Vec::new();
    walk(&rust_json, &py_json, String::new(), &mut diffs, 30);
    println!("found {} differing leaves (limit 30):", diffs.len());
    for d in &diffs {
        println!("  {}", d);
    }
}

fn nums_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()),
        _ => false,
    }
}

fn walk(rs: &serde_json::Value, py: &serde_json::Value, path: String, out: &mut Vec<String>, limit: usize) {
    if out.len() >= limit { return; }
    if rs == py { return; }
    if rs.is_number() && py.is_number() && nums_eq(rs, py) { return; }
    match (rs, py) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            let mut keys: Vec<String> = a.keys().chain(b.keys()).cloned().collect();
            keys.sort(); keys.dedup();
            for k in keys {
                let np = if path.is_empty() { k.clone() } else { format!("{}.{}", path, k) };
                let av = a.get(&k).cloned().unwrap_or(serde_json::Value::Null);
                let bv = b.get(&k).cloned().unwrap_or(serde_json::Value::Null);
                walk(&av, &bv, np, out, limit);
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            let n = a.len().max(b.len());
            for i in 0..n {
                let av = a.get(i).cloned().unwrap_or(serde_json::Value::Null);
                let bv = b.get(i).cloned().unwrap_or(serde_json::Value::Null);
                walk(&av, &bv, format!("{}[{}]", path, i), out, limit);
            }
        }
        _ => {
            out.push(format!("{}: rust={} py={}", path, rs, py));
        }
    }
}
