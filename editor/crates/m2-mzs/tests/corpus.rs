// Walk all .psb.m / .nut.m / .pce.m / .pcd.m files in alldata_wip/ and
// confirm Rust unpack matches Python mzstool.py byte-for-byte. Skipped
// silently if the corpus or python3 isn't available.

use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn walk_m_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_m_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("m") {
            out.push(p);
        }
    }
}

fn python_unpack(input: &Path) -> Option<Vec<u8>> {
    let script = project_root().join("tools/psbtool/mzstool.py");
    let tmp = std::env::temp_dir().join(format!(
        "m2mzs_corpus_{}.bin",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let out = Command::new("python3")
        .arg(&script).arg("unpack").arg(input).arg(&tmp)
        .output().ok()?;
    if !out.status.success() { return None; }
    let bytes = std::fs::read(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    Some(bytes)
}

#[test]
#[ignore] // run with `cargo test --release -p m2-mzs -- --ignored corpus_full`
fn corpus_full() {
    let root = project_root().join("alldata_wip");
    if !root.exists() {
        eprintln!("skipping: {} not present", root.display());
        return;
    }

    let mut files = Vec::new();
    walk_m_files(&root, &mut files);
    files.sort();
    println!("checking {} files", files.len());

    let mut ok = 0usize;
    let mut size_mismatch = 0usize;
    let mut content_mismatch = 0usize;
    let mut rust_err = 0usize;
    let mut python_skip = 0usize;

    for path in &files {
        let raw = std::fs::read(path).unwrap();
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let rust_out = match m2_mzs::unpack_default(&raw, &fname) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("RUST ERR: {} -> {:?}", path.display(), e);
                rust_err += 1;
                continue;
            }
        };
        let Some(py_out) = python_unpack(path) else {
            python_skip += 1;
            continue;
        };
        if rust_out.len() != py_out.len() {
            eprintln!("SIZE MISMATCH: {} (rust={} py={})", path.display(), rust_out.len(), py_out.len());
            size_mismatch += 1;
        } else if rust_out != py_out {
            eprintln!("CONTENT MISMATCH: {}", path.display());
            content_mismatch += 1;
        } else {
            ok += 1;
        }
    }

    println!("== corpus results ==");
    println!("  ok:               {}", ok);
    println!("  size mismatch:    {}", size_mismatch);
    println!("  content mismatch: {}", content_mismatch);
    println!("  rust error:       {}", rust_err);
    println!("  python skipped:   {}", python_skip);

    assert_eq!(rust_err, 0);
    assert_eq!(size_mismatch, 0);
    assert_eq!(content_mismatch, 0);
}
