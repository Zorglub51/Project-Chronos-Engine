// CLI: m2-publish <library_root> <stock_data_root> <output_root>
//
// Walks <library_root> (a PCE Mini source library: jp/, us/, with games and
// optional _folders/ subdirs) and produces <output_root>/m2engage/ with
// packed ROMs and per-folder PSBs.
//
// <stock_data_root> points to an unpacked alldata directory (alldata_wip/) —
// we read PSB templates from there to generate folder-specific variants.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: m2-publish <library_root> <stock_data_root> <output_root>");
        return ExitCode::from(2);
    }
    let opts = m2_publish::PublishOptions {
        library_root: PathBuf::from(&args[1]),
        stock_data_root: PathBuf::from(&args[2]),
        output_root: PathBuf::from(&args[3]),
        incremental: false,
    };

    match m2_publish::publish(&opts) {
        Ok(report) => {
            println!("publish ok:");
            println!("  ROMs packed (mzs): {}", report.roms_packed);
            println!("  ROMs copied:        {}", report.roms_copied);
            println!("  PSB files written:  {}", report.psb_files_written);
            println!("  Folders emitted:    {}", report.folders_emitted);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("publish failed: {}", e);
            ExitCode::from(1)
        }
    }
}
