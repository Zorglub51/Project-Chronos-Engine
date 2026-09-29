use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

/// IP the recovery initrd brings up. Hardcoded in `nand_dump/Program.cs:15`
/// and the Linux USB network configuration.
pub const PEER_IP: &str = "169.254.13.37";

/// Port we probe to confirm the recovery image is actually up. dropbear in
/// the Chronos boot.img.tftp initrd listens on :22; the trampoline-based
/// initrd does too. nothing on the host or in the kernel will answer for
/// us, so a successful TCP connect proves the device is reachable.
const PROBE_PORT: u16 = 22;

/// Spawn an always-on probe thread that polls `PEER_IP:22` every `interval`
/// for the lifetime of the app. Emits `net-probe` events so the header LED
/// reflects current reachability.
///
/// TCP establishes real reachability without relying on ICMP responses.
pub fn spawn_persistent(app: AppHandle, interval: Duration) {
    std::thread::spawn(move || loop {
        let ok = tcp_reachable(PEER_IP, PROBE_PORT, Duration::from_millis(500));
        let payload = if ok {
            serde_json::json!({ "state": "up", "target": PEER_IP })
        } else {
            serde_json::json!({ "state": "down", "target": PEER_IP })
        };
        let _ = app.emit("net-probe", payload);
        std::thread::sleep(interval);
    });
}

fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let addr: SocketAddr = match format!("{host}:{port}").parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}
