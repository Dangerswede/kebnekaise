use std::time::{Duration, Instant};

use everest_proto::{is_reply_to, Classification, Group, Packet, SafetyClass, PACKET_SIZE};

use crate::transport::Transport;
use crate::{Error, Result};

/// Which packets a session may send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Phase 1: queries only.
    ReadOnly,
    /// Phase 2: queries plus the lighting commands (profile/slot switch,
    /// effects, per-key colours, saving a lighting slot). Nothing else:
    /// no display settings, actions, remaps, pictures or resets.
    ///
    /// Only for sessions whose writes were cleared by the firmware gate.
    Lighting,
}

impl Policy {
    pub fn allows(self, c: &Classification) -> bool {
        match (self, c.class) {
            (_, SafetyClass::Read) => true,
            (Policy::Lighting, SafetyClass::Volatile | SafetyClass::Persist) => {
                c.group == Group::Lighting
            }
            _ => false,
        }
    }
}

/// Timing for [`Session::query`].
#[derive(Clone, Copy, Debug)]
pub struct QueryOptions {
    pub timeout: Duration,
    pub attempts: u32,
}

impl Default for QueryOptions {
    fn default() -> Self {
        QueryOptions {
            timeout: Duration::from_millis(800),
            attempts: 3,
        }
    }
}

/// A conversation with the keyboard's command interface.
///
/// Every packet passes the [`Policy`] allowlist before it is written.
/// Queries are sent one at a time (the keyboard has a single command
/// buffer) and replies are matched on their command bytes (the keyboard
/// broadcasts replies and interleaves events).
pub struct Session<T: Transport> {
    transport: T,
    policy: Policy,
    /// Unrelated packets seen while waiting for replies, for diagnostics.
    skipped: Vec<[u8; PACKET_SIZE]>,
}

impl<T: Transport> Session<T> {
    pub fn new(transport: T, policy: Policy) -> Self {
        Session {
            transport,
            policy,
            skipped: Vec::new(),
        }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// Sends one packet if the policy allows its safety class.
    pub fn send(&mut self, packet: &Packet) -> Result<()> {
        let classification = packet.classification();
        if !self.policy.allows(&classification) {
            return Err(Error::Refused {
                packet: packet.clone(),
                classification,
            });
        }
        self.transport.write_raw(packet.as_bytes())
    }

    /// Sends `packet` and returns the reply that echoes its command bytes.
    pub fn query(&mut self, packet: &Packet, options: QueryOptions) -> Result<[u8; PACKET_SIZE]> {
        let command = packet.command();
        for _ in 0..options.attempts.max(1) {
            self.send(packet)?;
            let deadline = Instant::now() + options.timeout;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                match self.transport.read_raw(left)? {
                    Some(reply) if is_reply_to(&reply, command) => return Ok(reply),
                    Some(other) => self.note_skipped(other),
                    None => break,
                }
            }
        }
        Err(Error::NoReply(command))
    }

    /// Discards whatever is already queued (stale replies, events).
    pub fn drain(&mut self) -> Result<()> {
        while let Some(p) = self.transport.read_raw(Duration::from_millis(30))? {
            self.note_skipped(p);
        }
        Ok(())
    }

    fn note_skipped(&mut self, packet: [u8; PACKET_SIZE]) {
        const KEEP: usize = 64;
        if self.skipped.len() == KEEP {
            self.skipped.remove(0);
        }
        self.skipped.push(packet);
    }

    /// Packets that were not the reply being waited for (events, other
    /// programs' replies), oldest first, at most 64.
    pub fn skipped(&self) -> &[[u8; PACKET_SIZE]] {
        &self.skipped
    }

    pub fn into_transport(self) -> T {
        self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockTransport;

    fn reply(prefix: &[u8]) -> [u8; PACKET_SIZE] {
        let mut p = [0u8; PACKET_SIZE];
        p[..prefix.len()].copy_from_slice(prefix);
        p
    }

    #[test]
    fn matches_reply_and_skips_events() {
        let mut mock = MockTransport::default();
        mock.on(
            [0x11, 0x00],
            vec![
                reply(&[0x01, 0x00]),
                reply(&[0xFF, 0xAA]),
                reply(&[0x11, 0x00, 0, 0, 0x57]),
            ],
        );
        let mut s = Session::new(mock, Policy::ReadOnly);
        let r = s
            .query(&Packet::firmware_info(), QueryOptions::default())
            .unwrap();
        assert_eq!(r[4], 0x57);
        assert_eq!(s.skipped().len(), 2);
        assert_eq!(s.into_transport().written().len(), 1);
    }

    #[test]
    fn retries_then_gives_up() {
        let mock = MockTransport::default();
        let mut s = Session::new(mock, Policy::ReadOnly);
        let opts = QueryOptions {
            timeout: Duration::from_millis(5),
            attempts: 3,
        };
        assert!(matches!(
            s.query(&Packet::layout(), opts),
            Err(Error::NoReply([0x11, 0x12]))
        ));
        assert_eq!(s.into_transport().written().len(), 3);
    }

    fn pad(prefix: &[u8]) -> [u8; PACKET_SIZE] {
        reply(prefix)
    }

    #[test]
    fn read_only_policy() {
        assert!(Policy::ReadOnly.allows(&Packet::firmware_info().classification()));
        for p in [
            pad(&[0x14, 0x00, 0x00, 0x00, 0x01, 0x00]),
            pad(&[0x13, 0x55, 0x00, 0x00, 0x00]),
            pad(&[0x11, 0x14, 0x00, 0x01]),
            pad(&[0xAA, 0x55, 0x21, 0x04]),
            pad(&[0x13, 0x40]),
        ] {
            assert!(!Policy::ReadOnly.allows(&everest_proto::classify(&p)));
        }
    }

    #[test]
    fn lighting_policy_allows_only_lighting() {
        let allowed = [
            pad(&[0x11, 0x00]),
            pad(&[0x14, 0x00, 0x00, 0x00, 0x01, 0x00]),
            pad(&[0x13, 0x55, 0x00, 0x00, 0x06]),
            pad(&[0x14, 0x2C, 0x00, 0x00, 0xFF, 0x64]),
            pad(&[0x14, 0xA0, 0x00, 0x01]),
        ];
        for p in allowed {
            let c = everest_proto::classify(&p);
            assert!(Policy::Lighting.allows(&c), "{}", c.name);
        }
        let refused = [
            pad(&[0x11, 0x14, 0x00, 0x01]),             // settings write
            pad(&[0x11, 0x81, 0x00, 0x00, 0x10]),       // dial metric
            pad(&[0x14, 0x20, 0x04, 0x00, 0x05]),       // key remap
            pad(&[0x12, 0x08, 0x00, 0x01]),             // action slot
            pad(&[0x13, 0x42, 0x00, 0x00, 0x0F]),       // picture reset
            pad(&[0xAA, 0x55, 0x21, 0x04]),             // picture transfer
            pad(&[0x13, 0x40, 0x00, 0x00, 0x01]),       // flash reset
            pad(&[0x14, 0x00, 0x00, 0x00, 0x07, 0x00]), // malformed switch
        ];
        for p in refused {
            let c = everest_proto::classify(&p);
            assert!(!Policy::Lighting.allows(&c), "{}", c.name);
        }
    }
}
