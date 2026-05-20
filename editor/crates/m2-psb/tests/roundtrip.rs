// Round-trip every PSB in alldata_wip/ through Rust:
//   raw .psb.m → unpack (m2-mzs) → read (m2-psb) → write (m2-psb) → read (m2-psb)
// The two read trees must be value-equal. Byte-equality vs. the original is a
// stretch goal (often achievable but not guaranteed — string/stream ordering
// can differ between writers).

use std::path::{Path, PathBuf};

/// Tree equality that accepts tiny f32-quantization differences in floats.
/// Our writer (matching Python's policy) writes values as f32 when the
/// f64→f32→f64 roundtrip is within 1e-6 of the original, so re-reading
/// produces an approximately-equal but not bit-equal f64.
fn tree_eq(a: &m2_psb::Value, b: &m2_psb::Value) -> bool {
    use m2_psb::Value::*;
    match (a, b) {
        (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y || (x - y).abs() < 1e-5 * x.abs().max(1.0),
        (String(x), String(y)) => x == y,
        (Stream(x), Stream(y)) => x == y,
        (BStream(x), BStream(y)) => x == y,
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| tree_eq(a, b)),
        (Object(x), Object(y)) => x.len() == y.len() && x.iter()
            .all(|(k, v)| y.get(k).map_or(false, |yv| tree_eq(v, yv))),
        _ => false,
    }
}

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

#[test]
fn roundtrip_one_simple_file() {
    let target = project_root().join("alldata_wip/system/config/genre_code.psb.m");
    if !target.exists() { return; }
    let raw = std::fs::read(&target).unwrap();
    let fname = target.file_name().unwrap().to_string_lossy().to_string();
    let psb1 = m2_mzs::unpack_default(&raw, &fname).unwrap();
    let v1 = m2_psb::read(&psb1).unwrap();
    let psb2 = m2_psb::write(&v1, 4).unwrap();
    let v2 = m2_psb::read(&psb2).unwrap();
    assert_eq!(v1, v2, "round-trip Value tree mismatch");
}

#[test]
#[ignore]
fn roundtrip_corpus() {
    let root = project_root().join("alldata_wip");
    if !root.exists() { return; }
    let mut files = Vec::new();
    walk_psb_m(&root, &mut files);
    files.sort();
    println!("roundtripping {} PSB files", files.len());

    let mut tree_eq_count = 0;
    let mut byte_eq = 0;
    let mut tree_diff = 0;
    let mut errs = 0;
    let mut diff_samples: Vec<PathBuf> = Vec::new();

    for path in &files {
        let raw = std::fs::read(path).unwrap();
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let psb1 = match m2_mzs::unpack_default(&raw, &fname) {
            Ok(b) => b,
            Err(e) => { eprintln!("MZS ERR {}: {:?}", path.display(), e); errs += 1; continue; }
        };
        let v1 = match m2_psb::read(&psb1) {
            Ok(v) => v,
            Err(e) => { eprintln!("READ ERR {}: {:?}", path.display(), e); errs += 1; continue; }
        };
        let psb2 = match m2_psb::write(&v1, 4) {
            Ok(b) => b,
            Err(e) => { eprintln!("WRITE ERR {}: {:?}", path.display(), e); errs += 1; continue; }
        };
        let v2 = match m2_psb::read(&psb2) {
            Ok(v) => v,
            Err(e) => { eprintln!("READBACK ERR {}: {:?}", path.display(), e); errs += 1; continue; }
        };

        if tree_eq(&v1, &v2) {
            tree_eq_count += 1;
            if psb1 == psb2 { byte_eq += 1; }
        } else {
            tree_diff += 1;
            if diff_samples.len() < 5 { diff_samples.push(path.clone()); }
        }
    }

    println!("== psb roundtrip results ==");
    println!("  tree-equal: {} / {}", tree_eq_count, files.len());
    println!("    of which byte-equal to original: {}", byte_eq);
    println!("  tree-diff:  {}", tree_diff);
    println!("  errors:     {}", errs);
    for p in &diff_samples { println!("    diff sample: {}", p.display()); }

    assert_eq!(errs, 0);
    assert_eq!(tree_diff, 0);
}
