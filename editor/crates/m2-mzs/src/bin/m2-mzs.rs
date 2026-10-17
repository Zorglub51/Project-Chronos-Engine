//! Small reproducible pack/unpack utility for console mod resources.
use std::{fs, path::Path};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 || !matches!(args[1].as_str(), "pack" | "unpack") {
        return Err("Usage: m2-mzs pack|unpack INPUT OUTPUT".into());
    }
    let input = Path::new(&args[2]);
    let output = Path::new(&args[3]);
    let bytes = fs::read(input)?;
    let transformed = if args[1] == "pack" {
        let name = output.file_name().and_then(|s| s.to_str()).ok_or("Invalid output basename")?;
        let packed = m2_mzs::pack_default(&bytes, name)?;
        if m2_mzs::unpack_default(&packed, name)? != bytes {
            return Err("MZS verification failed".into());
        }
        packed
    } else {
        let name = input.file_name().and_then(|s| s.to_str()).ok_or("Invalid input basename")?;
        m2_mzs::unpack_default(&bytes, name)?
    };
    fs::write(output, transformed)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
