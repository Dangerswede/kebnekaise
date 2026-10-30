use std::fmt;

use crate::classify::{classify, Classification};

/// Size of every packet on the vendor interface, without a report ID.
pub const PACKET_SIZE: usize = 64;

/// One outgoing 64-byte packet for the interrupt channel.
///
/// Packets can only be built through the constructors in this crate, so
/// every byte sequence that reaches the device is one `docs/PROTOCOL.md`
/// describes.
#[derive(Clone, PartialEq, Eq)]
pub struct Packet([u8; PACKET_SIZE]);

impl Packet {
    fn from_prefix(prefix: &[u8]) -> Self {
        let mut bytes = [0u8; PACKET_SIZE];
        bytes[..prefix.len()].copy_from_slice(prefix);
        Packet(bytes)
    }

    /// `11 00`: firmware info (version, active profile, lighting slot).
    pub fn firmware_info() -> Self {
        Self::from_prefix(&[0x11, 0x00])
    }

    /// `11 12`: layout code. Also the first packet of a session.
    pub fn layout() -> Self {
        Self::from_prefix(&[0x11, 0x12])
    }

    /// `11 14` (read form): extended info (attached modules, dial settings).
    pub fn extended_info() -> Self {
        Self::from_prefix(&[0x11, 0x14])
    }

    /// The command family and command, bytes 0..2. Replies echo them.
    pub fn command(&self) -> [u8; 2] {
        [self.0[0], self.0[1]]
    }

    pub fn as_bytes(&self) -> &[u8; PACKET_SIZE] {
        &self.0
    }

    pub fn classification(&self) -> Classification {
        classify(&self.0)
    }
}

impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Trailing zeros are noise in logs; print up to the last non-zero byte.
        let end = self
            .0
            .iter()
            .rposition(|&b| b != 0)
            .map_or(2, |i| i + 1)
            .max(2);
        write!(f, "Packet[")?;
        for (i, b) in self.0[..end].iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{b:02x}")?;
        }
        write!(f, "]")
    }
}

/// True when `reply` is the keyboard's answer to `command`.
///
/// The keyboard broadcasts every answer to every program that has the
/// interface open, and interleaves key events (`01 ..`) and notifications
/// (`ff aa`), so replies must be matched on their first two bytes.
pub fn is_reply_to(reply: &[u8], command: [u8; 2]) -> bool {
    reply.len() >= 2 && reply[0] == command[0] && reply[1] == command[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SafetyClass;

    #[test]
    fn read_packets_are_zero_padded() {
        let p = Packet::firmware_info();
        assert_eq!(p.as_bytes()[..2], [0x11, 0x00]);
        assert!(p.as_bytes()[2..].iter().all(|&b| b == 0));
        assert_eq!(Packet::layout().command(), [0x11, 0x12]);
        assert_eq!(Packet::extended_info().command(), [0x11, 0x14]);
    }

    #[test]
    fn every_builder_is_classified_read() {
        for p in [
            Packet::firmware_info(),
            Packet::layout(),
            Packet::extended_info(),
        ] {
            assert_eq!(p.classification().class, SafetyClass::Read, "{p:?}");
        }
    }

    #[test]
    fn reply_matching() {
        assert!(is_reply_to(&[0x11, 0x00, 0x00], [0x11, 0x00]));
        assert!(!is_reply_to(&[0x11, 0x14], [0x11, 0x00]));
        assert!(!is_reply_to(&[0x01, 0x00], [0x11, 0x00]));
        assert!(!is_reply_to(&[0x11], [0x11, 0x00]));
    }

    #[test]
    fn debug_trims_trailing_zeros() {
        assert_eq!(format!("{:?}", Packet::layout()), "Packet[11 12]");
    }
}
