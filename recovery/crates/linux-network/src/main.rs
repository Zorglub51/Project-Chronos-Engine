fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "configure" {
        eprintln!("Usage: pce-recovery-network configure <recovery USB interface>");
        std::process::exit(2);
    }
    if let Err(e) = linux_network::configure(&args[1]) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
