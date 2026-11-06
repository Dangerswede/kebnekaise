/// How dangerous a host-to-device packet is. See "Safety classes" in
/// `docs/PROTOCOL.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum SafetyClass {
    /// Query without known side effects.
    Read,
    /// Changes live state only, as far as we know.
    Volatile,
    /// Writes the keyboard's flash.
    Persist,
    /// Picture transfer (same mechanism as the firmware updater).
    Transfer,
    /// Never sent: resets, firmware, bootloader, or not understood.
    Deny,
}

/// Which feature a packet belongs to. Session policies allow writes per
/// group, so a lighting session cannot send, say, a key remap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Group {
    /// Queries (`11 00`, `11 12`, `11 14` read form).
    Info,
    /// Profile/slot switch, effects, per-key colours, saving a slot.
    Lighting,
    /// Dial and display-key settings and live values.
    Displays,
    /// Display-key actions.
    Actions,
    /// Key remapping.
    Keymap,
    /// Picture transfer and picture resets.
    Pictures,
    /// Resets, firmware, anything not understood.
    Denied,
}

/// What a host-to-device packet is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Classification {
    pub name: &'static str,
    pub class: SafetyClass,
    pub group: Group,
}

const fn c(name: &'static str, class: SafetyClass) -> Classification {
    let group = match class {
        SafetyClass::Read => Group::Info,
        SafetyClass::Deny => Group::Denied,
        _ => Group::Displays,
    };
    Classification { name, class, group }
}

const fn g(name: &'static str, class: SafetyClass, group: Group) -> Classification {
    Classification { name, class, group }
}

/// Lighting packets must match their documented form exactly (ranges and
/// fixed bytes); anything else is denied. See "Lighting (phase 2)" in
/// `docs/PROTOCOL.md`.
fn classify_lighting(bytes: &[u8]) -> Classification {
    use Group::Lighting;
    use SafetyClass::*;
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let zero_from = |from: usize| bytes.iter().skip(from).all(|&b| b == 0);

    match (at(0), at(1), at(2), at(3)) {
        (0x14, 0x00, 0x00, 0x00) if (1..=5).contains(&at(4)) && at(5) <= 8 && zero_from(6) => {
            g("switch profile/slot", Volatile, Lighting)
        }
        (0x13, 0x55, 0x00, 0x00) if at(4) <= 8 && zero_from(5) => {
            g("save slot to flash", Persist, Lighting)
        }
        (0x14, 0x2C, 0x0A, 0x00) if at(4) == 0xFF && at(5) <= 100 && zero_from(6) => {
            g("enter custom mode", Volatile, Lighting)
        }
        (0x14, 0x2C, 0x00, 0x01) if at(4) < 8 && at(5) <= 100 && at(6) == 0 => {
            g("per-key colours", Volatile, Lighting)
        }
        (0x14, 0x2C, id, 0x00) if crate::lighting::is_effect_id(id) && at(5) <= 100 => {
            g("effect parameters", Persist, Lighting)
        }
        (0x14, 0x2D, 0x0A, 0x00) if at(4) < 3 && at(5) == 0xFF && at(6) == 0 => {
            let used = 7 + 3 * crate::lighting::SIDE_PACKET_SIZES[usize::from(at(4))];
            if zero_from(used) {
                g("side-strip colours", Volatile, Lighting)
            } else {
                c("side-strip colours (malformed)", Deny)
            }
        }
        (0x14, 0xA0, chunk, 0x01) if chunk < 3 && zero_from(4) => {
            g("bind keys to static slot", Volatile, Lighting)
        }
        _ => c("lighting command (malformed)", Deny),
    }
}

/// Picture resets must match their documented form exactly. See
/// "Pictures (phase 3)" in `docs/PROTOCOL.md`.
fn classify_picture_reset(bytes: &[u8]) -> Classification {
    use Group::Pictures;
    use SafetyClass::*;
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let zero_from = |from: usize| bytes.iter().skip(from).all(|&b| b == 0);

    match (at(0), at(1), at(2), at(3)) {
        (0x13, 0x41, 0x00, 0x00) if at(4) == 0x01 && zero_from(5) => {
            g("dial picture reset", Persist, Pictures)
        }
        (0x13, 0x42, 0x00, 0x00)
            if (4..9).all(|i| at(i) <= 0x0F) && (4..9).any(|i| at(i) != 0) && zero_from(9) =>
        {
            g("display-key picture reset", Persist, Pictures)
        }
        _ => c("picture reset (malformed)", Deny),
    }
}

/// `aa 55` feature reports: only the picture-transfer forms Base Camp uses,
/// with the two documented targets and exact picture sizes.
fn classify_transfer(bytes: &[u8]) -> Classification {
    use crate::pictures::{DIAL_BYTES, KEY_BYTES, TARGET_DIAL, TARGET_KEYS};
    use Group::Pictures;
    use SafetyClass::*;
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let zero_from = |from: usize| bytes.iter().skip(from).all(|&b| b == 0);

    match (at(2), at(3)) {
        (0x21, TARGET_DIAL) if zero_from(4) => g("picture target: dial", Transfer, Pictures),
        (0x21, TARGET_KEYS) if zero_from(4) => {
            g("picture target: display keys", Transfer, Pictures)
        }
        (0x22, 0x00) if zero_from(4) => g("picture target confirm", Transfer, Pictures),
        (0x10, _) => {
            let size = usize::from(at(3)) | usize::from(at(4)) << 8 | usize::from(at(5)) << 16;
            let fixed = at(8) == 0 && at(9) == 0 && at(10) == 0x02 && zero_from(13);
            let (profile, key) = (at(11), at(12));
            let slot_ok = match size {
                KEY_BYTES => (1..=5).contains(&profile) && key <= 3,
                DIAL_BYTES => profile == 1 && key == 0,
                _ => false,
            };
            if fixed && slot_ok {
                g("picture descriptor", Transfer, Pictures)
            } else {
                c("picture descriptor (malformed)", Deny)
            }
        }
        (0x7F, _) => c("device reset", Deny),
        _ => c("unknown flash command", Deny),
    }
}

/// Classifies a host-to-device packet (interrupt channel or feature report).
///
/// Anything not described in `docs/PROTOCOL.md` is [`SafetyClass::Deny`].
/// Read commands only count as reads in their exact, zero-padded form.
/// Raw picture chunks cannot be recognised by content; they are only ever
/// sent inside a transfer started by an accepted descriptor (phase 3).
pub fn classify(bytes: &[u8]) -> Classification {
    use SafetyClass::*;

    if bytes.len() < 4 {
        return c("truncated", Deny);
    }
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let rest_zero = |from: usize| bytes.iter().skip(from).all(|&b| b == 0);

    match (at(0), at(1)) {
        (0x11, 0x00) if rest_zero(2) => c("firmware info", Read),
        (0x11, 0x12) if rest_zero(2) => c("layout", Read),
        (0x11, 0x14) if rest_zero(2) => c("extended info", Read),
        (0x11, 0x14) if at(3) == 0x01 => c("settings write", Persist),
        (0x11, 0x02) => c("built-in icon select", Persist),
        (0x11, 0x80) => c("clock announce", Volatile),
        (0x11, 0x81) => c("dial metric", Volatile),
        (0x11, 0x83) => c("dial volume", Volatile),
        (0x11, 0x84) => c("clock set", Volatile),
        (0x12, 0x08) => g("action slot select", Persist, Group::Actions),
        (0x13, 0x40) => c("flash reset", Deny),
        (0x13, 0x41) | (0x13, 0x42) => classify_picture_reset(bytes),
        (0x13, 0x55) | (0x14, 0x00) | (0x14, 0x2C) | (0x14, 0x2D) | (0x14, 0xA0) => {
            classify_lighting(bytes)
        }
        (0x14, 0x20) => g("key remap", Persist, Group::Keymap),
        (0x14, 0x21) => g("key remap with modifiers", Persist, Group::Keymap),
        (0x17, 0xAA) => g("action payload", Persist, Group::Actions),
        (0xAA, 0x55) => classify_transfer(bytes),
        _ => c("unknown", Deny),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(prefix: &[u8]) -> [u8; 64] {
        let mut p = [0u8; 64];
        p[..prefix.len()].copy_from_slice(prefix);
        p
    }

    #[test]
    fn reads_must_be_exact() {
        assert_eq!(classify(&pad(&[0x11, 0x00])).class, SafetyClass::Read);
        // A "read" with extra bytes is not something we understand.
        assert_eq!(classify(&pad(&[0x11, 0x00, 0x01])).class, SafetyClass::Deny);
        assert_eq!(
            classify(&pad(&[0x11, 0x12, 0x00, 0x00, 0x05])).class,
            SafetyClass::Deny
        );
    }

    #[test]
    fn settings_write_is_not_a_read() {
        assert_eq!(
            classify(&pad(&[0x11, 0x14, 0x00, 0x01])).class,
            SafetyClass::Persist
        );
    }

    #[test]
    fn resets_and_unknown_flash_commands_are_denied() {
        for p in [
            pad(&[0x13, 0x40, 0x00, 0x00, 0x01]),
            pad(&[0xAA, 0x55, 0x7F]),
            pad(&[0xAA, 0x55, 0x21, 0x05]),
            pad(&[0xAA, 0x55, 0x21, 0x01]),
            pad(&[0xAA, 0x55, 0x30]),
            pad(&[0x42, 0x00]),
            pad(&[]),
        ] {
            assert_eq!(classify(&p).class, SafetyClass::Deny, "{:02x?}", &p[..4]);
        }
    }

    #[test]
    fn picture_transfer_targets() {
        assert_eq!(
            classify(&pad(&[0xAA, 0x55, 0x21, 0x03])).class,
            SafetyClass::Transfer
        );
        assert_eq!(
            classify(&pad(&[0xAA, 0x55, 0x21, 0x04])).class,
            SafetyClass::Transfer
        );
    }

    #[test]
    fn picture_packets_are_strict() {
        let key_desc = |profile: u8, key: u8| {
            pad(&[
                0xAA, 0x55, 0x10, 0x80, 0x28, 0x00, 0x12, 0x34, 0, 0, 0x02, profile, key,
            ])
        };
        let dial_desc = |profile: u8, key: u8| {
            pad(&[
                0xAA, 0x55, 0x10, 0x80, 0x7E, 0x01, 0x12, 0x34, 0, 0, 0x02, profile, key,
            ])
        };
        let ok = [
            pad(&[0xAA, 0x55, 0x21, 0x03]),
            pad(&[0xAA, 0x55, 0x21, 0x04]),
            pad(&[0xAA, 0x55, 0x22, 0x00]),
            key_desc(1, 0),
            key_desc(5, 3),
            dial_desc(1, 0),
            pad(&[0x13, 0x41, 0x00, 0x00, 0x01]),
            pad(&[0x13, 0x42, 0x00, 0x00, 0x0F]),
            pad(&[0x13, 0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08]),
        ];
        for p in ok {
            let got = classify(&p);
            assert_ne!(got.class, SafetyClass::Deny, "{:02x?}", &p[..13]);
            assert_eq!(got.group, Group::Pictures, "{:02x?}", &p[..13]);
        }
        let mut long = key_desc(1, 0);
        long[13] = 1;
        let mut bad_fixed = key_desc(1, 0);
        bad_fixed[10] = 0x01;
        let denied = [
            key_desc(0, 0),
            key_desc(6, 0),
            key_desc(1, 4),
            dial_desc(2, 0),
            dial_desc(1, 1),
            pad(&[0xAA, 0x55, 0x10, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0x02, 1, 0]), // 4 KiB
            long,
            bad_fixed,
            pad(&[0xAA, 0x55, 0x21, 0x03, 0x01]),
            pad(&[0xAA, 0x55, 0x22, 0x01]),
            pad(&[0xAA, 0x55, 0x7F]),
            pad(&[0x13, 0x41, 0x00, 0x00, 0x02]),
            pad(&[0x13, 0x41, 0x00, 0x00, 0x01, 0x01]),
            pad(&[0x13, 0x42, 0x00, 0x00]),       // nothing selected
            pad(&[0x13, 0x42, 0x00, 0x00, 0x10]), // bit 4
            pad(&[0x13, 0x42, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0x01]),
        ];
        for p in denied {
            assert_eq!(classify(&p).class, SafetyClass::Deny, "{:02x?}", &p[..13]);
        }
    }

    #[test]
    fn lighting_packets_are_strict() {
        let ok = [
            (
                pad(&[0x14, 0x00, 0x00, 0x00, 0x01, 0x00]),
                SafetyClass::Volatile,
            ),
            (
                pad(&[0x14, 0x00, 0x00, 0x00, 0x05, 0x08]),
                SafetyClass::Volatile,
            ),
            (pad(&[0x13, 0x55, 0x00, 0x00, 0x06]), SafetyClass::Persist),
            (
                pad(&[0x14, 0x2C, 0x0A, 0x00, 0xFF, 0x64]),
                SafetyClass::Volatile,
            ),
            (
                pad(&[0x14, 0x2C, 0x00, 0x01, 0x07, 0x64, 0x00, 0xFF]),
                SafetyClass::Volatile,
            ),
            (
                pad(&[0x14, 0x2C, 0x04, 0x00, 0x06, 0x64]),
                SafetyClass::Persist,
            ),
            (
                pad(&[0x14, 0x2D, 0x0A, 0x00, 0x02, 0xFF, 0x00, 0x01]),
                SafetyClass::Volatile,
            ),
            (pad(&[0x14, 0xA0, 0x02, 0x01]), SafetyClass::Volatile),
        ];
        for (p, class) in ok {
            let got = classify(&p);
            assert_eq!(got.class, class, "{:02x?}", &p[..8]);
            assert_eq!(got.group, Group::Lighting, "{:02x?}", &p[..8]);
        }
        let denied = [
            pad(&[0x14, 0x00, 0x00, 0x00, 0x00, 0x00]), // profile 0
            pad(&[0x14, 0x00, 0x00, 0x00, 0x06, 0x00]), // profile 6
            pad(&[0x14, 0x00, 0x00, 0x00, 0x01, 0x09]), // slot 9
            pad(&[0x13, 0x55, 0x00, 0x00, 0x09]),       // slot 9
            pad(&[0x14, 0x2C, 0x05, 0x00, 0x06, 0x64]), // unknown effect id
            pad(&[0x14, 0x2C, 0x04, 0x00, 0x06, 0x65]), // brightness 101
            pad(&[0x14, 0x2C, 0x00, 0x01, 0x08, 0x64]), // key packet 8
            pad(&[0x14, 0x2D, 0x0A, 0x00, 0x03, 0xFF]), // side packet 3
            pad(&[0x14, 0xA0, 0x03, 0x01]),             // chunk 3
            pad(&[0x14, 0xA0, 0x00, 0x00]),
        ];
        for p in denied {
            assert_eq!(classify(&p).class, SafetyClass::Deny, "{:02x?}", &p[..8]);
        }
        // The last side packet carries 7 LEDs: bytes after them must be zero.
        let mut p = pad(&[0x14, 0x2D, 0x0A, 0x00, 0x02, 0xFF]);
        p[7 + 21] = 1;
        assert_eq!(classify(&p).class, SafetyClass::Deny);
    }

    #[test]
    fn short_input_does_not_panic() {
        assert_eq!(classify(&[0x11]).class, SafetyClass::Deny);
        assert_eq!(classify(&[0xAA, 0x55]).class, SafetyClass::Deny);
    }
}
