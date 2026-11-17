use std::fmt;

use crate::classify::{classify, Classification};
use crate::lighting::{
    put_triplets, CustomFrame, EffectSettings, LightingError, Rgb, KEYS_PER_PACKET, PROFILES,
    SIDE_PACKET_SIZES,
};
use crate::pictures::{KeyResetMask, Picture};

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

    /// `14 00 00 00 <profile> <slot>`: switch the active profile and
    /// lighting slot (Base Camp's `SwitchProfile`).
    pub fn switch_slot(profile: u8, slot: u8) -> Result<Self, LightingError> {
        if !PROFILES.contains(&profile) {
            return Err(LightingError::BadProfile(profile));
        }
        assert!(slot <= 8, "lighting slot {slot} out of range");
        Ok(Self::from_prefix(&[0x14, 0x00, 0x00, 0x00, profile, slot]))
    }

    /// `14 2C <effect> 00 ...`: parameters of a built-in effect for the
    /// current slot. Kept until replug only; `13 55` saves it.
    pub fn effect(settings: &EffectSettings) -> Self {
        Packet(settings.encode())
    }

    /// `13 55 00 00 <slot>`: save the slot to flash (`SaveFlash`).
    pub fn save_slot(slot: u8) -> Self {
        assert!(slot <= 8, "lighting slot {slot} out of range");
        Self::from_prefix(&[0x13, 0x55, 0x00, 0x00, slot])
    }

    /// `14 2C 0A 00 FF <brightness> 00`: enter custom (per-key) mode.
    pub fn enter_custom_mode(brightness: u8) -> Result<Self, LightingError> {
        if brightness > 100 {
            return Err(LightingError::BadBrightness(brightness));
        }
        Ok(Self::from_prefix(&[
            0x14, 0x2C, 0x0A, 0x00, 0xFF, brightness, 0x00,
        ]))
    }

    /// `14 2C 00 01 <ix> <brightness> 00 <19 x RGB>`: key colours, packet
    /// `ix` (0..8) of a custom frame.
    pub fn key_colors(ix: usize, brightness: u8, colors: &[Rgb]) -> Result<Self, LightingError> {
        if brightness > 100 {
            return Err(LightingError::BadBrightness(brightness));
        }
        assert!(ix < 8 && colors.len() == KEYS_PER_PACKET);
        let mut p = Self::from_prefix(&[0x14, 0x2C, 0x00, 0x01, ix as u8, brightness, 0x00]);
        put_triplets(&mut p.0, colors);
        Ok(p)
    }

    /// `14 2D 0A 00 <ix> FF 00 <RGB...>`: side-strip colours, packet `ix`
    /// (0..3) of a custom frame.
    pub fn side_colors(ix: usize, colors: &[Rgb]) -> Self {
        assert!(ix < 3 && colors.len() == SIDE_PACKET_SIZES[ix]);
        let mut p = Self::from_prefix(&[0x14, 0x2D, 0x0A, 0x00, ix as u8, 0xFF, 0x00]);
        put_triplets(&mut p.0, colors);
        p
    }

    /// `14 A0 <chunk> 01`: bind keys to the static slot after a custom
    /// frame, chunk 0..3 (exact meaning unknown, see `docs/PROTOCOL.md`).
    pub fn bind_keys(chunk: u8) -> Self {
        assert!(chunk < 3);
        Self::from_prefix(&[0x14, 0xA0, chunk, 0x01])
    }

    /// The 11 packets of a custom frame: 8 key packets, then 3 side packets.
    pub fn custom_frame(frame: &CustomFrame, brightness: u8) -> Result<Vec<Self>, LightingError> {
        let mut out = Vec::with_capacity(11);
        for ix in 0..8 {
            out.push(Self::key_colors(ix, brightness, frame.key_chunk(ix))?);
        }
        for ix in 0..3 {
            out.push(Self::side_colors(ix, frame.side_chunk(ix)));
        }
        Ok(out)
    }

    /// Feature `aa 55 21 <target>`: select the picture target (`04`
    /// display keys, `03` dial). Selects only; erases nothing.
    pub fn picture_select_target(target: crate::PictureTarget) -> Self {
        Self::from_prefix(&[0xAA, 0x55, 0x21, target.target_byte()])
    }

    /// Feature `aa 55 22 00`: confirm the selected picture target.
    pub fn picture_confirm_target() -> Self {
        Self::from_prefix(&[0xAA, 0x55, 0x22, 0x00])
    }

    /// Feature `aa 55 10 <size:3 LE> <checksum:2 LE> 00 00 02 <profile>
    /// <key>`: announce a picture. Once the keyboard accepts it, the
    /// picture's chunks must follow until it reports done.
    pub fn picture_descriptor(picture: &Picture) -> Self {
        let size = picture.bytes().len();
        let [c0, c1] = picture.checksum().to_le_bytes();
        let (profile, key) = picture.target().descriptor_slot();
        Self::from_prefix(&[
            0xAA,
            0x55,
            0x10,
            size as u8,
            (size >> 8) as u8,
            (size >> 16) as u8,
            c0,
            c1,
            0x00,
            0x00,
            0x02,
            profile,
            key,
        ])
    }

    /// `13 41 00 00 01`: dial picture back to the factory logo.
    pub fn reset_dial_picture() -> Self {
        Self::from_prefix(&[0x13, 0x41, 0x00, 0x00, 0x01])
    }

    /// `13 42 00 00 <p1..p5>`: display-key pictures back to factory.
    pub fn reset_key_pictures(mask: &KeyResetMask) -> Self {
        let mut p = Self::from_prefix(&[0x13, 0x42, 0x00, 0x00]);
        p.0[4..9].copy_from_slice(&mask.keys);
        p
    }

    /// `14 20 <key> 00 <new key>`: make the key at position `key` act as
    /// the key at position `to`, in the active profile (phase 4, [P]).
    /// `remap(k, k)` is believed to undo a remap.
    pub fn remap(key: crate::KeyPos, to: crate::KeyPos) -> Self {
        Self::from_prefix(&[0x14, 0x20, key.code(), 0x00, to.code()])
    }

    /// `12 08 00 <1..4>`: select the action slot of a display key. Must be
    /// followed by [`Packet::action_text`].
    pub fn action_slot(key: crate::DKey) -> Self {
        Self::from_prefix(&[0x12, 0x08, 0x00, key.number()])
    }

    /// `17 AA <len> 00 <kind> <text>`: the text for the selected action
    /// slot; `len` = 1 + text length.
    pub fn action_text(kind: crate::StoredKind, text: &crate::ActionText) -> Self {
        let t = text.as_str().as_bytes();
        let mut p = Self::from_prefix(&[0x17, 0xAA, (t.len() + 1) as u8, 0x00, kind.byte()]);
        p.0[5..5 + t.len()].copy_from_slice(t);
        p
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
    fn lighting_builders_pass_the_classifier() {
        use crate::classify::Group;
        use crate::lighting::{Effect, EffectSettings, Rgb};

        let mut packets = vec![
            Packet::switch_slot(1, 0).unwrap(),
            Packet::switch_slot(5, 6).unwrap(),
            Packet::save_slot(6),
            Packet::enter_custom_mode(100).unwrap(),
            Packet::bind_keys(0),
            Packet::bind_keys(2),
        ];
        for effect in Effect::ALL {
            let colors = vec![Rgb::WHITE; effect.modes()[0].colors()];
            let s = EffectSettings::new(effect, None, colors, None, 100, None).unwrap();
            packets.push(Packet::effect(&s));
        }
        let frame = CustomFrame::filled(Rgb::WHITE, Rgb::new(1, 2, 3));
        packets.extend(Packet::custom_frame(&frame, 100).unwrap());
        for p in packets {
            let c = p.classification();
            assert_eq!(c.group, Group::Lighting, "{p:?} {}", c.name);
            assert_ne!(c.class, SafetyClass::Deny, "{p:?}");
        }
    }

    #[test]
    fn lighting_builders_reject_bad_input() {
        assert!(Packet::switch_slot(0, 0).is_err());
        assert!(Packet::switch_slot(6, 0).is_err());
        assert!(Packet::enter_custom_mode(101).is_err());
        assert_eq!(
            format!("{:?}", Packet::switch_slot(2, 6).unwrap()),
            "Packet[14 00 00 00 02 06]"
        );
    }

    #[test]
    fn custom_frame_layout() {
        use crate::lighting::Rgb;
        let mut frame = CustomFrame::filled(Rgb::BLACK, Rgb::BLACK);
        frame.keys[0] = Rgb::new(0xAA, 0xBB, 0xCC); // Esc
        frame.keys[20] = Rgb::new(1, 2, 3); // W: packet 1, position 1
        frame.side[44] = Rgb::new(4, 5, 6); // last side LED: packet 2, position 6
        let p = Packet::custom_frame(&frame, 80).unwrap();
        assert_eq!(p.len(), 11);
        assert_eq!(
            format!("{:?}", p[0]),
            "Packet[14 2c 00 01 00 50 00 aa bb cc]"
        );
        assert_eq!(
            format!("{:?}", p[1]),
            "Packet[14 2c 00 01 01 50 00 00 00 00 01 02 03]"
        );
        assert_eq!(p[10].as_bytes()[7 + 18..7 + 21], [4, 5, 6]);
        assert_eq!(format!("{:?}", p[8]), "Packet[14 2d 0a 00 00 ff]");
    }

    #[test]
    fn picture_builders_pass_the_classifier() {
        use crate::classify::Group;
        use crate::pictures::{PictureTarget, DIAL_BYTES, KEY_BYTES};

        let key =
            Picture::from_rgb565(PictureTarget::key(2, 4).unwrap(), vec![1; KEY_BYTES]).unwrap();
        let dial = Picture::from_rgb565(PictureTarget::Dial, vec![0xFF; DIAL_BYTES]).unwrap();
        assert_eq!(
            format!("{:?}", Packet::picture_descriptor(&key)),
            "Packet[aa 55 10 80 28 00 80 28 00 00 02 02 03]"
        );
        let sum = (0xFFu32 * DIAL_BYTES as u32 % 65_536) as u16;
        let [lo, hi] = sum.to_le_bytes();
        assert_eq!(
            Packet::picture_descriptor(&dial).as_bytes()[..13],
            [0xAA, 0x55, 0x10, 0x80, 0x7E, 0x01, lo, hi, 0, 0, 0x02, 0x01, 0x00]
        );
        let mask = KeyResetMask::for_profile(1, &[1, 2, 3, 4]).unwrap();
        for p in [
            Packet::picture_select_target(key.target()),
            Packet::picture_select_target(PictureTarget::Dial),
            Packet::picture_confirm_target(),
            Packet::picture_descriptor(&key),
            Packet::picture_descriptor(&dial),
            Packet::reset_dial_picture(),
            Packet::reset_key_pictures(&mask),
        ] {
            let c = p.classification();
            assert_eq!(c.group, Group::Pictures, "{p:?} {}", c.name);
            assert_ne!(c.class, SafetyClass::Deny, "{p:?}");
        }
        assert_eq!(
            format!("{:?}", Packet::reset_key_pictures(&mask)),
            "Packet[13 42 00 00 0f]"
        );
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
