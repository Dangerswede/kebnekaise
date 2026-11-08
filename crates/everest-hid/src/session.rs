use std::time::{Duration, Instant};

use everest_proto::{
    is_reply_to, Classification, Group, ImageChunk, Packet, SafetyClass, TransferReply, PACKET_SIZE,
};

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
    /// Phase 3: queries plus the picture group: the picture transfer
    /// (`aa 55 21/22/10` feature reports and image chunks) and the picture
    /// resets (`13 41`, `13 42`). Nothing else.
    ///
    /// Only for sessions whose writes were cleared by the firmware gate.
    Pictures,
}

impl Policy {
    pub fn allows(self, c: &Classification) -> bool {
        match (self, c.class) {
            (_, SafetyClass::Read) => true,
            (Policy::Lighting, SafetyClass::Volatile | SafetyClass::Persist) => {
                c.group == Group::Lighting
            }
            (Policy::Pictures, SafetyClass::Persist | SafetyClass::Transfer) => {
                c.group == Group::Pictures
            }
            _ => false,
        }
    }
}

/// Which channel a packet is meant for. Picture-transfer packets only go
/// over feature reports, everything else only over the interrupt channel.
fn is_feature_packet(c: &Classification) -> bool {
    c.class == SafetyClass::Transfer
}

/// State of an armed picture transfer: the keyboard accepted a descriptor
/// and now expects `total` bytes of chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmedTransfer {
    pub total: usize,
    /// Bytes the keyboard has acknowledged so far.
    pub received: usize,
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
    /// Set while a picture transfer is in progress.
    transfer: Option<ArmedTransfer>,
}

impl<T: Transport> Session<T> {
    pub fn new(transport: T, policy: Policy) -> Self {
        Session {
            transport,
            policy,
            skipped: Vec::new(),
            transfer: None,
        }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    fn check(&self, packet: &Packet, feature: bool) -> Result<()> {
        let classification = packet.classification();
        if !self.policy.allows(&classification) {
            return Err(Error::Refused {
                packet: packet.clone(),
                classification,
            });
        }
        if is_feature_packet(&classification) != feature {
            return Err(Error::WrongChannel {
                packet: packet.clone(),
                classification,
            });
        }
        if self.transfer.is_some() {
            return Err(Error::TransferInProgress);
        }
        Ok(())
    }

    /// Sends one packet on the interrupt channel if the policy allows its
    /// safety class. Refused while a picture transfer is armed.
    pub fn send(&mut self, packet: &Packet) -> Result<()> {
        self.check(packet, false)?;
        self.transport.write_raw(packet.as_bytes())
    }

    /// Sends one picture-transfer packet as a feature report, waits `wait`,
    /// and reads the feature reply. A descriptor the keyboard accepts arms
    /// the transfer: from then on only [`Session::send_chunk`] and
    /// [`Session::poll_feature`] may be used until the keyboard has
    /// acknowledged every byte.
    pub fn feature_exchange(
        &mut self,
        packet: &Packet,
        wait: Duration,
    ) -> Result<Option<TransferReply>> {
        self.check(packet, true)?;
        self.transport.send_feature_raw(packet.as_bytes())?;
        std::thread::sleep(wait);
        let reply = self.poll_feature()?;
        let b = packet.as_bytes();
        if b[2] == 0x10 && reply.is_some_and(|r| r.accepted(0x10)) {
            let total = usize::from(b[3]) | usize::from(b[4]) << 8 | usize::from(b[5]) << 16;
            self.transfer = Some(ArmedTransfer { total, received: 0 });
        }
        Ok(reply)
    }

    /// Sends one image chunk. Only while a transfer is armed, and only the
    /// chunk the keyboard asked for (at its acknowledged byte count).
    /// Disarms once the keyboard has acknowledged the whole picture.
    pub fn send_chunk(
        &mut self,
        chunk: &ImageChunk,
        wait: Duration,
    ) -> Result<Option<TransferReply>> {
        let Some(armed) = self.transfer else {
            return Err(Error::NotArmed);
        };
        if chunk.offset() != armed.received || chunk.offset() >= armed.total {
            return Err(Error::NotArmed);
        }
        self.transport.send_feature_raw(chunk.as_bytes())?;
        std::thread::sleep(wait);
        self.poll_feature()
    }

    /// Reads the feature reply without sending anything (GET_REPORT).
    /// Tracks the acknowledged byte count of an armed transfer.
    pub fn poll_feature(&mut self) -> Result<Option<TransferReply>> {
        let reply = self
            .transport
            .get_feature_raw()?
            .and_then(|r| TransferReply::parse(&r));
        if let (Some(armed), Some(r)) = (self.transfer.as_mut(), reply) {
            if r.accepted(0x10) && r.received > armed.received {
                armed.received = r.received.min(armed.total);
            }
            // Only the byte count ends a transfer: a stray `fe`/`fa` in other
            // bytes of a mid-stream answer must never end it early.
            if armed.received >= armed.total {
                self.transfer = None;
            }
        }
        Ok(reply)
    }

    /// The armed transfer, if one is in progress.
    pub fn transfer(&self) -> Option<ArmedTransfer> {
        self.transfer
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

    /// Reads the interrupt channel until a packet starting with `prefix`
    /// arrives (true) or `timeout` passes (false). Other packets are kept
    /// in [`Session::skipped`].
    pub fn wait_for(&mut self, prefix: &[u8], timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self
                .transport
                .read_raw(left.max(Duration::from_millis(1)))?
            {
                Some(p) if p.starts_with(prefix) => return Ok(true),
                Some(p) => self.note_skipped(p),
                None => {}
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
        }
    }

    /// Returns every packet that arrives on the interrupt channel within
    /// `duration` (for commands without a documented reply).
    pub fn listen(&mut self, duration: Duration) -> Result<Vec<[u8; PACKET_SIZE]>> {
        let deadline = Instant::now() + duration;
        let mut out = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(out);
            }
            if let Some(p) = self.transport.read_raw(left)? {
                out.push(p);
            }
        }
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
    fn pictures_policy_allows_only_pictures() {
        let allowed = [
            pad(&[0x11, 0x00]),
            pad(&[0xAA, 0x55, 0x21, 0x04]),
            pad(&[0xAA, 0x55, 0x22, 0x00]),
            pad(&[0x13, 0x41, 0x00, 0x00, 0x01]),
            pad(&[0x13, 0x42, 0x00, 0x00, 0x0F]),
        ];
        for p in allowed {
            let c = everest_proto::classify(&p);
            assert!(Policy::Pictures.allows(&c), "{}", c.name);
        }
        let refused = [
            pad(&[0x14, 0x00, 0x00, 0x00, 0x01, 0x00]), // lighting
            pad(&[0x11, 0x14, 0x00, 0x01]),             // settings write
            pad(&[0x12, 0x08, 0x00, 0x01]),             // action slot
            pad(&[0xAA, 0x55, 0x7F]),                   // device reset
            pad(&[0xAA, 0x55, 0x21, 0x05]),             // other target
            pad(&[0x13, 0x40, 0x00, 0x00, 0x01]),       // flash reset
        ];
        for p in refused {
            let c = everest_proto::classify(&p);
            assert!(!Policy::Pictures.allows(&c), "{}", c.name);
        }
        // The lighting policy does not allow pictures.
        let c = everest_proto::classify(&pad(&[0xAA, 0x55, 0x21, 0x04]));
        assert!(!Policy::Lighting.allows(&c));
    }

    #[test]
    fn channels_are_enforced() {
        use everest_proto::{Picture, PictureTarget};
        let mut s = Session::new(MockTransport::default(), Policy::Pictures);
        let target = PictureTarget::key(1, 1).unwrap();
        // A transfer packet on the interrupt channel is refused...
        assert!(matches!(
            s.send(&Packet::picture_select_target(target)),
            Err(Error::WrongChannel { .. })
        ));
        // ...and a reset as a feature report.
        assert!(matches!(
            s.feature_exchange(&Packet::reset_dial_picture(), Duration::ZERO),
            Err(Error::WrongChannel { .. })
        ));
        // No chunk without an accepted descriptor.
        let pic =
            Picture::from_rgb565(target, vec![0; everest_proto::pictures::KEY_BYTES]).unwrap();
        assert!(matches!(
            s.send_chunk(&pic.chunk_at(0).unwrap(), Duration::ZERO),
            Err(Error::NotArmed)
        ));
        assert!(s.into_transport().written().is_empty());
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
