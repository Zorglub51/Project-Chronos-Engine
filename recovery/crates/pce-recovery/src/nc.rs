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

use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
/// `expected_size` must match the selected partition's factory layout and
/// its current device size. RAM recovery and unmounted targets are required.
pub fn dump_partition<W: Write>(
    device_path: &str,
    expected_size: u64,
    out: &mut W,
    mut on_progress: impl FnMut(u64),
) -> Result<[u8; 20]> {
    let host = host_str();
    preflight(&host, device_path, expected_size, false)?;

    let mut listener = RemoteListener::start(&host, device_path, false)?;

    let mut stream = listener.connect()?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;

    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; IO_BUF];
    let mut transferred: u64 = 0;
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let actual = transferred + n as u64;
        if actual > expected_size {
            return Err(NcError::SizeMismatch {
                expected: expected_size,
                got: actual,
            });
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        transferred += n as u64;
        on_progress(transferred);
    }
    drop(stream); // ensure the device's nc sees EOF and exits.

    listener.finish()?;

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
    on_progress: impl FnMut(u64),
) -> Result<[u8; 20]> {
    restore_partition_reader(
        device_path,
        &mut std::io::Cursor::new(data),
        data.len() as u64,
        on_progress,
    )
}

/// Restore an already validated, seekable snapshot with constant memory.
/// The caller must validate/decompress the entire image before calling this;
/// this function checks its exact length again before opening a device writer.
pub fn restore_partition_reader<R: Read + Seek>(
    device_path: &str,
    data: &mut R,
    expected_size: u64,
    mut on_progress: impl FnMut(u64),
) -> Result<[u8; 20]> {
    let actual = data.seek(SeekFrom::End(0))?;
    if actual != expected_size {
        return Err(NcError::SizeMismatch {
            expected: expected_size,
            got: actual,
        });
    }
    data.rewind()?;
    let host = host_str();
    preflight(&host, device_path, expected_size, true)?;
    let mut listener = RemoteListener::start(&host, device_path, true)?;
    let mut stream = listener.connect()?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    let mut hasher = Sha1::new();
    let mut buffer = [0; IO_BUF];
    let mut sent = 0;
    while sent < expected_size {
        let limit = (expected_size - sent).min(buffer.len() as u64) as usize;
        let count = data.read(&mut buffer[..limit])?;
        if count == 0 {
            return Err(NcError::SizeMismatch {
                expected: expected_size,
                got: sent,
            });
        }
        stream.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
        sent += count as u64;
        on_progress(sent);
    }
    stream.shutdown(std::net::Shutdown::Write)?;
    drop(stream);
    listener.finish()?;

    ensure_success(ssh::exec(&host, "sync")?)?;
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

const REMOTE_LOCK: &str = "/tmp/chronos-mmc-transfer";
static NEXT_LISTENER: AtomicU64 = AtomicU64::new(1);
// Chronos' A33 kernel has IPv6 disabled. Only add tcp6 when it exists;
// passing a missing input file to BusyBox awk can abort readiness detection.
const TCP_TABLE_FILES: &str = "pce_tcp_tables=/proc/net/tcp; if test -r /proc/net/tcp6; then pce_tcp_tables=\"$pce_tcp_tables /proc/net/tcp6\"; fi;";

/// Own exactly one remote listener. On every local transfer error, close its
/// nc process and await the SSH worker before releasing the application's
/// operation lock. Never kill unrelated nc processes or another recovery app.
struct RemoteListener {
    host: String,
    token: String,
    worker: Option<std::thread::JoinHandle<ssh::Result<ssh::ExecOutput>>>,
    cancelled: Arc<AtomicBool>,
}
impl RemoteListener {
    fn start(host: &str, path: &str, writing: bool) -> Result<Self> {
        let token = format!(
            "{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_LISTENER.fetch_add(1, Ordering::Relaxed)
        );
        // Acquire before a TCP probe: never connect to another application's
        // existing listener after our own worker failed to acquire the lock.
        let acquire = format!(
            "set -e; umask 077; mkdir {lock} || {{ echo 'Another MMC transfer is already running; finish it or restart RAM recovery.' >&2; exit 1; }}; echo '{token}' > {lock}/owner; {TCP_TABLE_FILES} if busybox awk '$2 ~ /:1389$/ && $4 == \"0A\" {{ found=1 }} END {{ exit !found }}' $pce_tcp_tables; then rm -f {lock}/owner; rmdir {lock}; echo 'TCP transfer port is already in use; close the other recovery utility.' >&2; exit 1; fi",
            lock = REMOTE_LOCK
        );
        ensure_success(ssh::exec(host, &acquire)?)?;
        let command = listener_command(path, writing, &token);
        let remote_host = host.to_owned();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        let worker =
            std::thread::spawn(move || ssh::exec_cancellable(&remote_host, &command, cancellation));
        Ok(Self {
            host: host.to_owned(),
            token,
            worker: Some(worker),
            cancelled,
        })
    }
    fn connect(&self) -> Result<TcpStream> {
        let deadline = Instant::now() + LISTENER_READY_TIMEOUT;
        let check = listener_ready_command(&self.token);
        while Instant::now() < deadline {
            if self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
            {
                return Err(NcError::Remote(
                    "Owned transfer listener exited before accepting a connection.".into(),
                ));
            }
            let output = ssh::exec(&self.host, &check)?;
            if output.exit_status == Some(0) {
                return Ok(TcpStream::connect_timeout(
                    &SocketAddr::V4(data_addr()),
                    Duration::from_millis(500),
                )?);
            }
            if output.exit_status == Some(2) {
                return Err(NcError::Remote(
                    "Transfer listener ownership changed; no data connection was opened.".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(NcError::ListenerTimeout {
            host: self.host.clone(),
            port: DATA_PORT,
        })
    }
    fn finish(&mut self) -> Result<()> {
        // Keep ownership until the worker has actually ended: on timeout,
        // Drop must still be able to cancel its SSH wait and remote listener.
        if !wait_for_worker(self.worker.as_ref().unwrap(), Duration::from_secs(30)) {
            return Err(NcError::Remote(
                "Transfer bytes completed, but the SSH listener did not close within 30 seconds. Verification was not completed.".into(),
            ));
        }
        let worker = self.worker.take().unwrap();
        ensure_success(worker.join().map_err(|_| NcError::ListenerPanic)??)
    }
}

fn wait_for_worker<T>(worker: &std::thread::JoinHandle<T>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if worker.is_finished() {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(25)));
    }
}
impl Drop for RemoteListener {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let cancel = format!(
                "touch /tmp/chronos-mmc-cancel-{token}; if test \"$(cat {lock}/owner 2>/dev/null)\" = '{token}'; then kill \"$(cat {lock}/pid 2>/dev/null)\" 2>/dev/null || true; fi",
                token = self.token, lock = REMOTE_LOCK
            );
            let _ = ssh::exec_timeout(&self.host, &cancel, Duration::from_secs(15));
            self.cancelled.store(true, Ordering::Release);
            let _ = worker.join();
        }
        // Retain the cross-application lock through sync and remote checksum
        // verification, and release it even if SSH worker startup failed.
        let release = format!(
            "if test \"$(cat {lock}/owner 2>/dev/null)\" = '{token}'; then rm -f {lock}/pid {lock}/owner; rmdir {lock}; fi; rm -f /tmp/chronos-mmc-cancel-{token}",
            token = self.token, lock = REMOTE_LOCK
        );
        let _ = ssh::exec_timeout(&self.host, &release, Duration::from_secs(15));
    }
}
fn listener_command(path: &str, writing: bool, token: &str) -> String {
    let redirect = if writing { ">" } else { "<" };
    format!(
        "set -e; umask 077; test \"$(cat {lock}/owner 2>/dev/null)\" = '{token}'; trap 'if test -n \"$child\"; then kill \"$child\" 2>/dev/null || true; fi; rm -f {lock}/pid' EXIT HUP INT TERM; test ! -e /tmp/chronos-mmc-cancel-{token}; busybox nc -l -p {port} {redirect} {path} & child=$!; echo \"$child\" > {lock}/pid; if test -e /tmp/chronos-mmc-cancel-{token}; then kill \"$child\" 2>/dev/null || true; fi; wait \"$child\"",
        lock = REMOTE_LOCK, port = DATA_PORT
    )
}
fn listener_ready_command(token: &str) -> String {
    // Require the actual listening socket to be owned by our nc process.
    // A successful connection to an unrelated service is never readiness.
    format!(
        "test \"$(cat {lock}/owner 2>/dev/null)\" = '{token}' || exit 2; pid=$(cat {lock}/pid 2>/dev/null); test -n \"$pid\" && kill -0 \"$pid\" 2>/dev/null || exit 1; {TCP_TABLE_FILES} sockets=$(busybox awk '$2 ~ /:1389$/ && $4 == \"0A\" {{print $10}}' $pce_tcp_tables); for fd in /proc/\"$pid\"/fd/*; do target=$(busybox readlink \"$fd\" 2>/dev/null); for inode in $sockets; do test \"$target\" = \"socket:[$inode]\" && exit 0; done; done; exit 1",
        lock = REMOTE_LOCK
    )
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
        &format!(
            "test -b {path} && test ! -L {path} && \
             cat /sys/class/block/{name}/size && \
             cat /sys/class/block/{name}/dev && \
             busybox stat -c '%t:%T' {path} && \
             printf 'PCE_MMC\\n' && \
             for pce_device in /sys/class/block/mmcblk0/dev /sys/class/block/mmcblk0p*/dev; do \
                 test -r \"$pce_device\" && cat \"$pce_device\" || exit 1; \
             done && \
             printf 'PCE_MOUNTS\\n' && cat /proc/mounts && \
             printf 'PCE_MOUNTINFO\\n' && cat /proc/self/mountinfo"
        ),
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
    let invalid = || NcError::Preflight("Console device or mount information is incomplete".into());
    let target_device = lines
        .next()
        .and_then(|s| parse_device_number(s, 10))
        .ok_or_else(invalid)?;
    let actual_device = lines
        .next()
        .and_then(|s| parse_device_number(s, 16))
        .ok_or_else(invalid)?;
    if target_device != actual_device {
        return Err(NcError::Preflight(
            "Console device node does not match the selected partition".into(),
        ));
    }
    if lines.next() != Some("PCE_MMC") {
        return Err(invalid());
    }
    let mut mmc_devices = Vec::new();
    loop {
        match lines.next() {
            Some("PCE_MOUNTS") => break,
            Some(value) => mmc_devices.push(parse_device_number(value, 10).ok_or_else(invalid)?),
            None => return Err(invalid()),
        }
    }
    if !mmc_devices.contains(&target_device) {
        return Err(invalid());
    }
    let mut mounts = String::new();
    loop {
        match lines.next() {
            Some("PCE_MOUNTINFO") => break,
            Some(value) => {
                mounts.push_str(value);
                mounts.push('\n');
            }
            None => return Err(invalid()),
        }
    }
    // A live filesystem cannot provide a coherent block-level backup; the
    // post-transfer checksum is not a substitute for a stable source image.
    if crate::console::classify_mounts(&mounts) != crate::console::Environment::RamRecovery {
        return Err(NcError::Preflight(format!(
            "Boot the console into RAM recovery before {} partitions.",
            if writing { "restoring" } else { "backing up" }
        )));
    }
    let mut mount_count = 0;
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // Linux mountinfo reports the filesystem's major:minor regardless of
        // the displayed device alias (/dev/root, UUID, bind mount, etc.).
        if fields.len() < 10 || !fields[6..].contains(&"-") {
            return Err(invalid());
        }
        let device = parse_device_number(fields[2], 10).ok_or_else(invalid)?;
        if device == target_device || (path == "/dev/mmcblk0" && mmc_devices.contains(&device)) {
            return Err(NcError::Preflight(
                "Target is mounted. Unmount it in RAM recovery before transferring partitions."
                    .into(),
            ));
        }
        mount_count += 1;
    }
    if mount_count == 0 {
        return Err(invalid());
    }
    Ok(())
}

fn parse_device_number(value: &str, radix: u32) -> Option<(u32, u32)> {
    let (major, minor) = value.trim().split_once(':')?;
    Some((
        u32::from_str_radix(major, radix).ok()?,
        u32::from_str_radix(minor, radix).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_listener_wait_is_bounded_and_preserves_worker_for_cleanup() {
        let (release, waiting) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || waiting.recv().unwrap());
        assert!(!wait_for_worker(&worker, Duration::ZERO));
        // A timeout neither joins nor consumes the handle. The owner can
        // still cancel the operation and join the released worker.
        release.send(()).unwrap();
        assert!(wait_for_worker(&worker, Duration::from_secs(1)));
        worker.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn socket_tables_support_ipv4_only_and_optional_ipv6_kernels() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("tcp"),
            "0: 00000000:1389 00000000:0000 0A 0:0 0:0 0 0 0 777\n").unwrap();
        // Execute the actual shell selection with proc-table fixtures. A
        // missing tcp6 must not abort awk on Chronos' IPv4-only kernel.
        let selection = TCP_TABLE_FILES.replace("/proc/net/tcp6", "tcp6")
            .replace("/proc/net/tcp", "tcp");
        let command = format!("{selection} awk '$2 ~ /:1389$/ && $4 == \"0A\" {{print $10}}' $pce_tcp_tables");
        for ipv6 in [false, true] {
            if ipv6 {
                std::fs::write(directory.path().join("tcp6"),
                    "0: 0000000000000000:1389 0000000000000000:0000 0A 0:0 0:0 0 0 0 888\n").unwrap();
            }
            let output = std::process::Command::new("sh").args(["-ec", &command])
                .current_dir(directory.path()).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), if ipv6 { "777\n888\n" } else { "777\n" });
        }
    }

    #[test]
    fn streaming_restore_rejects_wrong_source_length_without_console_traffic() {
        let mut short = std::io::Cursor::new(b"short");
        assert!(matches!(
            restore_partition_reader("/dev/mmcblk0p6", &mut short, 8 * 1024 * 1024, |_| {}),
            Err(NcError::SizeMismatch {
                expected: 8_388_608,
                got: 5
            })
        ));
    }

    #[test]
    fn readiness_checks_owned_socket_and_does_not_claim_tcp_reachability() {
        let ready = listener_ready_command("123-456");
        assert!(ready.contains("owner"));
        assert!(ready.contains("/proc/\"$pid\"/fd/*"));
        assert!(ready.contains("socket:[$inode]"));
        let start = listener_command("/dev/mmcblk0p7", true, "123-456");
        assert!(start.contains("-p 5001 > /dev/mmcblk0p7"));
        assert!(!start.contains("killall"));
        // The owning guard, not nc EOF, releases the lock after remote hash.
        assert!(!start.contains("rmdir"));
    }

    fn snapshot(
        sectors: u64,
        target: &str,
        device_node: &str,
        mounts: &str,
        mountinfo: &str,
    ) -> String {
        format!(
            "{sectors}\n{target}\n{device_node}\nPCE_MMC\n179:0\n179:7\n179:9\nPCE_MOUNTS\n{mounts}PCE_MOUNTINFO\n{mountinfo}"
        )
    }

    const RAM_MOUNTS: &str = "rootfs / rootfs rw 0 0\n";
    const RAM_MOUNTINFO: &str = "1 0 0:1 / / rw - rootfs rootfs rw\n";

    #[test]
    fn stable_ram_sources_and_layout_are_required_for_reads_and_writes() {
        for writing in [false, true] {
            assert!(validate_target(
                &snapshot(2, "179:7", "b3:7", RAM_MOUNTS, RAM_MOUNTINFO),
                "/dev/mmcblk0p7",
                1024,
                writing
            )
            .is_ok());
            assert!(validate_target(
                &snapshot(3, "179:7", "b3:7", RAM_MOUNTS, RAM_MOUNTINFO),
                "/dev/mmcblk0p7",
                1024,
                writing
            )
            .is_err());
            assert!(validate_target(
                &snapshot(
                    2,
                    "179:7",
                    "b3:7",
                    "/dev/root / ext4 rw 0 0\n",
                    RAM_MOUNTINFO
                ),
                "/dev/mmcblk0p7",
                1024,
                writing
            )
            .is_err());
            assert!(validate_target(
                &snapshot(2, "179:7", "b3:9", RAM_MOUNTS, RAM_MOUNTINFO),
                "/dev/mmcblk0p7",
                1024,
                writing
            )
            .is_err());
        }
    }

    #[test]
    fn mount_aliases_and_bind_mounts_cannot_hide_the_target() {
        for alias in [
            "/dev/mmcblk0p7",
            "/dev/root",
            "UUID=012345",
            "/dev/block/179:7",
        ] {
            let mounts = format!("{RAM_MOUNTS}{alias} /linux ext4 rw 0 0\n");
            let mountinfo =
                format!("{RAM_MOUNTINFO}2 1 179:7 /nested /linux rw - ext4 {alias} rw\n");
            for writing in [false, true] {
                assert!(validate_target(
                    &snapshot(2, "179:7", "b3:7", &mounts, &mountinfo),
                    "/dev/mmcblk0p7",
                    1024,
                    writing
                )
                .is_err());
            }
        }
    }

    #[test]
    fn full_device_rejects_any_mounted_mmc_child_but_other_partitions_can_be_read() {
        let mounts = format!("{RAM_MOUNTS}/dev/root /game ext4 ro 0 0\n");
        let mountinfo = format!("{RAM_MOUNTINFO}2 1 179:9 / /game ro - ext4 /dev/root ro\n");
        for writing in [false, true] {
            assert!(validate_target(
                &snapshot(2, "179:0", "b3:0", &mounts, &mountinfo),
                "/dev/mmcblk0",
                1024,
                writing
            )
            .is_err());
            assert!(validate_target(
                &snapshot(2, "179:7", "b3:7", &mounts, &mountinfo),
                "/dev/mmcblk0p7",
                1024,
                writing
            )
            .is_ok());
        }
    }

    #[test]
    fn incomplete_mount_information_fails_closed() {
        for mounts in [
            "",
            "not mountinfo\n",
            "1 0 invalid / / rw - rootfs rootfs rw\n",
        ] {
            assert!(validate_target(
                &snapshot(2, "179:7", "b3:7", RAM_MOUNTS, mounts),
                "/dev/mmcblk0p7",
                1024,
                false
            )
            .is_err());
        }
    }
}
