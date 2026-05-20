// Diagnostic: are Rust and Python reading identical f64 bits for the
// "preamp" values that show JSON formatting differences?

use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

#[test]
#[ignore]
fn dump_preamp_bits() {
    let target = project_root().join("alldata_wip/040/config/title_prof.psb.m");
    if !target.exists() { return; }
    let raw = std::fs::read(&target).unwrap();
    let fname = target.file_name().unwrap().to_string_lossy().to_string();
    let psb = m2_mzs::unpack_default(&raw, &fname).unwrap();
    let v = m2_psb::read(&psb).unwrap();

    let m2epi = match &v {
        m2_psb::Value::Object(o) => o.get("m2epi").unwrap(),
        _ => panic!(),
    };
    let version = match m2epi {
        m2_psb::Value::Object(o) => o.get("version").unwrap(),
        _ => panic!(),
    };
    let games = match version {
        m2_psb::Value::Object(o) => o,
        _ => panic!(),
    };

    for tag in ["GAME011", "GAME022", "GAME024", "GAME027", "GAME048"] {
        let game = games.get(tag).unwrap();
        let preamp = match game {
            m2_psb::Value::Object(o) => o.get("preamp").unwrap(),
            _ => panic!(),
        };
        let f = match preamp {
            m2_psb::Value::Float(f) => *f,
            other => panic!("not float: {:?}", other),
        };
        println!(
            "{}.preamp: f64={} bits=0x{:016x}",
            tag, f, f.to_bits()
        );
    }
}
