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
        if n < PACKET_SIZE {
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
}
