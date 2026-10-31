use std::fmt;

use crate::layout::Layout;
use crate::packet::is_reply_to;
use crate::TESTED_FIRMWARE;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The packet is not the reply to the expected command.
    WrongReply { expected: [u8; 2] },
    /// The reply is shorter than the fields we read from it.
    TooShort { needed: usize, got: usize },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::WrongReply { expected } => {
                write!(f, "not a reply to {:02x} {:02x}", expected[0], expected[1])
            }
            ParseError::TooShort { needed, got } => {
                write!(f, "reply too short: need {needed} bytes, got {got}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

fn check(reply: &[u8], command: [u8; 2], needed: usize) -> Result<(), ParseError> {
    if !is_reply_to(reply, command) {
        return Err(ParseError::WrongReply { expected: command });
    }
    if reply.len() < needed {
        return Err(ParseError::TooShort {
            needed,
            got: reply.len(),
        });
    }
    Ok(())
}

/// Firmware version byte, BCD-like: `0x57` is "firmware 57".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FirmwareVersion(pub u8);

impl FirmwareVersion {
    /// Whether writing commands have been tested on this version.
    pub fn is_tested(self) -> bool {
        TESTED_FIRMWARE.contains(&self.0)
    }
}

impl fmt::Display for FirmwareVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:x}", self.0)
    }
}

/// The lighting slot of a profile. Each built-in effect has its own slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum LightingSlot {
    Static,
    Wave,
    Tornado,
    Breathing,
    Reactive,
    Matrix,
    Custom,
    Yeti,
    Off,
    Unknown(u8),
}

impl LightingSlot {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Static,
            1 => Self::Wave,
            2 => Self::Tornado,
            3 => Self::Breathing,
            4 => Self::Reactive,
            5 => Self::Matrix,
            6 => Self::Custom,
            7 => Self::Yeti,
            8 => Self::Off,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for LightingSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(b) => write!(f, "unknown (0x{b:02x})"),
            other => write!(f, "{}", format!("{other:?}").to_lowercase()),
        }
    }
}

/// Reply to `11 00`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FirmwareInfo {
    pub version: FirmwareVersion,
    /// Active profile 1..=5, `None` if the byte is out of range.
    pub active_profile: Option<u8>,
    pub lighting_slot: LightingSlot,
}

impl FirmwareInfo {
    pub fn parse(reply: &[u8]) -> Result<Self, ParseError> {
        check(reply, [0x11, 0x00], 12)?;
        Ok(FirmwareInfo {
            version: FirmwareVersion(reply[4]),
            active_profile: (1..=5).contains(&reply[10]).then_some(reply[10]),
            lighting_slot: LightingSlot::from_byte(reply[11]),
        })
    }
}

/// Reply to `11 12`: the raw layout code and what Base Camp makes of it.
pub fn parse_layout(reply: &[u8]) -> Result<(u8, Layout), ParseError> {
    check(reply, [0x11, 0x12], 5)?;
    Ok((reply[4], Layout::from_code(reply[4])))
}

/// What the dial screen shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum DialMode {
    Image,
    Clock,
    Volume,
    Cpu,
    Gpu,
    Disk,
    Network,
    Ram,
    Apm,
    Unknown(u8),
}

impl DialMode {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x01 => Self::Image,
            0x11 => Self::Clock,
            0x71 => Self::Volume,
            0x91 => Self::Cpu,
            0xA1 => Self::Gpu,
            0xB1 => Self::Disk,
            0xC1 => Self::Network,
            0xD1 => Self::Ram,
            0xE1 => Self::Apm,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for DialMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(b) => write!(f, "unknown (0x{b:02x})"),
            Self::Cpu | Self::Gpu | Self::Ram | Self::Apm => {
                write!(f, "{}", format!("{self:?}").to_uppercase())
            }
            other => write!(f, "{}", format!("{other:?}").to_lowercase()),
        }
    }
}

/// Display brightness for one lighting slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum DisplayBrightness {
    FollowLighting,
    Fixed(u8),
}

impl DisplayBrightness {
    pub fn from_byte(b: u8) -> Self {
        if b & 0x80 != 0 {
            Self::Fixed(b & 0x7F)
        } else {
            Self::FollowLighting
        }
    }
}

/// Which detachable modules are attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ModuleSet {
    pub numpad: bool,
    pub media_dock: bool,
}

impl ModuleSet {
    pub fn describe(self) -> &'static str {
        match (self.numpad, self.media_dock) {
            (true, true) => "numpad and media dock attached (complete Everest Max)",
            (true, false) => "numpad attached, no media dock",
            (false, true) => "media dock attached, no numpad",
            (false, false) => "no modules (Everest Core, or a Max with both modules removed)",
        }
    }
}

/// Reply to `11 14` (read form).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ExtendedInfo {
    pub modules: ModuleSet,
    pub dial_mode: DialMode,
    /// Dial menu colour (R, G, B).
    pub dock_color: [u8; 3],
    pub screensaver_seconds: u16,
    pub turn_off_seconds: u16,
    /// (dial, numpad displays) per lighting slot.
    pub display_brightness: [(DisplayBrightness, DisplayBrightness); 5],
}

impl ExtendedInfo {
    pub fn parse(reply: &[u8]) -> Result<Self, ParseError> {
        check(reply, [0x11, 0x14], 33)?;
        let le = |i: usize| u16::from_le_bytes([reply[i], reply[i + 1]]);
        let pair = |i: usize| {
            (
                DisplayBrightness::from_byte(reply[23 + 2 * i]),
                DisplayBrightness::from_byte(reply[24 + 2 * i]),
            )
        };
        Ok(ExtendedInfo {
            modules: ModuleSet {
                media_dock: reply[4] != 0,
                numpad: reply[20] != 0,
            },
            dial_mode: DialMode::from_byte(reply[10]),
            dock_color: [reply[7], reply[8], reply[9]],
            screensaver_seconds: le(11),
            turn_off_seconds: le(13),
            display_brightness: [pair(0), pair(1), pair(2), pair(3), pair(4)],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(prefix: &[u8], fields: &[(usize, u8)]) -> Vec<u8> {
        let mut p = vec![0u8; 64];
        p[..prefix.len()].copy_from_slice(prefix);
        for &(i, b) in fields {
            p[i] = b;
        }
        p
    }

    #[test]
    fn firmware_info() {
        let r = reply(&[0x11, 0x00], &[(4, 0x57), (10, 2), (11, 6)]);
        let info = FirmwareInfo::parse(&r).unwrap();
        assert_eq!(info.version.to_string(), "57");
        assert!(info.version.is_tested());
        assert_eq!(info.active_profile, Some(2));
        assert_eq!(info.lighting_slot, LightingSlot::Custom);
    }

    #[test]
    fn firmware_info_out_of_range_profile() {
        let r = reply(&[0x11, 0x00], &[(4, 0x55), (10, 9), (11, 42)]);
        let info = FirmwareInfo::parse(&r).unwrap();
        assert!(!info.version.is_tested());
        assert_eq!(info.active_profile, None);
        assert_eq!(info.lighting_slot, LightingSlot::Unknown(42));
    }

    #[test]
    fn wrong_or_short_reply() {
        assert_eq!(
            FirmwareInfo::parse(&reply(&[0x11, 0x14], &[])),
            Err(ParseError::WrongReply {
                expected: [0x11, 0x00]
            })
        );
        assert_eq!(
            FirmwareInfo::parse(&[0x11, 0x00, 0, 0, 0x57]),
            Err(ParseError::TooShort { needed: 12, got: 5 })
        );
    }

    #[test]
    fn layout_reply() {
        assert_eq!(
            parse_layout(&reply(&[0x11, 0x12], &[(4, 11)])).unwrap(),
            (11, Layout::Nordic)
        );
    }

    #[test]
    fn extended_info() {
        let r = reply(
            &[0x11, 0x14],
            &[
                (4, 0x01),
                (7, 0xF3),
                (8, 0xCC),
                (9, 0x23),
                (10, 0x11),
                (11, 0x2C),
                (12, 0x01),
                (13, 0x58),
                (14, 0x02),
                (20, 0x01),
                (23, 0x80),
                (24, 0xB2),
            ],
        );
        let info = ExtendedInfo::parse(&r).unwrap();
        assert_eq!(
            info.modules,
            ModuleSet {
                numpad: true,
                media_dock: true
            }
        );
        assert_eq!(info.dial_mode, DialMode::Clock);
        assert_eq!(info.dock_color, [0xF3, 0xCC, 0x23]);
        assert_eq!(info.screensaver_seconds, 300);
        assert_eq!(info.turn_off_seconds, 600);
        assert_eq!(
            info.display_brightness[0],
            (DisplayBrightness::Fixed(0), DisplayBrightness::Fixed(50))
        );
        assert_eq!(
            info.display_brightness[1],
            (
                DisplayBrightness::FollowLighting,
                DisplayBrightness::FollowLighting
            )
        );
    }

    #[test]
    fn core_without_modules() {
        let info = ExtendedInfo::parse(&reply(&[0x11, 0x14], &[])).unwrap();
        assert_eq!(
            info.modules,
            ModuleSet {
                numpad: false,
                media_dock: false
            }
        );
    }
}
