//! Pictures (phase 3): display keys D1..D4 and the dial screen.
//!
//! Byte layouts are described in `docs/PROTOCOL.md`, section "Pictures
//! (phase 3)". The facts come from the MIT-licensed everest-mac project
//! (<https://github.com/Amorem/everest-mac>), which recovered them from
//! Base Camp 1.9.10; this is our own implementation.
//!
//! Nothing here does I/O. [`Picture`] holds an image already converted to
//! the keyboard's format; [`PictureTarget`] says where it goes. The feature
//! packets are built by [`crate::Packet`] constructors, image chunks by
//! [`Picture::chunk`].

use std::fmt;

use crate::lighting::PROFILES;
use crate::PACKET_SIZE;

/// Display key picture: 72 x 72 pixels.
pub const KEY_WIDTH: u32 = 72;
pub const KEY_HEIGHT: u32 = 72;
/// Dial picture: 240 x 204 pixels.
pub const DIAL_WIDTH: u32 = 240;
pub const DIAL_HEIGHT: u32 = 204;
/// Bytes of a display key picture (RGB565).
pub const KEY_BYTES: usize = (KEY_WIDTH * KEY_HEIGHT * 2) as usize;
/// Bytes of a dial picture (RGB565).
pub const DIAL_BYTES: usize = (DIAL_WIDTH * DIAL_HEIGHT * 2) as usize;

/// `aa 55 21 <target>` value for the display keys.
pub const TARGET_KEYS: u8 = 0x04;
/// `aa 55 21 <target>` value for the dial.
pub const TARGET_DIAL: u8 = 0x03;

/// Reply status byte (offset 3) of a feature reply.
pub const STATUS_OK: u8 = 0xFA;
pub const STATUS_BUSY: u8 = 0xFB;
pub const STATUS_REJECTED: u8 = 0xFE;
/// Offset 9 of a chunk reply is this when the picture is complete.
pub const DONE_MARK: u8 = 0xFE;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PictureError {
    #[error("profile {0} is out of range (1..5)")]
    BadProfile(u8),
    #[error("display key {0} is out of range (1..4)")]
    BadKey(u8),
    #[error("a {target} picture is {expected} bytes, got {got}")]
    BadSize {
        target: PictureTarget,
        expected: usize,
        got: usize,
    },
    #[error("no display key selected for the reset")]
    EmptyReset,
}

/// Where a picture goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case", tag = "kind"))]
pub enum PictureTarget {
    /// Display key `key` (0..3 = D1..D4) of `profile` (1..5).
    Key { profile: u8, key: u8 },
    /// The dial screen of the media dock.
    Dial,
}

impl PictureTarget {
    /// Display key `number` (1..4, as printed on the keyboard: D1..D4).
    pub fn key(profile: u8, number: u8) -> Result<Self, PictureError> {
        if !PROFILES.contains(&profile) {
            return Err(PictureError::BadProfile(profile));
        }
        if !(1..=4).contains(&number) {
            return Err(PictureError::BadKey(number));
        }
        Ok(PictureTarget::Key {
            profile,
            key: number - 1,
        })
    }

    pub fn width(self) -> u32 {
        match self {
            PictureTarget::Key { .. } => KEY_WIDTH,
            PictureTarget::Dial => DIAL_WIDTH,
        }
    }

    pub fn height(self) -> u32 {
        match self {
            PictureTarget::Key { .. } => KEY_HEIGHT,
            PictureTarget::Dial => DIAL_HEIGHT,
        }
    }

    /// Size of the picture in bytes (RGB565).
    pub fn byte_len(self) -> usize {
        match self {
            PictureTarget::Key { .. } => KEY_BYTES,
            PictureTarget::Dial => DIAL_BYTES,
        }
    }

    /// The `aa 55 21` target byte.
    pub fn target_byte(self) -> u8 {
        match self {
            PictureTarget::Key { .. } => TARGET_KEYS,
            PictureTarget::Dial => TARGET_DIAL,
        }
    }

    /// Profile and key bytes of the descriptor (dial: profile 1, key 0).
    pub fn descriptor_slot(self) -> (u8, u8) {
        match self {
            PictureTarget::Key { profile, key } => (profile, key),
            PictureTarget::Dial => (1, 0),
        }
    }
}

impl fmt::Display for PictureTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PictureTarget::Key { profile, key } => {
                write!(f, "display key D{} (profile {profile})", key + 1)
            }
            PictureTarget::Dial => write!(f, "dial"),
        }
    }
}

/// One RGB565 pixel from 8-bit RGB, as the two bytes the keyboard expects
/// (little-endian `R5 G6 B5`).
pub fn rgb565(r: u8, g: u8, b: u8) -> [u8; 2] {
    let v = (u16::from(r >> 3) << 11) | (u16::from(g >> 2) << 5) | u16::from(b >> 3);
    v.to_le_bytes()
}

/// The 8-bit RGB value a pixel shows as (for previews).
pub fn rgb565_to_rgb(bytes: [u8; 2]) -> [u8; 3] {
    let v = u16::from_le_bytes(bytes);
    let r = ((v >> 11) & 0x1F) as u8;
    let g = ((v >> 5) & 0x3F) as u8;
    let b = (v & 0x1F) as u8;
    // Replicate the high bits so full scale maps to 255.
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// Picture checksum: the sum of all bytes, modulo 65536.
pub fn checksum(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0u16, |acc, &b| acc.wrapping_add(u16::from(b)))
}

/// A picture in the keyboard's format, checked against its target size.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    target: PictureTarget,
    bytes: Vec<u8>,
}

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Picture({}, {} bytes, checksum {:04x})",
            self.target,
            self.bytes.len(),
            self.checksum()
        )
    }
}

impl Picture {
    /// Wraps RGB565 bytes. The length must be exactly the target's.
    pub fn from_rgb565(target: PictureTarget, bytes: Vec<u8>) -> Result<Self, PictureError> {
        if bytes.len() != target.byte_len() {
            return Err(PictureError::BadSize {
                target,
                expected: target.byte_len(),
                got: bytes.len(),
            });
        }
        Ok(Picture { target, bytes })
    }

    /// Converts 8-bit RGB pixels (row-major, top row first, `width * height
    /// * 3` bytes at the target size).
    pub fn from_rgb8(target: PictureTarget, rgb: &[u8]) -> Result<Self, PictureError> {
        let pixels = (target.width() * target.height()) as usize;
        if rgb.len() != pixels * 3 {
            return Err(PictureError::BadSize {
                target,
                expected: pixels * 3,
                got: rgb.len(),
            });
        }
        let bytes = rgb
            .chunks_exact(3)
            .flat_map(|p| rgb565(p[0], p[1], p[2]))
            .collect();
        Self::from_rgb565(target, bytes)
    }

    pub fn target(&self) -> PictureTarget {
        self.target
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn checksum(&self) -> u16 {
        checksum(&self.bytes)
    }

    /// Number of 64-byte chunks.
    pub fn chunk_count(&self) -> usize {
        self.bytes.len().div_ceil(PACKET_SIZE)
    }

    /// The chunk that starts at byte `offset` (zero-padded at the end).
    /// `None` when `offset` is at or past the end.
    pub fn chunk_at(&self, offset: usize) -> Option<ImageChunk> {
        if offset >= self.bytes.len() {
            return None;
        }
        let end = (offset + PACKET_SIZE).min(self.bytes.len());
        let mut out = [0u8; PACKET_SIZE];
        out[..end - offset].copy_from_slice(&self.bytes[offset..end]);
        Some(ImageChunk { offset, bytes: out })
    }

    /// 8-bit RGB of the picture as the keyboard will show it.
    pub fn to_rgb8(&self) -> Vec<u8> {
        self.bytes
            .chunks_exact(2)
            .flat_map(|p| rgb565_to_rgb([p[0], p[1]]))
            .collect()
    }
}

/// 64 image bytes for one feature report during a transfer.
///
/// Chunks cannot be told apart from other data by content, so they have
/// their own type: the session only sends one while a transfer is armed.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageChunk {
    offset: usize,
    bytes: [u8; PACKET_SIZE],
}

impl ImageChunk {
    /// Byte offset of this chunk in the picture.
    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn as_bytes(&self) -> &[u8; PACKET_SIZE] {
        &self.bytes
    }
}

impl fmt::Debug for ImageChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImageChunk(offset {})", self.offset)
    }
}

/// Status byte of a feature reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum TransferStatus {
    Accepted,
    Busy,
    Rejected,
    Other(u8),
}

impl TransferStatus {
    pub fn from_byte(b: u8) -> Self {
        match b {
            STATUS_OK => Self::Accepted,
            STATUS_BUSY => Self::Busy,
            STATUS_REJECTED => Self::Rejected,
            other => Self::Other(other),
        }
    }
}

/// A parsed feature reply (`aa 55 <command> <status> ...`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct TransferReply {
    /// Echoed command byte: `21`, `22` or `10`.
    pub command: u8,
    pub status: TransferStatus,
    /// Bytes 4..6, little-endian: bytes received so far (chunk replies).
    pub received: usize,
    /// Byte 9 is `fe`: the picture is complete.
    pub complete: bool,
}

impl TransferReply {
    /// `None` unless the reply starts with `aa 55`.
    pub fn parse(reply: &[u8]) -> Option<Self> {
        if reply.len() < 10 || reply[0] != 0xAA || reply[1] != 0x55 {
            return None;
        }
        Some(TransferReply {
            command: reply[2],
            status: TransferStatus::from_byte(reply[3]),
            received: usize::from(reply[4])
                | (usize::from(reply[5]) << 8)
                | (usize::from(reply[6]) << 16),
            complete: reply[9] == DONE_MARK,
        })
    }

    /// The keyboard accepted `command` (`fa`).
    pub fn accepted(&self, command: u8) -> bool {
        self.command == command && self.status == TransferStatus::Accepted
    }
}

/// Key bitmap per profile for `13 42` (picture reset of the display keys).
///
/// `keys[p]` is the bitmap for profile `p + 1`: bit 0 = D1 ... bit 3 = D4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct KeyResetMask {
    pub keys: [u8; 5],
}

impl KeyResetMask {
    /// Display keys `numbers` (1..4) of `profile` (1..5).
    pub fn for_profile(profile: u8, numbers: &[u8]) -> Result<Self, PictureError> {
        if !PROFILES.contains(&profile) {
            return Err(PictureError::BadProfile(profile));
        }
        let mut bitmap = 0u8;
        for &n in numbers {
            if !(1..=4).contains(&n) {
                return Err(PictureError::BadKey(n));
            }
            bitmap |= 1 << (n - 1);
        }
        if bitmap == 0 {
            return Err(PictureError::EmptyReset);
        }
        let mut keys = [0u8; 5];
        keys[usize::from(profile - 1)] = bitmap;
        Ok(KeyResetMask { keys })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_format() {
        assert_eq!(rgb565(255, 0, 0), [0x00, 0xF8]);
        assert_eq!(rgb565(0, 255, 0), [0xE0, 0x07]);
        assert_eq!(rgb565(0, 0, 255), [0x1F, 0x00]);
        assert_eq!(rgb565(255, 255, 255), [0xFF, 0xFF]);
        assert_eq!(rgb565(0, 0, 0), [0x00, 0x00]);
        for c in [[255, 255, 255], [0, 0, 0], [255, 0, 0], [0, 255, 0]] {
            assert_eq!(rgb565_to_rgb(rgb565(c[0], c[1], c[2])), c);
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(KEY_BYTES, 10_368);
        assert_eq!(DIAL_BYTES, 97_920);
        let t = PictureTarget::key(1, 1).unwrap();
        let p = Picture::from_rgb565(t, vec![0; KEY_BYTES]).unwrap();
        assert_eq!(p.chunk_count(), 162);
        let d = Picture::from_rgb565(PictureTarget::Dial, vec![0; DIAL_BYTES]).unwrap();
        assert_eq!(d.chunk_count(), 1530);
        assert!(Picture::from_rgb565(t, vec![0; KEY_BYTES - 1]).is_err());
        assert!(Picture::from_rgb565(PictureTarget::Dial, vec![0; KEY_BYTES]).is_err());
    }

    #[test]
    fn target_validation() {
        assert_eq!(
            PictureTarget::key(3, 4).unwrap(),
            PictureTarget::Key { profile: 3, key: 3 }
        );
        assert_eq!(PictureTarget::key(0, 1), Err(PictureError::BadProfile(0)));
        assert_eq!(PictureTarget::key(6, 1), Err(PictureError::BadProfile(6)));
        assert_eq!(PictureTarget::key(1, 0), Err(PictureError::BadKey(0)));
        assert_eq!(PictureTarget::key(1, 5), Err(PictureError::BadKey(5)));
        assert_eq!(PictureTarget::Dial.descriptor_slot(), (1, 0));
    }

    #[test]
    fn checksum_wraps() {
        assert_eq!(checksum(&[0xFF; 257]), 0xFF * 257); // below 65536: no wrap
        assert_eq!(checksum(&[0xFF; 300]), (0xFFu32 * 300 - 65_536) as u16);
        assert_eq!(checksum(&[0xFF; 10_368]), (255u32 * 10_368 % 65_536) as u16);
    }

    #[test]
    fn chunks_cover_the_picture() {
        let t = PictureTarget::key(1, 2).unwrap();
        let bytes: Vec<u8> = (0..KEY_BYTES).map(|i| i as u8).collect();
        let p = Picture::from_rgb565(t, bytes.clone()).unwrap();
        let mut joined = Vec::new();
        let mut off = 0;
        while let Some(c) = p.chunk_at(off) {
            assert_eq!(c.offset(), off);
            joined.extend_from_slice(c.as_bytes());
            off += PACKET_SIZE;
        }
        assert_eq!(&joined[..KEY_BYTES], &bytes[..]);
        assert_eq!(joined.len(), KEY_BYTES); // 162 * 64 exactly, no padding
        assert!(p.chunk_at(KEY_BYTES).is_none());
    }

    #[test]
    fn rgb8_conversion() {
        let t = PictureTarget::key(1, 1).unwrap();
        let mut rgb = vec![0u8; 72 * 72 * 3];
        rgb[..3].copy_from_slice(&[255, 0, 0]);
        let p = Picture::from_rgb8(t, &rgb).unwrap();
        assert_eq!(&p.bytes()[..4], &[0x00, 0xF8, 0x00, 0x00]);
        assert_eq!(&p.to_rgb8()[..3], &[255, 0, 0]);
        assert!(Picture::from_rgb8(t, &rgb[..30]).is_err());
    }

    #[test]
    fn reply_parsing() {
        let mut r = [0u8; 64];
        r[..10].copy_from_slice(&[0xAA, 0x55, 0x10, 0xFA, 0x80, 0x28, 0x00, 0, 0, 0xFE]);
        let p = TransferReply::parse(&r).unwrap();
        assert!(p.accepted(0x10));
        assert_eq!(p.received, 10_368);
        assert!(p.complete);
        r[3] = 0xFB;
        assert_eq!(
            TransferReply::parse(&r).unwrap().status,
            TransferStatus::Busy
        );
        assert!(TransferReply::parse(&[0x11, 0x14, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
        assert!(TransferReply::parse(&[0xAA, 0x55]).is_none());
    }

    #[test]
    fn reset_mask() {
        assert_eq!(
            KeyResetMask::for_profile(2, &[1, 4]).unwrap().keys,
            [0, 0x09, 0, 0, 0]
        );
        assert_eq!(
            KeyResetMask::for_profile(1, &[]),
            Err(PictureError::EmptyReset)
        );
        assert_eq!(
            KeyResetMask::for_profile(1, &[5]),
            Err(PictureError::BadKey(5))
        );
        assert_eq!(
            KeyResetMask::for_profile(6, &[1]),
            Err(PictureError::BadProfile(6))
        );
    }
}
