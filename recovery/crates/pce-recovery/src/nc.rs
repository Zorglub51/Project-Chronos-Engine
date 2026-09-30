//! Block-level dump/restore over a plain TCP stream — no TFTP, no SCP.
//!
//! On the device we run busybox `nc -l -p <port>` redirecting from/to the
//! raw block device; on the host we open a `TcpStream`. Roughly 12 MB/s
//! over USB high-speed RNDIS — bound by the link, not by CPU crypto. End
//! integrity is checked by streaming SHA-1 on the host side and comparing
//! against `busybox sha1sum <device_path>` after the transfer completes.
//!
//! Why SHA-1 and not SHA-256: ~3× faster on Cortex-A7 (no ARMv8 crypto
//! extensions) and the link is private to a USB cable — we want a checksum,
//! not a security guarantee.
//!
//! Listener lifecycle: an `nc -l` invocation exits as soon as its peer
//! closes. We spawn it via `ssh::exec` on a worker thread; the call returns
//! when the data transfer (and the device-side `nc`) finish.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

use sha1::{Digest, Sha1};

use crate::ssh;

/// Port used for the data channel. Single-flight: GUI/CLI serialise dump
/// and restore operations, so reusing one port keeps things obvious. If we
/// ever want parallelism, allocate ports per call.
pub const DATA_PORT: u16 = 5001;

/// Time we'll wait for the device-side `nc -l` to start listening before
/// giving up. Sub-second under normal conditions.
const LISTENER_READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Read buffer size. 64 KiB is large enough to amortise syscall overhead
/// without bloating per-call memory.
const IO_BUF: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum NcError {
    #[error("ssh: {0}")]
    Ssh(#[from] ssh::SshError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("listener never came up on {host}:{port}")]
    ListenerTimeout { host: String, port: u16 },
    #[error("device sha1sum returned no parsable hash: {0:?}")]
    BadDeviceHash(String),
    #[error("hash mismatch: host={host}, device={device}")]
    HashMismatch { host: String, device: String },
    #[error("size mismatch: expected {expected}, got {got}")]
    SizeMismatch { expected: u64, got: u64 },
    #[error("partition preflight: {0}")]
    Preflight(String),
    #[error("remote command failed: {0}")]
    Remote(String),
    #[error("listener thread panicked")]
    ListenerPanic,
}

pub type Result<T> = std::result::Result<T, NcError>;

fn data_addr() -> SocketAddrV4 {
    SocketAddrV4::new(
        Ipv4Addr::new(
            crate::PEER_IP[0],
            crate::PEER_IP[1],
            crate::PEER_IP[2],
            crate::PEER_IP[3],
        ),
        DATA_PORT,
    )
}

fn host_str() -> String {
    let ip = crate::PEER_IP;
    format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}

/// Pipe `device_path` over TCP into `out`. Calls `on_progress(transferred)`
/// after every chunk. After the stream closes, fetches `sha1sum
/// device_path` from the device and verifies against the SHA-1 of the
/// bytes written. Returns the verified hash.
///
/// `expected_size` is checked against the actual byte count and against
/// the file we wrote — pass `0` to skip the check.
pub fn dump_partition<W: Write>(
    device_path: &str,
    expected_size: u64,
    out: &mut W,
    mut on_progress: impl FnMut(u64),
) -> Result<[u8; 20]> {
    let host = host_str();
    preflight(&host, device_path, expected_size, false)?;

    // Best-effort cleanup of any stale listener from a previous aborted run.
    // Ignore errors: a missing nc just means there was nothing to kill.
    let _ = ssh::exec(&host, "killall nc 2>/dev/null; true");

    // Spawn the listener. ssh::exec is blocking and waits for the remote
    // command to exit — exactly what we want here, since `nc -l` exits the
    // moment we close our TCP side. The thread joins after the transfer.
    let cmd = format!("busybox nc -l -p {} < {}", DATA_PORT, device_path);
    let listener_host = host.clone();
    let cmd_clone = cmd.clone();
    let listener = std::thread::spawn(move || {
        tracing::debug!(cmd = %cmd_clone, "spawning device listener");
        let r = ssh::exec(&listener_host, &cmd_clone);
        tracing::debug!(
            ok = r.is_ok(),
            stderr = %r.as_ref().map(|o| String::from_utf8_lossy(&o.stderr).into_owned()).unwrap_or_default(),
            "device listener finished",
        );
        r
    });

    let mut stream = wait_for_listener(&host, DATA_PORT)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;

    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; IO_BUF];
    let mut transferred: u64 = 0;
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        transferred += n as u64;
        on_progress(transferred);
    }
    drop(stream); // ensure the device's nc sees EOF and exits.

    ensure_success(listener.join().map_err(|_| NcError::ListenerPanic)??)?;

    if expected_size != 0 && transferred != expected_size {
        return Err(NcError::SizeMismatch {
            expected: expected_size,
            got: transferred,
        });
    }

    let host_hash: [u8; 20] = hasher.finalize().into();
    let device_hash = remote_sha1(&host, device_path, expected_size)?;
    if host_hash != device_hash {
        return Err(NcError::HashMismatch {
            host: hex(&host_hash),
            device: hex(&device_hash),
        });
    }
    Ok(host_hash)
}

/// Stream `data` into `device_path` over TCP. After the transfer, runs
/// `sha1sum device_path` on the device and compares against the SHA-1 of
/// the bytes we sent.
pub fn restore_partition(
    device_path: &str,
    data: &[u8],
    mut on_progress: impl FnMut(u64),
) -> Result<[u8; 20]> {
    let host = host_str();
    preflight(&host, device_path, data.len() as u64, true)?;

    let _ = ssh::exec(&host, "killall nc 2>/dev/null; true");

    // Device redirects nc's stdout into the block device. nc reads until
    // we close the TCP side, then exits; the kernel caps writes at the
    // partition size. We don't pipe through `dd` to bound the write —
    // dd would need a per-call bs/count, and bs={data.len()} count=1
    // makes dd try to allocate the whole partition in RAM (256 MB on
    // device, partitions up to 2.2 GiB → OOM). Host already validates
    // file size == partition size before calling this.
    let cmd = format!("busybox nc -l -p {} > {}", DATA_PORT, device_path,);
    let listener_host = host.clone();
    let listener = std::thread::spawn(move || ssh::exec(&listener_host, &cmd));

    let mut stream = wait_for_listener(&host, DATA_PORT)?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    let mut hasher = Sha1::new();
    let mut sent: u64 = 0;
    for chunk in data.chunks(IO_BUF) {
        stream.write_all(chunk)?;
        hasher.update(chunk);
        sent += chunk.len() as u64;
        on_progress(sent);
    }
    stream.shutdown(std::net::Shutdown::Write)?;
    drop(stream);

    ensure_success(listener.join().map_err(|_| NcError::ListenerPanic)??)?;

    ensure_success(ssh::exec(&host, "sync")?)?;
    let host_hash: [u8; 20] = hasher.finalize().into();
    let device_hash = remote_sha1(&host, device_path, data.len() as u64)?;
    if host_hash != device_hash {
        return Err(NcError::HashMismatch {
            host: hex(&host_hash),
            device: hex(&device_hash),
        });
    }
    Ok(host_hash)
}

/// Try to connect every 50 ms until the listener accepts or we hit the
/// timeout. The first attempt typically succeeds within ~150 ms after
/// `ssh::exec` is called (SSH session setup + nc start).
fn wait_for_listener(host: &str, port: u16) -> Result<TcpStream> {
    let addr = data_addr();
    let deadline = Instant::now() + LISTENER_READY_TIMEOUT;
    let mut last_err: Option<std::io::Error> = None;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&SocketAddr::V4(addr), Duration::from_millis(500)) {
            Ok(s) => return Ok(s),
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    let _ = last_err;
    Err(NcError::ListenerTimeout {
        host: host.into(),
        port,
    })
}

/// `busybox sha1sum <path>` on the device, parse the hex out of stdout.
/// `expected_size` is informational — we don't read it back, but it lets a
/// future caller add a "device file size matches expected" sanity check.
fn remote_sha1(host: &str, device_path: &str, _expected_size: u64) -> Result<[u8; 20]> {
    let out = ssh::exec(host, &format!("busybox sha1sum {}", device_path))?;
    ensure_success_ref(&out)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let token = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| NcError::BadDeviceHash(stdout.to_string()))?;
    if token.len() != 40 {
        return Err(NcError::BadDeviceHash(stdout.to_string()));
    }
    let mut hash = [0u8; 20];
    for (i, b) in hash.iter_mut().enumerate() {
        *b = u8::from_str_radix(&token[i * 2..i * 2 + 2], 16)
            .map_err(|_| NcError::BadDeviceHash(stdout.to_string()))?;
    }
    Ok(hash)
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push_str(&format!("{:02x}", x));
    }
    s
}

fn ensure_success_ref(out: &ssh::ExecOutput) -> Result<()> {
    if out.exit_status != Some(0) {
        return Err(NcError::Remote(format!(
            "exit {:?}: {}",
            out.exit_status,
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}
fn ensure_success(out: ssh::ExecOutput) -> Result<()> {
    ensure_success_ref(&out)
}

fn preflight(host: &str, path: &str, expected: u64, writing: bool) -> Result<()> {
    let partition = crate::PARTITIONS
        .iter()
        .chain(std::iter::once(&crate::FULL))
        .find(|p| p.device_path == path)
        .ok_or_else(|| NcError::Preflight("Unknown target device".into()))?;
    if expected != partition.size_bytes() {
        return Err(NcError::Preflight(
            "Image size does not match the selected partition".into(),
        ));
    }
    let name = path.strip_prefix("/dev/").unwrap();
    let out = ssh::exec(
        host,
        &format!("test -b {path} && cat /sys/class/block/{name}/size && cat /proc/mounts"),
    )?;
    ensure_success_ref(&out)?;
    validate_target(
        &String::from_utf8_lossy(&out.stdout),
        path,
        expected,
        writing,
    )
}

fn validate_target(output: &str, path: &str, expected: u64, writing: bool) -> Result<()> {
    let mut lines = output.lines();
    let sectors = lines.next().and_then(|s| s.trim().parse::<u64>().ok());
    if sectors.and_then(|n| n.checked_mul(512)) != Some(expected) {
        return Err(NcError::Preflight(
            "Console partition size differs from the expected layout".into(),
        ));
    }
    if writing {
        let mounts: Vec<Vec<&str>> = lines.map(|s| s.split_whitespace().collect()).collect();
        if mounts.iter().any(|m| {
            m.len() >= 3
                && (m[0] == path || (path == "/dev/mmcblk0" && m[0].starts_with("/dev/mmcblk0")))
        }) {
            return Err(NcError::Preflight(
                "Target is mounted. Boot the console into recovery before restoring.".into(),
            ));
        }
        // Share the same mode check used by the connection diagnostics.
        if crate::console::classify_mounts(
            output
                .split_once('\n')
                .map(|(_, mounts)| mounts)
                .unwrap_or(""),
        ) != crate::console::Environment::RamRecovery
        {
            return Err(NcError::Preflight(
                "Console is not running from a RAM recovery filesystem.".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_layout_and_mounted_targets_are_rejected_before_writes() {
        assert!(
            validate_target("2\nrootfs / rootfs rw 0 0\n", "/dev/mmcblk0p7", 1024, true).is_ok()
        );
        assert!(
            validate_target("3\nrootfs / rootfs rw 0 0\n", "/dev/mmcblk0p7", 1024, true).is_err()
        );
        assert!(validate_target(
            "2\nrootfs / rootfs rw 0 0\n/dev/mmcblk0p7 /linux ext4 rw 0 0\n",
            "/dev/mmcblk0p7",
            1024,
            true
        )
        .is_err());
        assert!(
            validate_target("2\n/dev/root / ext4 rw 0 0\n", "/dev/mmcblk0p7", 1024, true).is_err()
        );
        assert!(validate_target(
            "2\nrootfs / rootfs rw 0 0\n/dev/mmcblk0p9 /game ext4 rw 0 0\n",
            "/dev/mmcblk0",
            1024,
            true
        )
        .is_err());
    }
}
