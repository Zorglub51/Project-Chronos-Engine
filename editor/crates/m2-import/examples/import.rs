//! Local smoke harness using exactly the desktop import pipeline.
use anyhow::{bail, Context, Result};
use std::path::Path;
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let progress =
        |p: m2_import::Progress| eprintln!("{} {:?}/{:?}", p.message, p.completed, p.total);
    match args.get(1).map(String::as_str) {
        Some("library") if args.len()==6 => {
            let result=m2_import::create_library(Path::new(&args[2]),Path::new(&args[3]),args[4]=="stock",Path::new(&args[5]),&progress)?;
            println!("{}",serde_json::to_string_pretty(&result)?);
        },
        Some("rom") if args.len()==6 => {
            let bios=m2_import::configure_bios(Some(Path::new(&args[4])),None,None,Path::new(&args[5])).context("BIOS")?;
            let result=m2_import::import_rom(Path::new(&args[2]),Path::new(&args[3]),Some(&bios),&progress)?;
            println!("{}",serde_json::to_string_pretty(&result)?);
        },
        _ => bail!("Usage: import library SOURCE DEST stock|empty BIOS_CACHE\n       import rom SOURCE DEST STOCK_PCD BIOS_CACHE"),
    }
    Ok(())
}
