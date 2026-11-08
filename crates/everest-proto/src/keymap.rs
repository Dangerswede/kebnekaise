//! Physical key positions for key remapping (phase 4).
//!
//! The remap command (`14 20`) addresses keys by **position code**, not by
//! HID usage and not by LED index. The codes follow the classic IBM PC/AT
//! key position numbering: the key left of `1` is `0x01`, Tab `0x10`,
//! Caps Lock `0x1E`, Left Shift `0x2C`, and on ISO boards the extra key
//! beside Left Shift is `0x2D`.
//!
//! The table holds only codes reported by the MIT-licensed everest-mac
//! project. None of them has been seen working on hardware by anyone yet,
//! so every entry is [P] in `docs/PROTOCOL.md`. Codes outside the table
//! (function row, Esc, numpad Enter and slash, ...) are not accepted.
//!
//! Names are physical positions on a US board, as in [`crate::ledmap`]:
//! on a Nordic board `semicolon` is Ö, `quote` is Ä, `lbracket` is Å.
//! Nordic aliases (`ö`, `ä`, `å`, `§`, ...) are accepted as input.

use std::fmt;

/// A physical key position code as used by `14 20`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyPos(u8);

impl KeyPos {
    /// The position code byte.
    pub fn code(self) -> u8 {
        self.0
    }

    /// The canonical name of this position.
    pub fn name(self) -> &'static str {
        KEYS.iter()
            .find(|(_, c, _)| *c == self.0)
            .map(|(n, _, _)| *n)
            .expect("KeyPos is only built from the table")
    }

    /// Whether the key exists only on ISO boards (or only on ANSI boards).
    pub fn shape(self) -> Shape {
        KEYS.iter()
            .find(|(_, c, _)| *c == self.0)
            .map(|(_, _, s)| *s)
            .expect("KeyPos is only built from the table")
    }

    /// Looks up a code; `None` if it is not in the table.
    pub fn from_code(code: u8) -> Option<Self> {
        KEYS.iter()
            .any(|(_, c, _)| *c == code)
            .then_some(KeyPos(code))
    }

    /// Looks up a name (canonical, alias, Nordic alias) or `0xNN`.
    pub fn from_name(name: &str) -> Result<Self, KeymapError> {
        let lower = name.trim().to_lowercase();
        if let Some(hex) = lower.strip_prefix("0x") {
            let code =
                u8::from_str_radix(hex, 16).map_err(|_| KeymapError::UnknownKey(name.into()))?;
            return Self::from_code(code).ok_or(KeymapError::UnknownCode(code));
        }
        let canonical = ALIASES
            .iter()
            .find(|(a, _)| *a == lower)
            .map(|(_, c)| *c)
            .unwrap_or(lower.as_str());
        KEYS.iter()
            .find(|(n, _, _)| *n == canonical)
            .map(|(_, c, _)| KeyPos(*c))
            .ok_or_else(|| KeymapError::UnknownKey(name.into()))
    }
}

impl fmt::Display for KeyPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for KeyPos {
    type Err = KeymapError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_name(s)
    }
}

/// Which boards have the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Both,
    IsoOnly,
    AnsiOnly,
}

/// Everything that can be wrong with a remap request.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    #[error("unknown key {0:?} (see `kebnekaise key names`)")]
    UnknownKey(String),
    #[error("position code 0x{0:02x} is not in the documented table")]
    UnknownCode(u8),
}

/// (canonical name, position code, shape). Source: everest-mac (MIT).
const KEYS: &[(&str, u8, Shape)] = &[
    ("grave", 0x01, Shape::Both),
    ("1", 0x02, Shape::Both),
    ("2", 0x03, Shape::Both),
    ("3", 0x04, Shape::Both),
    ("4", 0x05, Shape::Both),
    ("5", 0x06, Shape::Both),
    ("6", 0x07, Shape::Both),
    ("7", 0x08, Shape::Both),
    ("8", 0x09, Shape::Both),
    ("9", 0x0A, Shape::Both),
    ("0", 0x0B, Shape::Both),
    ("minus", 0x0C, Shape::Both),
    ("equal", 0x0D, Shape::Both),
    ("backspace", 0x0F, Shape::Both),
    ("tab", 0x10, Shape::Both),
    ("q", 0x11, Shape::Both),
    ("w", 0x12, Shape::Both),
    ("e", 0x13, Shape::Both),
    ("r", 0x14, Shape::Both),
    ("t", 0x15, Shape::Both),
    ("y", 0x16, Shape::Both),
    ("u", 0x17, Shape::Both),
    ("i", 0x18, Shape::Both),
    ("o", 0x19, Shape::Both),
    ("p", 0x1A, Shape::Both),
    ("lbracket", 0x1B, Shape::Both),
    ("rbracket", 0x1C, Shape::Both),
    ("backslash", 0x1D, Shape::AnsiOnly),
    ("caps", 0x1E, Shape::Both),
    ("a", 0x1F, Shape::Both),
    ("s", 0x20, Shape::Both),
    ("d", 0x21, Shape::Both),
    ("f", 0x22, Shape::Both),
    ("g", 0x23, Shape::Both),
    ("h", 0x24, Shape::Both),
    ("j", 0x25, Shape::Both),
    ("k", 0x26, Shape::Both),
    ("l", 0x27, Shape::Both),
    ("semicolon", 0x28, Shape::Both),
    ("quote", 0x29, Shape::Both),
    ("iso-hash", 0x2A, Shape::IsoOnly),
    ("enter", 0x2B, Shape::Both),
    ("lshift", 0x2C, Shape::Both),
    ("iso-extra", 0x2D, Shape::IsoOnly),
    ("z", 0x2E, Shape::Both),
    ("x", 0x2F, Shape::Both),
    ("c", 0x30, Shape::Both),
    ("v", 0x31, Shape::Both),
    ("b", 0x32, Shape::Both),
    ("n", 0x33, Shape::Both),
    ("m", 0x34, Shape::Both),
    ("comma", 0x35, Shape::Both),
    ("period", 0x36, Shape::Both),
    ("slash", 0x37, Shape::Both),
    ("rshift", 0x39, Shape::Both),
    ("lctrl", 0x3A, Shape::Both),
    ("lwin", 0x3B, Shape::Both),
    ("lalt", 0x3C, Shape::Both),
    ("space", 0x3D, Shape::Both),
    ("ralt", 0x3E, Shape::Both),
    ("rwin", 0x3F, Shape::Both),
    ("rctrl", 0x40, Shape::Both),
    ("insert", 0x4B, Shape::Both),
    ("delete", 0x4C, Shape::Both),
    ("left", 0x4F, Shape::Both),
    ("home", 0x50, Shape::Both),
    ("end", 0x51, Shape::Both),
    ("up", 0x53, Shape::Both),
    ("down", 0x54, Shape::Both),
    ("pgup", 0x55, Shape::Both),
    ("pgdn", 0x56, Shape::Both),
    ("right", 0x59, Shape::Both),
    ("numlock", 0x5A, Shape::Both),
];

/// (alias, canonical name). Lower case.
const ALIASES: &[(&str, &str)] = &[
    ("capslock", "caps"),
    ("lcontrol", "lctrl"),
    ("ctrl", "lctrl"),
    ("rcontrol", "rctrl"),
    ("lshift", "lshift"),
    ("shift", "lshift"),
    ("alt", "lalt"),
    ("altgr", "ralt"),
    ("win", "lwin"),
    ("super", "lwin"),
    ("lsuper", "lwin"),
    ("rsuper", "rwin"),
    ("return", "enter"),
    ("del", "delete"),
    ("ins", "insert"),
    ("pageup", "pgup"),
    ("pagedown", "pgdn"),
    // Nordic keycap legends at the same positions.
    ("§", "grave"),
    ("+", "minus"),
    ("´", "equal"),
    ("å", "lbracket"),
    ("¨", "rbracket"),
    ("ö", "semicolon"),
    ("ä", "quote"),
    ("'", "iso-hash"),
    ("<", "iso-extra"),
    ("-", "slash"),
    (",", "comma"),
    (".", "period"),
];

/// Every documented position: (name, code, shape), in code order.
pub fn all_keys() -> impl Iterator<Item = (KeyPos, Shape)> {
    KEYS.iter().map(|(_, c, s)| (KeyPos(*c), *s))
}

/// Nordic legend for a position, if it differs from the US name.
pub fn nordic_legend(key: KeyPos) -> Option<&'static str> {
    ALIASES
        .iter()
        .filter(|(a, _)| !a.is_ascii() || matches!(*a, "+" | "'" | "<" | "-"))
        .find(|(_, c)| *c == key.name())
        .map(|(a, _)| *a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_and_lctrl() {
        assert_eq!(KeyPos::from_name("caps").unwrap().code(), 0x1E);
        assert_eq!(KeyPos::from_name("CapsLock").unwrap().code(), 0x1E);
        assert_eq!(KeyPos::from_name("lctrl").unwrap().code(), 0x3A);
        assert_eq!(KeyPos::from_name("0x3a").unwrap().name(), "lctrl");
    }

    #[test]
    fn nordic_aliases() {
        assert_eq!(KeyPos::from_name("ö").unwrap().name(), "semicolon");
        assert_eq!(KeyPos::from_name("Å").unwrap().name(), "lbracket");
        assert_eq!(KeyPos::from_name("<").unwrap().name(), "iso-extra");
        assert_eq!(
            nordic_legend(KeyPos::from_name("quote").unwrap()),
            Some("ä")
        );
    }

    #[test]
    fn unknown_codes_rejected() {
        // Not in the table: Esc and the function row.
        assert_eq!(
            KeyPos::from_name("0x6e"),
            Err(KeymapError::UnknownCode(0x6E))
        );
        assert!(KeyPos::from_name("esc").is_err());
        assert!(KeyPos::from_code(0x00).is_none());
        assert!(KeyPos::from_code(0x0E).is_none());
    }

    #[test]
    fn table_is_sorted_and_unique() {
        let codes: Vec<u8> = KEYS.iter().map(|(_, c, _)| *c).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted);
        for (alias, canonical) in ALIASES {
            assert!(
                KEYS.iter().any(|(n, _, _)| n == canonical),
                "alias {alias} points nowhere"
            );
        }
    }
}
