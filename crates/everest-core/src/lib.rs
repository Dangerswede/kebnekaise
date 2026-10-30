//! High-level access to the Mountain Everest Max.
//!
//! Phase 1 is read-only: [`Keyboard`] opens a [`Session`] with
//! [`Policy::ReadOnly`] and only sends `11 00`, `11 12` and `11 14`.

use everest_hid::session::QueryOptions;
use everest_hid::{Policy, Session, Transport};
use everest_proto::{
    parse_layout, ExtendedInfo, FirmwareInfo, FirmwareVersion, Layout, Packet, ParseError,
    PACKET_SIZE,
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Hid(#[from] everest_hid::Error),
    #[error("unexpected reply: {0}")]
    Parse(#[from] ParseError),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Whether writing commands may be sent to this keyboard (later phases).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WriteGate {
    /// The firmware is one writes have been tested on.
    Allowed { firmware: FirmwareVersion },
    /// Untested firmware, but the user passed `--force`.
    Forced { firmware: FirmwareVersion },
    /// Untested firmware: writes are refused.
    Refused { firmware: FirmwareVersion },
}

impl WriteGate {
    pub fn decide(firmware: FirmwareVersion, force: bool) -> Self {
        if firmware.is_tested() {
            WriteGate::Allowed { firmware }
        } else if force {
            WriteGate::Forced { firmware }
        } else {
            WriteGate::Refused { firmware }
        }
    }

    pub fn writes_allowed(self) -> bool {
        !matches!(self, WriteGate::Refused { .. })
    }
}

/// Everything phase 1 reads from the keyboard.
#[derive(Clone, Debug, Serialize)]
pub struct DeviceReport {
    pub firmware: FirmwareInfo,
    pub layout_code: u8,
    pub layout: Layout,
    pub layout_code_known: bool,
    pub extended: ExtendedInfo,
    pub write_gate: WriteGate,
    /// Raw replies, for filling the unknowns in `docs/PROTOCOL.md`.
    #[serde(serialize_with = "hex_replies")]
    pub raw: RawReplies,
}

#[derive(Clone, Debug)]
pub struct RawReplies {
    pub firmware_info: [u8; PACKET_SIZE],
    pub layout: [u8; PACKET_SIZE],
    pub extended_info: [u8; PACKET_SIZE],
}

pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn hex_replies<S: serde::Serializer>(
    raw: &RawReplies,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut m = s.serialize_map(Some(3))?;
    m.serialize_entry("11 00", &hex(&raw.firmware_info))?;
    m.serialize_entry("11 12", &hex(&raw.layout))?;
    m.serialize_entry("11 14", &hex(&raw.extended_info))?;
    m.end()
}

/// The keyboard, opened read-only.
pub struct Keyboard<T: Transport> {
    session: Session<T>,
    options: QueryOptions,
}

impl<T: Transport> Keyboard<T> {
    /// Wraps an open transport. Nothing is sent yet.
    pub fn new(transport: T) -> Self {
        Keyboard {
            session: Session::new(transport, Policy::ReadOnly),
            options: QueryOptions::default(),
        }
    }

    pub fn with_query_options(mut self, options: QueryOptions) -> Self {
        self.options = options;
        self
    }

    /// Reads firmware, layout and attached modules.
    ///
    /// The order follows what every known client does: `11 12` (which also
    /// wakes the channel), `11 14`, then `11 00`.
    pub fn read_report(&mut self, force: bool) -> Result<DeviceReport> {
        self.session.drain()?;
        let layout_raw = self.session.query(&Packet::layout(), self.options)?;
        let extended_raw = self.session.query(&Packet::extended_info(), self.options)?;
        let firmware_raw = self.session.query(&Packet::firmware_info(), self.options)?;

        let (layout_code, layout) = parse_layout(&layout_raw)?;
        let extended = ExtendedInfo::parse(&extended_raw)?;
        let firmware = FirmwareInfo::parse(&firmware_raw)?;
        Ok(DeviceReport {
            write_gate: WriteGate::decide(firmware.version, force),
            firmware,
            layout_code,
            layout,
            layout_code_known: Layout::is_known_code(layout_code),
            extended,
            raw: RawReplies {
                firmware_info: firmware_raw,
                layout: layout_raw,
                extended_info: extended_raw,
            },
        })
    }

    /// Packets seen while waiting for replies (events, other programs' traffic).
    pub fn skipped(&self) -> &[[u8; PACKET_SIZE]] {
        self.session.skipped()
    }

    pub fn into_transport(self) -> T {
        self.session.into_transport()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use everest_hid::mock::MockTransport;
    use everest_proto::{classify, LightingSlot, SafetyClass};

    use super::*;

    fn reply(prefix: &[u8], fields: &[(usize, u8)]) -> [u8; PACKET_SIZE] {
        let mut p = [0u8; PACKET_SIZE];
        p[..prefix.len()].copy_from_slice(prefix);
        for &(i, b) in fields {
            p[i] = b;
        }
        p
    }

    fn scripted(version: u8) -> MockTransport {
        let mut m = MockTransport::default();
        m.always([0x11, 0x12], vec![reply(&[0x11, 0x12], &[(4, 11)])]);
        m.always(
            [0x11, 0x14],
            vec![
                reply(&[0x01, 0x00], &[(42, 0x02)]),
                reply(&[0x11, 0x14], &[(4, 1), (10, 0x11), (20, 1)]),
            ],
        );
        m.always(
            [0x11, 0x00],
            vec![reply(&[0x11, 0x00], &[(4, version), (10, 3), (11, 1)])],
        );
        m
    }

    #[test]
    fn full_report() {
        let mut kb = Keyboard::new(scripted(0x57));
        let r = kb.read_report(false).unwrap();
        assert_eq!(r.layout, Layout::Nordic);
        assert_eq!(r.firmware.active_profile, Some(3));
        assert_eq!(r.firmware.lighting_slot, LightingSlot::Wave);
        assert!(r.extended.modules.numpad && r.extended.modules.media_dock);
        assert!(r.write_gate.writes_allowed());
        assert_eq!(kb.skipped().len(), 1);
    }

    #[test]
    fn only_read_commands_are_written() {
        let mut kb = Keyboard::new(scripted(0x57));
        kb.read_report(false).unwrap();
        let written = kb.into_transport().written().to_vec();
        assert_eq!(written.len(), 3);
        assert!(written
            .iter()
            .all(|p| classify(p).class == SafetyClass::Read));
    }

    #[test]
    fn untested_firmware_gate() {
        let r = Keyboard::new(scripted(0x55)).read_report(false).unwrap();
        assert_eq!(
            r.write_gate,
            WriteGate::Refused {
                firmware: FirmwareVersion(0x55)
            }
        );
        let r = Keyboard::new(scripted(0x55)).read_report(true).unwrap();
        assert_eq!(
            r.write_gate,
            WriteGate::Forced {
                firmware: FirmwareVersion(0x55)
            }
        );
    }

    #[test]
    fn silent_keyboard() {
        let opts = QueryOptions {
            timeout: Duration::from_millis(1),
            attempts: 1,
        };
        let mut kb = Keyboard::new(MockTransport::default()).with_query_options(opts);
        assert!(matches!(
            kb.read_report(false),
            Err(Error::Hid(everest_hid::Error::NoReply(_)))
        ));
    }
}
