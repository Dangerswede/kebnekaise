//! High-level access to the Mountain Everest Max.
//!
//! [`Keyboard::new`] is read-only: its [`Session`] has [`Policy::ReadOnly`]
//! and only sends `11 00`, `11 12` and `11 14`. [`Keyboard::for_lighting`]
//! also allows the lighting commands, and [`Keyboard::apply_lighting`] sends
//! a [`lighting::LightingPlan`] only when the firmware gate allows writes.

pub mod lighting;

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
    #[error("writes are refused on untested firmware {0} (pass --force to override)")]
    WritesRefused(FirmwareVersion),
    #[error("this session was opened read-only")]
    ReadOnlySession,
    #[error("the keyboard reported no valid active profile")]
    NoActiveProfile,
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
    /// `None` when the keyboard did not answer `11 12`. Some firmware
    /// answers it only once the channel is awake, so it is not fatal.
    pub layout_code: Option<u8>,
    pub layout: Option<Layout>,
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
    pub layout: Option<[u8; PACKET_SIZE]>,
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
    m.serialize_entry("11 12", &raw.layout.as_ref().map(|l| hex(l)))?;
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

    /// Wraps an open transport for lighting: reads plus the lighting
    /// commands, nothing else. Nothing is sent yet.
    pub fn for_lighting(transport: T) -> Self {
        Keyboard {
            session: Session::new(transport, Policy::Lighting),
            options: QueryOptions::default(),
        }
    }

    /// Sends a lighting plan, pausing after each packet as the plan says,
    /// then reads `11 00` back (for up to about 3 s) to check that the
    /// expected profile and lighting slot are active.
    ///
    /// Refuses unless `gate` allows writes and the session was opened with
    /// [`Keyboard::for_lighting`]. Every packet still passes the session's
    /// allowlist.
    pub fn apply_lighting(
        &mut self,
        plan: &lighting::LightingPlan,
        gate: WriteGate,
    ) -> Result<lighting::ApplyOutcome> {
        if let WriteGate::Refused { firmware } = gate {
            return Err(Error::WritesRefused(firmware));
        }
        if self.session.policy() != Policy::Lighting {
            return Err(Error::ReadOnlySession);
        }
        // Check the whole plan against the allowlist before the first write,
        // so a refusal can never leave the keyboard half-way.
        plan.dry_run()?;
        for step in &plan.steps {
            self.session.send(&step.packet)?;
            std::thread::sleep(step.wait_after);
        }
        self.session.drain()?;

        let deadline = std::time::Instant::now() + lighting::VERIFY_FOR;
        loop {
            let reply = self.session.query(&Packet::firmware_info(), self.options)?;
            let after = FirmwareInfo::parse(&reply)?;
            let verified = after.active_profile == Some(plan.expected.profile)
                && after.lighting_slot.as_byte() == plan.expected.lighting_slot;
            if verified || std::time::Instant::now() >= deadline {
                return Ok(lighting::ApplyOutcome {
                    packets_sent: plan.steps.len(),
                    firmware_after: after,
                    verified,
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }

    pub fn with_query_options(mut self, options: QueryOptions) -> Self {
        self.options = options;
        self
    }

    /// Reads firmware, layout and attached modules.
    ///
    /// The order follows what every known client does: `11 12` (which also
    /// wakes the channel), `11 14`, then `11 00`. A missing answer to `11 12`
    /// is tolerated, as everest-mac does; the other two are required.
    pub fn read_report(&mut self, force: bool) -> Result<DeviceReport> {
        self.session.drain()?;
        let layout_raw = match self.session.query(&Packet::layout(), self.options) {
            Ok(r) => Some(r),
            Err(everest_hid::Error::NoReply(_)) => None,
            Err(e) => return Err(e.into()),
        };
        let extended_raw = self.session.query(&Packet::extended_info(), self.options)?;
        let firmware_raw = self.session.query(&Packet::firmware_info(), self.options)?;

        let parsed_layout = layout_raw.as_ref().map(|r| parse_layout(r)).transpose()?;
        let layout_code = parsed_layout.map(|(code, _)| code);
        let layout = parsed_layout.map(|(_, layout)| layout);
        let extended = ExtendedInfo::parse(&extended_raw)?;
        let firmware = FirmwareInfo::parse(&firmware_raw)?;
        Ok(DeviceReport {
            write_gate: WriteGate::decide(firmware.version, force),
            firmware,
            layout_code,
            layout,
            layout_code_known: layout_code.is_some_and(Layout::is_known_code),
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
        assert_eq!(r.layout, Some(Layout::Nordic));
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
    fn missing_layout_reply_is_tolerated() {
        let mut m = MockTransport::default();
        m.always([0x11, 0x14], vec![reply(&[0x11, 0x14], &[(20, 1)])]);
        m.always(
            [0x11, 0x00],
            vec![reply(&[0x11, 0x00], &[(4, 0x57), (10, 1)])],
        );
        let opts = QueryOptions {
            timeout: Duration::from_millis(1),
            attempts: 1,
        };
        let r = Keyboard::new(m)
            .with_query_options(opts)
            .read_report(false)
            .unwrap();
        assert_eq!(r.layout, None);
        assert!(r.raw.layout.is_none());
        assert_eq!(r.firmware.version, FirmwareVersion(0x57));
    }

    fn lighting_mock(slot_after: u8) -> MockTransport {
        let mut m = scripted(0x57);
        // First 11 00 (read_report): profile 3, slot 1. Afterwards: slot_after.
        m.on(
            [0x11, 0x00],
            vec![reply(&[0x11, 0x00], &[(4, 0x57), (10, 3), (11, 1)])],
        );
        m.always(
            [0x11, 0x00],
            vec![reply(
                &[0x11, 0x00],
                &[(4, 0x57), (10, 3), (11, slot_after)],
            )],
        );
        m
    }

    fn static_plan(profile: u8) -> lighting::LightingPlan {
        let s = everest_proto::EffectSettings::new(
            everest_proto::Effect::Static,
            None,
            vec![everest_proto::Rgb::new(255, 0, 0)],
            None,
            100,
            None,
        )
        .unwrap();
        lighting::LightingPlan::effect(profile, &s, false).unwrap()
    }

    #[test]
    fn apply_static_without_save() {
        let mut kb = Keyboard::for_lighting(lighting_mock(0));
        let report = kb.read_report(false).unwrap();
        let plan = static_plan(report.firmware.active_profile.unwrap());
        let out = kb.apply_lighting(&plan, report.write_gate).unwrap();
        assert!(out.verified);
        assert_eq!(out.packets_sent, 2);
        let written = kb.into_transport().written().to_vec();
        let cmds: Vec<_> = written.iter().map(|p| [p[0], p[1]]).collect();
        assert_eq!(
            cmds,
            [
                [0x11, 0x12],
                [0x11, 0x14],
                [0x11, 0x00],
                [0x14, 0x00],
                [0x14, 0x2C],
                [0x11, 0x00]
            ]
        );
        assert_eq!(written[3][..6], [0x14, 0x00, 0x00, 0x00, 0x03, 0x00]);
        assert!(!written.iter().any(|p| p[0] == 0x13));
    }

    #[test]
    fn apply_static_with_save() {
        let mut kb = Keyboard::for_lighting(lighting_mock(0));
        let report = kb.read_report(false).unwrap();
        let s = everest_proto::EffectSettings::new(
            everest_proto::Effect::Static,
            None,
            vec![everest_proto::Rgb::new(0, 0x60, 0xff)],
            None,
            100,
            None,
        )
        .unwrap();
        let plan = lighting::LightingPlan::effect(3, &s, true).unwrap();
        let out = kb.apply_lighting(&plan, report.write_gate).unwrap();
        assert!(out.verified);
        let written = kb.into_transport().written().to_vec();
        assert_eq!(written[5][..5], [0x13, 0x55, 0x00, 0x00, 0x00]);
        assert!(written[6].starts_with(&[0x11, 0x00]));
    }

    #[test]
    fn apply_refused_on_untested_firmware() {
        let mut kb = Keyboard::for_lighting(scripted(0x55));
        let report = kb.read_report(false).unwrap();
        assert!(matches!(
            kb.apply_lighting(&static_plan(3), report.write_gate),
            Err(Error::WritesRefused(_))
        ));
        assert_eq!(kb.into_transport().written().len(), 3); // the reads only
    }

    #[test]
    fn apply_refused_on_read_only_session() {
        let mut kb = Keyboard::new(scripted(0x57));
        let report = kb.read_report(false).unwrap();
        assert!(matches!(
            kb.apply_lighting(&static_plan(3), report.write_gate),
            Err(Error::ReadOnlySession)
        ));
        assert_eq!(kb.into_transport().written().len(), 3);
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
