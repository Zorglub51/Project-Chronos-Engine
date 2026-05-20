// Walk every .psb.m in alldata_wip/, decode with both Rust (m2-mzs+m2-psb)
// and Python (mzstool.py psb2json), compare the resulting JSON objects.

use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn walk_psb_m(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_psb_m(&p, out);
        } else {
            let n = p.to_string_lossy();
            if n.ends_with(".psb.m") {
                out.push(p);
            }
        }
    }
}

/// Strip whitespace differences and trailing newline so string comparison
/// catches semantic JSON differences but not formatter quirks.
fn normalize(s: &str) -> String {
    s.lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn python_psb_to_json_text(psb_path: &Path) -> Option<String> {
    let script = project_root().join("tools/psbtool/mzstool.py");
    let tmp_dir = std::env::temp_dir().join(format!(
        "m2psb_corp_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&tmp_dir).ok()?;
    let out = Command::new("python3")
        .arg(&script).arg("psb2json").arg(psb_path).arg(&tmp_dir)
        .output().ok()?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }
    let stem = psb_path.file_stem()?.to_string_lossy().to_string();
    let json_path = tmp_dir.join(format!("{}.json", stem));
    let txt = std::fs::read_to_string(&json_path).ok();
    let _ = std::fs::remove_dir_all(&tmp_dir);
    txt
}

#[test]
#[ignore] // run with: cargo test --release -p m2-psb -- --ignored corpus_full --nocapture
fn corpus_full() {
    let root = project_root().join("alldata_wip");
    if !root.exists() { return; }

    let mut files = Vec::new();
    walk_psb_m(&root, &mut files);
    files.sort();
    println!("checking {} PSB files", files.len());

    let mut ok = 0usize;
    let mut json_diff = 0usize;
    let mut rust_err = 0usize;
    let mut py_skip = 0usize;
    let mut diffs = Vec::new();

    for path in &files {
        let raw = std::fs::read(path).unwrap();
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let psb_bytes = match m2_mzs::unpack_default(&raw, &fname) {
            Ok(b) => b,
            Err(e) => { eprintln!("MZS ERR: {} -> {:?}", path.display(), e); rust_err += 1; continue; }
        };
        let rust_value = match m2_psb::read(&psb_bytes) {
            Ok(v) => v,
            Err(e) => { eprintln!("PSB ERR: {} -> {:?}", path.display(), e); rust_err += 1; continue; }
        };
        let rust_text = serde_json::to_string_pretty(&rust_value.to_json()).unwrap();

        let tmp_psb = std::env::temp_dir().join(format!(
            "m2psb_in_{}.psb",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(&tmp_psb, &psb_bytes).unwrap();
        let py_text = python_psb_to_json_text(&tmp_psb);
        let _ = std::fs::remove_file(&tmp_psb);

        let Some(py_text) = py_text else { py_skip += 1; continue };

        if normalize(&rust_text) != normalize(&py_text) {
            json_diff += 1;
            if diffs.len() < 5 {
                diffs.push(path.clone());
            }
        } else {
            ok += 1;
        }
    }

    println!("== psb corpus results ==");
    println!("  ok:         {}", ok);
    println!("  json diff:  {}", json_diff);
    println!("  rust error: {}", rust_err);
    println!("  py skipped: {}", py_skip);
    if !diffs.is_empty() {
        println!("  first diff samples:");
        for p in &diffs { println!("    {}", p.display()); }
    }

    assert_eq!(rust_err, 0);
    assert_eq!(json_diff, 0);
}
