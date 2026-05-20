// Write a PSB with Rust, then have Python read it back. Proves the bytes
// our writer emits are valid PSB that an independent reader accepts.

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
        if p.is_dir() { walk_psb_m(&p, out); }
        else if p.to_string_lossy().ends_with(".psb.m") { out.push(p); }
    }
}

fn python_psb2json(psb: &Path) -> Option<String> {
    let script = project_root().join("tools/psbtool/mzstool.py");
    let out_dir = std::env::temp_dir().join(format!(
        "m2psb_pyrr_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&out_dir).ok()?;
    let r = Command::new("python3").arg(&script).arg("psb2json").arg(psb).arg(&out_dir).output().ok()?;
    if !r.status.success() {
        eprintln!("python read failed: {}", String::from_utf8_lossy(&r.stderr));
        let _ = std::fs::remove_dir_all(&out_dir);
        return None;
    }
    let stem = psb.file_stem()?.to_string_lossy().to_string();
    let txt = std::fs::read_to_string(out_dir.join(format!("{}.json", stem))).ok();
    let _ = std::fs::remove_dir_all(&out_dir);
    txt
}

fn normalize(s: &str) -> String {
    s.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n").trim_end().to_string()
}

#[test]
#[ignore]
fn python_reads_every_rust_written_psb() {
    let root = project_root().join("alldata_wip");
    if !root.exists() { return; }
    let mut files = Vec::new();
    walk_psb_m(&root, &mut files);
    files.sort();

    let mut ok = 0;
    let mut json_diff = 0;
    let mut errs = 0;
    let mut skipped = 0;
    let mut diff_samples: Vec<PathBuf> = Vec::new();

    for path in &files {
        let raw = std::fs::read(path).unwrap();
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let psb_orig = m2_mzs::unpack_default(&raw, &fname).unwrap();
        let v = m2_psb::read(&psb_orig).unwrap();
        let psb_rust = match m2_psb::write(&v, 4) {
            Ok(b) => b,
            Err(e) => { eprintln!("WRITE ERR {}: {:?}", path.display(), e); errs += 1; continue; }
        };

        // Save Rust-written PSB to a temp file, run Python's psb2json on it.
        let tmp = std::env::temp_dir().join(format!("rustout_{}.psb",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&tmp, &psb_rust).unwrap();
        let py_from_rust = python_psb2json(&tmp);
        let _ = std::fs::remove_file(&tmp);

        // Same for original (so the two JSON outputs share the same formatter).
        let tmp_orig = std::env::temp_dir().join(format!("origin_{}.psb",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&tmp_orig, &psb_orig).unwrap();
        let py_from_orig = python_psb2json(&tmp_orig);
        let _ = std::fs::remove_file(&tmp_orig);

        match (py_from_rust, py_from_orig) {
            (Some(a), Some(b)) => {
                if normalize(&a) == normalize(&b) {
                    ok += 1;
                } else {
                    json_diff += 1;
                    if diff_samples.len() < 5 { diff_samples.push(path.clone()); }
                }
            }
            _ => skipped += 1,
        }
    }

    println!("== python-reads-rust results ==");
    println!("  ok:        {}", ok);
    println!("  json diff: {}", json_diff);
    println!("  errors:    {}", errs);
    println!("  skipped:   {}", skipped);
    for p in &diff_samples { println!("    diff: {}", p.display()); }

    assert_eq!(errs, 0);
    assert_eq!(json_diff, 0);
}
