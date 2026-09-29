use crate::error::{FelError, Result};
use crate::protocol::*;
use futures_lite::future::block_on;
use nusb::transfer::RequestBuffer;
use nusb::{Device, Interface};
use std::time::{Duration, Instant};
use tracing::{debug, trace};
use zerocopy::{FromBytes, IntoBytes};

pub const FEL_VID: u16 = 0x1f3a;
pub const FEL_PID: u16 = 0xefe8;

/// What state the console is in, decided from the bulk endpoint addresses
/// exposed by the active interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Boot ROM USB-mass-storage stub. OUT=0x02, IN=0x81. Only the trigger CBW
    /// is meaningful here.
    BootRom,
    /// Full FEL protocol. OUT=0x01, IN=0x82. AWUC requests work.
    Fel,
}

#[derive(Debug, Clone, Copy)]
struct Endpoints {
    out: u8,
    in_: u8,
}

const BOOTROM_EP: Endpoints = Endpoints {
    out: 0x02,
    in_: 0x81,
};
const FEL_EP: Endpoints = Endpoints {
    out: 0x01,
    in_: 0x82,
};

pub struct FelDevice {
    _device: Device,
    interface: Interface,
    state: DeviceState,
    eps: Endpoints,
}

impl FelDevice {
    /// Open the only connected device with VID:PID 1f3a:efe8 and decide its
    /// state by inspecting the active interface's endpoints.
    pub fn open() -> Result<Self> {
        let mut matches = nusb::list_devices()
            .map_err(|e| FelError::Usb(e.to_string()))?
            .filter(|d| d.vendor_id() == FEL_VID && d.product_id() == FEL_PID);
        let info = matches.next().ok_or(FelError::NotFound)?;
        if matches.next().is_some() {
            return Err(FelError::Usb("Connect only one PCE Mini at a time.".into()));
        }

        debug!(
            bus = info.bus_number(),
            addr = info.device_address(),
            "opening device"
        );
        let device = info.open().map_err(|e| FelError::Usb(e.to_string()))?;

        let (state, (out, in_)) = detect_state(&device)?;
        debug!(?state, out, in_, "detected state");
        let eps = Endpoints { out, in_ };

        let interface = device
            .detach_and_claim_interface(0)
            .map_err(|e| FelError::Usb(e.to_string()))?;
        Ok(Self {
            _device: device,
            interface,
            state,
            eps,
        })
    }

    pub fn state(&self) -> DeviceState {
        self.state
    }

    /// FEL_READ: pull `len` bytes from device memory at `addr`. Chunks
    /// internally so callers can request megabytes in one call.
    pub fn read(&self, addr: u32, len: usize) -> Result<Vec<u8>> {
        self.require_fel()?;
        let mut out = Vec::with_capacity(len);
        let mut cur = addr;
        let mut remaining = len;
        while remaining > 0 {
            let n = remaining.min(Self::CHUNK);
            self.fel_request(AW_FEL_READ, cur, n as u32)?;
            let chunk = self.fel_read_data(n)?;
            self.read_fel_status()?;
            out.extend_from_slice(&chunk);
            cur = cur.wrapping_add(n as u32);
            remaining -= n;
        }
        Ok(out)
    }

    /// FEL_WRITE: push `data` to device memory starting at `addr`.
    pub fn write(&self, addr: u32, data: &[u8]) -> Result<()> {
        self.write_progress(addr, data, |_| {})
    }

    /// FEL_WRITE with a per-chunk progress callback. The callback receives
    /// the number of bytes written by the just-completed chunk (not the
    /// running total — let the caller accumulate if needed).
    pub fn write_progress<F: FnMut(usize)>(
        &self,
        addr: u32,
        data: &[u8],
        mut on_chunk: F,
    ) -> Result<()> {
        self.require_fel()?;
        let mut cur = addr;
        for chunk in data.chunks(Self::CHUNK) {
            self.fel_request(AW_FEL_WRITE, cur, chunk.len() as u32)?;
            self.fel_write_data(chunk)?;
            self.read_fel_status()?;
            cur = cur.wrapping_add(chunk.len() as u32);
            on_chunk(chunk.len());
        }
        Ok(())
    }

    /// Inject a small ARM thunk at `scratch_addr` that clears `SCTLR.I`,
    /// invalidates the entire I-cache (`ICIALLU`), barriers, and returns —
    /// then execute it.
    ///
    /// Why this is needed: when we hand control to a freshly-written U-Boot
    /// at an address that previously hosted a *different* U-Boot (e.g. the
    /// factory image), the CPU's I-cache can still hold instructions from
    /// the old binary at that VA. Fetching from I-cache after our DRAM
    /// re-init would execute stale code despite a correct DRAM image.
    /// Upstream sunxi-tools handles this through `aw_disable_icache` for
    /// SoCs with `icache_fix=true`; the recovery-boot path on R16 needs the
    /// same treatment even though R16 isn't tagged.
    ///
    /// Source: linux-sunxi/sunxi-tools `fel_lib.c` `aw_disable_icache`.
    pub fn disable_icache(&self, scratch_addr: u32) -> Result<()> {
        // 6 ARM instructions, all little-endian.
        let thunk: [u32; 6] = [
            0xee11_0f10, // mrc 15, 0, r0, cr1, cr0, {0}   ; r0 = SCTLR
            0xe3c0_0a01, // bic r0, r0, #0x1000            ; clear SCTLR.I
            0xee01_0f10, // mcr 15, 0, r0, cr1, cr0, {0}   ; SCTLR = r0
            0xee07_0f15, // mcr 15, 0, r0, cr7, cr5, {0}   ; ICIALLU (invalidate I-cache)
            0xf57f_f06f, // isb sy                          ; instruction barrier
            0xe12f_ff1e, // bx  lr                          ; return
        ];
        let mut bytes = [0u8; 24];
        for (i, w) in thunk.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        self.write(scratch_addr, &bytes)?;
        self.exe(scratch_addr)?;
        Ok(())
    }

    /// FEL_EXEC: jump to `addr`, then wait for the SoC to return to FEL and
    /// acknowledge. Use this for code that comes back (e.g. FES1 SPL after
    /// DRAM init).
    pub fn exe(&self, addr: u32) -> Result<()> {
        self.require_fel()?;
        self.fel_request(AW_FEL_EXEC, addr, 0)?;
        self.read_fel_status()?;
        Ok(())
    }

    /// FEL_EXEC variant for code that will NOT return (e.g. final U-Boot
    /// hand-off).
    ///
    /// Empirically, the FEL firmware does NOT actually perform the jump
    /// until the host issues the standard `aw_read_fel_status` sequence
    /// (AWUC(READ,8) + 8-byte status + AWUS). It seems the firmware queues
    /// those bytes for transmission and only proceeds to the jump in its
    /// main loop after the AWUC(READ,8) prompt is received. Skipping this
    /// leaves the device parked in FEL with the device still on USB.
    ///
    /// We perform the full upstream sequence and tolerate USB errors on
    /// the trailing reads — by the time we read the AWUS the device may
    /// already have jumped and torn down its USB endpoint.
    pub fn exe_no_return(&self, addr: u32) -> Result<()> {
        self.require_fel()?;
        self.fel_request(AW_FEL_EXEC, addr, 0)?;
        let _ = self.read_fel_status();
        Ok(())
    }

    /// Every high-level FEL op finishes with an 8-byte status read wrapped in
    /// its own AWUC(READ) envelope. Matches `aw_read_fel_status` in upstream
    /// `fel_lib.c`. Bytes are discarded (sunxi-tools does the same).
    fn read_fel_status(&self) -> Result<()> {
        let _ = self.fel_read_data(8)?;
        Ok(())
    }

    fn require_fel(&self) -> Result<()> {
        if self.state == DeviceState::Fel {
            Ok(())
        } else {
            Err(FelError::WrongState {
                expected: DeviceState::Fel,
                actual: self.state,
            })
        }
    }

    /// Maximum size of a single FEL data-phase transfer. Matches upstream
    /// sunxi-tools `AW_USB_MAX_BULK_SEND` (512 KiB). The upstream comment:
    /// "10-second timeout, slow transfers ~64 KiB/sec, max chunk in ~8 s".
    const CHUNK: usize = 512 * 1024;

    /// AW_FEL_VERSION → 32-byte version block. Only valid in `Fel` state.
    pub fn version(&self) -> Result<FelVersion> {
        self.require_fel()?;
        self.fel_request(AW_FEL_VERSION, 0, 0)?;
        let buf = self.fel_read_data(std::mem::size_of::<FelVersion>())?;
        self.read_fel_status()?;

        let v = FelVersion::read_from_bytes(&buf).map_err(|_| FelError::ShortTransfer {
            expected: 32,
            actual: buf.len(),
        })?;
        if &v.signature != b"AWUSBFEX" {
            return Err(FelError::BadVersion(v.signature));
        }
        Ok(v)
    }

    // --- low-level framing ---------------------------------------------------
    //
    // Each high-level FEL operation is two USB "phases":
    //
    //   Phase 1 (request):  AWUC(WRITE, 16) + 16-byte FEL request + AWUS status
    //   Phase 2 (data):     AWUC(READ|WRITE, N) + N bytes data + AWUS status
    //
    // For FEL_VERSION the data phase is a 32-byte read. For FEL_WRITE it's an
    // outbound write of N bytes. For FEL_READ it's an inbound read of N bytes.
    // FEL_EXEC has no data phase.

    /// Phase 1: issue a FEL request (no data phase).
    fn fel_request(&self, req: u32, addr: u32, len: u32) -> Result<()> {
        let fel = FelRequest::new(req, addr, len);
        let env = AwUsbRequest::new(AW_USB_WRITE, fel.as_bytes().len() as u32);
        trace!(req, addr, len, "FEL request");
        self.bulk_out(env.as_bytes())?;
        self.bulk_out(fel.as_bytes())?;
        self.read_status()?;
        Ok(())
    }

    /// Phase 2 (read flavor): pull `len` bytes back from the device.
    fn fel_read_data(&self, len: usize) -> Result<Vec<u8>> {
        let env = AwUsbRequest::new(AW_USB_READ, len as u32);
        trace!(len, "FEL data read");
        self.bulk_out(env.as_bytes())?;
        let data = self.bulk_in(len)?;
        self.read_status()?;
        Ok(data)
    }

    /// Phase 2 (write flavor): push `data` to the device.
    fn fel_write_data(&self, data: &[u8]) -> Result<()> {
        let env = AwUsbRequest::new(AW_USB_WRITE, data.len() as u32);
        trace!(len = data.len(), "FEL data write");
        self.bulk_out(env.as_bytes())?;
        self.bulk_out(data)?;
        self.read_status()?;
        Ok(())
    }

    fn bulk_out(&self, data: &[u8]) -> Result<()> {
        let comp = block_on(self.interface.bulk_out(self.eps.out, data.to_vec()));
        comp.status
            .map_err(|e| FelError::Usb(format!("OUT: {e}")))?;
        Ok(())
    }

    fn bulk_in(&self, len: usize) -> Result<Vec<u8>> {
        let comp = block_on(
            self.interface
                .bulk_in(self.eps.in_, RequestBuffer::new(len)),
        );
        comp.status.map_err(|e| FelError::Usb(format!("IN: {e}")))?;
        let data = comp.data;
        if data.len() != len {
            return Err(FelError::ShortTransfer {
                expected: len,
                actual: data.len(),
            });
        }
        Ok(data)
    }

    fn read_status(&self) -> Result<()> {
        let buf = self.bulk_in(std::mem::size_of::<AwUsbStatus>())?;
        let st = AwUsbStatus::read_from_bytes(&buf).map_err(|_| FelError::ShortTransfer {
            expected: 13,
            actual: buf.len(),
        })?;
        if &st.signature != AWUS_MAGIC || st.csw_status != 0 {
            return Err(FelError::BadStatus {
                signature: st.signature,
                csw_status: st.csw_status,
            });
        }
        Ok(())
    }
}

/// Walk the active configuration's first interface and decide which state
/// pair (BootRom vs FEL) the endpoints match.
///
/// Public so PCE-specific layers (e.g. `pce-fel`) can decide whether to send
/// a trigger CBW without first having to construct a full `FelDevice`.
pub fn detect_state(device: &Device) -> Result<(DeviceState, (u8, u8))> {
    let cfg = device
        .active_configuration()
        .map_err(|e| FelError::Usb(e.to_string()))?;

    let mut has_bootrom_in = false;
    let mut has_bootrom_out = false;
    let mut has_fel_in = false;
    let mut has_fel_out = false;

    let group = cfg
        .interfaces()
        .next()
        .ok_or_else(|| FelError::Usb("device exposes no interface".into()))?;
    let alt = group
        .alt_settings()
        .next()
        .ok_or_else(|| FelError::Usb("interface exposes no alt-setting".into()))?;
    for ep in alt.endpoints() {
        match ep.address() {
            x if x == BOOTROM_EP.in_ => has_bootrom_in = true,
            x if x == BOOTROM_EP.out => has_bootrom_out = true,
            x if x == FEL_EP.in_ => has_fel_in = true,
            x if x == FEL_EP.out => has_fel_out = true,
            _ => {}
        }
    }

    if has_fel_in && has_fel_out {
        Ok((DeviceState::Fel, (FEL_EP.out, FEL_EP.in_)))
    } else if has_bootrom_in && has_bootrom_out {
        Ok((DeviceState::BootRom, (BOOTROM_EP.out, BOOTROM_EP.in_)))
    } else {
        Err(FelError::Usb(
            "device endpoints match neither BootRom (0x02/0x81) nor FEL (0x01/0x82)".into(),
        ))
    }
}

/// True if a device with VID:PID 1f3a:efe8 is currently visible on USB.
pub fn is_present() -> bool {
    nusb::list_devices()
        .map(|it| {
            it.into_iter()
                .any(|d| d.vendor_id() == FEL_VID && d.product_id() == FEL_PID)
        })
        .unwrap_or(false)
}

/// Wait for the startup USB probe or an already-running FEL device. Inspect
/// descriptors without claiming the interface: the trigger must be its first
/// owner, avoiding a usb_storage reattach between detection and the response.
/// Poll every 50 ms; allow udev a brief grace period to install the user's ACL.
pub fn wait_for_device(timeout: Duration) -> Result<DeviceState> {
    let deadline = Instant::now() + timeout;
    let mut access_deadline = None;
    loop {
        let mut matches = nusb::list_devices()
            .map_err(|e| FelError::Usb(e.to_string()))?
            .filter(|d| d.vendor_id() == FEL_VID && d.product_id() == FEL_PID);
        if let Some(info) = matches.next() {
            if matches.next().is_some() {
                return Err(FelError::Usb("Connect only one PCE Mini at a time.".into()));
            }
            match info.open() {
                Ok(device) => return detect_state(&device).map(|(state, _)| state),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    let grace = access_deadline
                        .get_or_insert_with(|| Instant::now() + Duration::from_millis(500));
                    if Instant::now() >= *grace || Instant::now() >= deadline {
                        return Err(FelError::Usb(format!(
                            "USB access denied: {e}. Install 70-pce-recovery.rules, reload udev rules and reconnect the console."
                        )));
                    }
                }
                Err(e) => return Err(FelError::Usb(e.to_string())),
            }
        } else {
            access_deadline = None;
        }
        if Instant::now() >= deadline {
            return Err(FelError::NotFound);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Block until the device is present *and* in the requested state.
pub fn wait_for_state(state: DeviceState, timeout: Duration) -> std::result::Result<(), FelError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(info) = nusb::list_devices().map(|it| {
            it.into_iter()
                .find(|d| d.vendor_id() == FEL_VID && d.product_id() == FEL_PID)
        }) {
            if let Some(info) = info {
                if let Ok(dev) = info.open() {
                    if let Ok((s, _)) = detect_state(&dev) {
                        if s == state {
                            return Ok(());
                        }
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(FelError::NotFound);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
