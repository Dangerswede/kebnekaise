//! Key names to LED indices for custom (per-key) lighting.
//!
//! Index = `column * 9 + row`, the same on every language; only the shape
//! (ANSI or ISO) changes which indices exist. Names are physical positions
//! on a US keyboard, so they work whatever is printed on the keycap (on a
//! Nordic board `semicolon` is Ö, `iso-extra` is `<>|`).
//!
//! Indices are taken from the MIT-licensed everest-mac project, which
//! verified them LED by LED on a UK-ISO board. See `docs/PROTOCOL.md`.

use crate::lighting::{LightingError, SIDE_LEDS};
use crate::LayoutShape;

/// Keys present on every shape: (name, LED index).
const COMMON: &[(&str, u8)] = &[
    ("esc", 0),
    ("f1", 9),
    ("f2", 18),
    ("f3", 27),
    ("f4", 36),
    ("f5", 45),
    ("f6", 54),
    ("f7", 63),
    ("f8", 72),
    ("f9", 81),
    ("f10", 90),
    ("f11", 99),
    ("f12", 108),
    ("prtsc", 117),
    ("scrlk", 114),
    ("pause", 123),
    ("grave", 1),
    ("1", 10),
    ("2", 19),
    ("3", 28),
    ("4", 37),
    ("5", 46),
    ("6", 55),
    ("7", 64),
    ("8", 73),
    ("9", 82),
    ("0", 91),
    ("minus", 100),
    ("equal", 109),
    ("backspace", 87),
    ("insert", 96),
    ("home", 105),
    ("pgup", 115),
    ("tab", 2),
    ("q", 11),
    ("w", 20),
    ("e", 29),
    ("r", 38),
    ("t", 47),
    ("y", 56),
    ("u", 65),
    ("i", 74),
    ("o", 83),
    ("p", 92),
    ("lbracket", 101),
    ("rbracket", 110),
    ("enter", 120),
    ("delete", 88),
    ("end", 97),
    ("pgdn", 106),
    ("caps", 3),
    ("a", 12),
    ("s", 21),
    ("d", 30),
    ("f", 39),
    ("g", 48),
    ("h", 57),
    ("j", 66),
    ("k", 75),
    ("l", 84),
    ("semicolon", 93),
    ("quote", 102),
    ("lshift", 4),
    ("z", 22),
    ("x", 31),
    ("c", 40),
    ("v", 49),
    ("b", 58),
    ("n", 67),
    ("m", 76),
    ("comma", 85),
    ("period", 94),
    ("slash", 103),
    ("rshift", 121),
    ("up", 124),
    ("lctrl", 5),
    ("lwin", 14),
    ("lalt", 23),
    ("space", 41),
    ("ralt", 68),
    ("rwin", 77),
    ("fn", 86),
    ("rctrl", 95),
    ("left", 104),
    ("down", 113),
    ("right", 122),
    // Numpad module.
    ("numlock", 6),
    ("kp-slash", 24),
    ("kp-star", 16),
    ("kp-minus", 15),
    ("kp7", 61),
    ("kp8", 69),
    ("kp9", 70),
    ("kp-plus", 7),
    ("kp4", 51),
    ("kp5", 52),
    ("kp6", 60),
    ("kp1", 34),
    ("kp2", 42),
    ("kp3", 43),
    ("kp-enter", 33),
    ("kp0", 78),
    ("kp-dot", 79),
];

/// ISO only: `#~` in the Enter notch and the extra key beside left Shift.
const ISO_ONLY: &[(&str, u8)] = &[("iso-hash", 111), ("iso-extra", 13)];
/// ANSI only: `\|` at the end of the QWERTY row.
const ANSI_ONLY: &[(&str, u8)] = &[("backslash", 119)];

fn shape_keys(shape: LayoutShape) -> impl Iterator<Item = &'static (&'static str, u8)> {
    let extra = match shape {
        LayoutShape::Iso => ISO_ONLY,
        LayoutShape::Ansi => ANSI_ONLY,
    };
    COMMON.iter().chain(extra.iter())
}

/// Every key name and its LED index for a shape.
pub fn key_names(shape: LayoutShape) -> Vec<(&'static str, u8)> {
    shape_keys(shape).copied().collect()
}

/// LED index of a key, or an error naming the key.
pub fn key_index(name: &str, shape: LayoutShape) -> Result<u8, LightingError> {
    let lower = name.to_ascii_lowercase();
    shape_keys(shape)
        .find(|(n, _)| *n == lower)
        .map(|&(_, i)| i)
        .ok_or_else(|| LightingError::UnknownKey(name.to_string()))
}

/// Side-strip LED index from `side0`..`side44`.
pub fn side_index(name: &str) -> Option<usize> {
    let n: usize = name
        .to_ascii_lowercase()
        .strip_prefix("side")?
        .parse()
        .ok()?;
    (n < SIDE_LEDS).then_some(n)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn indices_are_unique_and_in_range() {
        for shape in [LayoutShape::Iso, LayoutShape::Ansi] {
            let keys = key_names(shape);
            let names: HashSet<_> = keys.iter().map(|k| k.0).collect();
            let leds: HashSet<_> = keys.iter().map(|k| k.1).collect();
            assert_eq!(names.len(), keys.len(), "{shape:?}");
            assert_eq!(leds.len(), keys.len(), "{shape:?}");
            assert!(leds.iter().all(|&i| i < 126));
        }
        // Full-size ISO: 88 keys on the main board + 17 on the numpad.
        assert_eq!(key_names(LayoutShape::Iso).len(), 105);
        assert_eq!(key_names(LayoutShape::Ansi).len(), 104);
    }

    #[test]
    fn lookups() {
        assert_eq!(key_index("ESC", LayoutShape::Iso).unwrap(), 0);
        assert_eq!(key_index("iso-extra", LayoutShape::Iso).unwrap(), 13);
        assert!(key_index("iso-extra", LayoutShape::Ansi).is_err());
        assert_eq!(key_index("backslash", LayoutShape::Ansi).unwrap(), 119);
        assert!(key_index("nope", LayoutShape::Iso).is_err());
        assert_eq!(side_index("side44"), Some(44));
        assert_eq!(side_index("side45"), None);
        assert_eq!(side_index("esc"), None);
    }
}
