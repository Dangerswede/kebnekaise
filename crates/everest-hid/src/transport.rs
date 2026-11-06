use std::ffi::CString;
use std::time::Duration;

use everest_proto::PACKET_SIZE;

use crate::{Error, Result};

/// Raw access to the interrupt channel of the command interface.
///
/// Do not call this directly from application code: go through
/// [`crate::Session`], which checks every packet against the allowlist.
pub trait Transport {
    /// Writes one 64-byte packet (without a report ID).
    fn write_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()>;
    /// Reads one 64-byte packet, or `None` after `timeout`.
    fn read_raw(&mut self, timeout: Duration) -> Result<Option<[u8; PACKET_SIZE]>>;
    /// Sends one 64-byte feature report (SET_REPORT(Feature), report ID 0).
    /// Only the picture transfer uses feature reports.
    fn send_feature_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()>;
    /// Reads the 64-byte feature report (GET_REPORT(Feature), report ID 0).
    /// `None` when the device returned nothing usable.
    fn get_feature_raw(&mut self) -> Result<Option<[u8; PACKET_SIZE]>>;
}

/// The vendor interface uses unnumbered reports (report ID 0). hidapi wants
/// the report ID as the first byte of every write, on every OS; Windows also
/// requires the full report length (1 + 64).
pub(crate) fn with_report_id(packet: &[u8; PACKET_SIZE]) -> [u8; PACKET_SIZE + 1] {
    let mut buf = [0u8; PACKET_SIZE + 1];
    buf[1..].copy_from_slice(packet);
    buf
}

/// Normalises what a read returned into a 64-byte packet.
///
/// hidapi strips report ID 0 on Windows and hidraw never adds it, so `n` is
/// normally 64. A 65-byte read with a leading zero is the report ID left in
/// place; drop it rather than shift every field by one.
pub(crate) fn normalise_read(buf: &[u8], n: usize) -> Option<[u8; PACKET_SIZE]> {
    if n == 0 {
        return None;
    }
    let data = if n == PACKET_SIZE + 1 && buf[0] == 0 {
        &buf[1..n]
    } else {
        &buf[..n.min(PACKET_SIZE)]
    };
    let mut packet = [0u8; PACKET_SIZE];
    packet[..data.len()].copy_from_slice(data);
    Some(packet)
}

/// Normalises a GET_REPORT(Feature) result into the 64 data bytes.
///
/// Every feature reply of this keyboard starts with `aa 55`. Depending on
/// the backend the data comes back at offset 0 or after the report-ID byte
/// at offset 1, so look for `aa 55` at both and refuse anything else.
pub(crate) fn normalise_feature(buf: &[u8], n: usize) -> Option<[u8; PACKET_SIZE]> {
    let n = n.min(buf.len());
    let start = if n >= 2 && buf[0] == 0xAA && buf[1] == 0x55 {
        0
    } else if n >= 3 && buf[0] == 0x00 && buf[1] == 0xAA && buf[2] == 0x55 {
        1
    } else {
        return None;
    };
    let data = &buf[start..n];
    let mut packet = [0u8; PACKET_SIZE];
    let len = data.len().min(PACKET_SIZE);
    packet[..len].copy_from_slice(&data[..len]);
    Some(packet)
}

/// [`Transport`] over hidapi: hidraw on Linux, the built-in HID driver on Windows.
pub struct HidTransport {
    device: hidapi::HidDevice,
}

impl HidTransport {
    pub fn open(api: &hidapi::HidApi, path: &str) -> Result<Self> {
        let cpath = CString::new(path).map_err(|e| Error::Hid(e.to_string()))?;
        let device = api
            .open_path(&cpath)
            .map_err(|e| Error::Hid(open_hint(path, &e.to_string())))?;
        Ok(HidTransport { device })
    }
}

fn open_hint(path: &str, err: &str) -> String {
    if cfg!(target_os = "linux") {
        format!(
            "cannot open {path}: {err}. Is the udev rule installed? \
             See packaging/linux/70-mountain-everest.rules"
        )
    } else {
        format!("cannot open {path}: {err}")
    }
}

impl Transport for HidTransport {
    fn write_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()> {
        let buf = with_report_id(packet);
        let n = self
            .device
            .write(&buf)
            .map_err(|e| Error::Hid(e.to_string()))?;
        // The Windows backend reports 0 when WriteFile completes
        // synchronously, so only a partial count is an error.
        if n != 0 && n < PACKET_SIZE {
            return Err(Error::Hid(format!(
                "short write: {n} of {} bytes",
                buf.len()
            )));
        }
        Ok(())
    }

    fn read_raw(&mut self, timeout: Duration) -> Result<Option<[u8; PACKET_SIZE]>> {
        let mut buf = [0u8; PACKET_SIZE + 1];
        let ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        let n = self
            .device
            .read_timeout(&mut buf, ms)
            .map_err(|e| Error::Hid(e.to_string()))?;
        Ok(normalise_read(&buf, n))
    }

    fn send_feature_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()> {
        let buf = with_report_id(packet);
        self.device
            .send_feature_report(&buf)
            .map_err(|e| Error::Hid(e.to_string()))
    }

    fn get_feature_raw(&mut self) -> Result<Option<[u8; PACKET_SIZE]>> {
        let mut buf = [0u8; PACKET_SIZE + 1];
        let n = self
            .device
            .get_feature_report(&mut buf)
            .map_err(|e| Error::Hid(e.to_string()))?;
        match normalise_feature(&buf, n) {
            Some(p) => Ok(Some(p)),
            // An empty report is "no answer yet".
            None if buf[..n.min(buf.len())].iter().all(|&b| b == 0) => Ok(None),
            // Anything else means our report-ID assumption is wrong: stop
            // and show the bytes rather than guess.
            None => {
                let shown = &buf[..n.min(buf.len()).min(16)];
                let hex: Vec<String> = shown.iter().map(|b| format!("{b:02x}")).collect();
                Err(Error::Hid(format!(
                    "unexpected feature report ({n} bytes): {} ...",
                    hex.join(" ")
                )))
            }
        }
    }
}

/// A transport that never touches a device: writes are recorded, reads
/// time out. Used by `--dry-run`, so a planned command still goes through
/// the session's allowlist without anything reaching the keyboard.
#[derive(Debug, Default)]
pub struct DryRunTransport {
    written: Vec<[u8; PACKET_SIZE]>,
    features: Vec<[u8; PACKET_SIZE]>,
}

impl DryRunTransport {
    /// Everything that would have been written, in order.
    pub fn written(&self) -> &[[u8; PACKET_SIZE]] {
        &self.written
    }

    /// Every feature report that would have been sent, in order.
    pub fn features(&self) -> &[[u8; PACKET_SIZE]] {
        &self.features
    }
}

impl Transport for DryRunTransport {
    fn write_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()> {
        self.written.push(*packet);
        Ok(())
    }

    fn read_raw(&mut self, _timeout: Duration) -> Result<Option<[u8; PACKET_SIZE]>> {
        Ok(None)
    }

    fn send_feature_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()> {
        self.features.push(*packet);
        Ok(())
    }

    fn get_feature_raw(&mut self) -> Result<Option<[u8; PACKET_SIZE]>> {
        Ok(None)
    }
}

/// Wraps a transport and, when enabled, logs every packet to stderr with
/// the time since the wrapper was created. Used by `--debug`.
pub struct TracingTransport<T> {
    inner: T,
    enabled: bool,
    start: std::time::Instant,
}

impl<T: Transport> TracingTransport<T> {
    pub fn new(inner: T, enabled: bool) -> Self {
        TracingTransport {
            inner,
            enabled,
            start: std::time::Instant::now(),
        }
    }

    fn log(&self, what: &str, bytes: Option<&[u8]>) {
        if !self.enabled {
            return;
        }
        let ms = self.start.elapsed().as_millis();
        match bytes {
            Some(b) => {
                // Up to the last non-zero byte, at least the command.
                let end = b.iter().rposition(|&x| x != 0).map_or(2, |i| i + 1).max(2);
                let hex: Vec<String> = b[..end].iter().map(|x| format!("{x:02x}")).collect();
                eprintln!("[{ms:>6} ms] {what} {}", hex.join(" "));
            }
            None => eprintln!("[{ms:>6} ms] {what}"),
        }
    }
}

impl<T: Transport> Transport for TracingTransport<T> {
    fn write_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()> {
        let result = self.inner.write_raw(packet);
        match &result {
            Ok(()) => self.log("write ok  ", Some(packet)),
            Err(e) => self.log(&format!("write FAILED ({e})"), Some(packet)),
        }
        result
    }

    fn read_raw(&mut self, timeout: Duration) -> Result<Option<[u8; PACKET_SIZE]>> {
        let result = self.inner.read_raw(timeout);
        match &result {
            Ok(Some(p)) => self.log("read      ", Some(p)),
            Ok(None) => self.log(
                &format!("read timeout after {} ms", timeout.as_millis()),
                None,
            ),
            Err(e) => self.log(&format!("read FAILED ({e})"), None),
        }
        result
    }

    fn send_feature_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()> {
        let result = self.inner.send_feature_raw(packet);
        match &result {
            Ok(()) => self.log("feature > ", Some(packet)),
            Err(e) => self.log(&format!("feature > FAILED ({e})"), Some(packet)),
        }
        result
    }

    fn get_feature_raw(&mut self) -> Result<Option<[u8; PACKET_SIZE]>> {
        let result = self.inner.get_feature_raw();
        match &result {
            Ok(Some(p)) => self.log("feature < ", Some(p)),
            Ok(None) => self.log("feature < nothing usable", None),
            Err(e) => self.log(&format!("feature < FAILED ({e})"), None),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_id_prefix() {
        let mut p = [0u8; 64];
        p[0] = 0x11;
        let b = with_report_id(&p);
        assert_eq!(b.len(), 65);
        assert_eq!(&b[..2], &[0x00, 0x11]);
    }

    #[test]
    fn read_normalisation() {
        let mut buf = [0u8; 65];
        buf[0] = 0x11;
        buf[1] = 0x00;
        assert_eq!(normalise_read(&buf, 64).unwrap()[..2], [0x11, 0x00]);
        assert!(normalise_read(&buf, 0).is_none());

        let mut with_id = [0u8; 65];
        with_id[1] = 0x11;
        with_id[2] = 0x12;
        assert_eq!(normalise_read(&with_id, 65).unwrap()[..2], [0x11, 0x12]);

        // Short reads are zero-padded.
        assert_eq!(
            normalise_read(&[0x11, 0x14], 2).unwrap()[..3],
            [0x11, 0x14, 0x00]
        );
    }

    #[test]
    fn feature_normalisation() {
        let mut plain = [0u8; 65];
        plain[..4].copy_from_slice(&[0xAA, 0x55, 0x21, 0xFA]);
        assert_eq!(
            normalise_feature(&plain, 64).unwrap()[..4],
            [0xAA, 0x55, 0x21, 0xFA]
        );
        let mut with_id = [0u8; 65];
        with_id[1..5].copy_from_slice(&[0xAA, 0x55, 0x10, 0xFB]);
        let p = normalise_feature(&with_id, 65).unwrap();
        assert_eq!(p[..4], [0xAA, 0x55, 0x10, 0xFB]);
        // A full 65-byte reply with the ID keeps all 64 data bytes.
        with_id[64] = 0x77;
        assert_eq!(normalise_feature(&with_id, 65).unwrap()[63], 0x77);
        assert!(normalise_feature(&[0u8; 65], 65).is_none());
        assert!(normalise_feature(&[0x11, 0x14, 0x00], 3).is_none());
        assert!(normalise_feature(&plain, 0).is_none());
    }
}
