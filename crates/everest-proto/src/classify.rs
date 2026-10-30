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

/// What a host-to-device packet is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Classification {
    pub name: &'static str,
    pub class: SafetyClass,
}

const fn c(name: &'static str, class: SafetyClass) -> Classification {
    Classification { name, class }
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
        (0x12, 0x08) => c("action slot select", Persist),
        (0x13, 0x40) => c("flash reset", Deny),
        (0x13, 0x41) => c("dial picture reset", Persist),
        (0x13, 0x42) => c("display-key picture reset", Persist),
        (0x13, 0x55) => c("save slot to flash", Persist),
        (0x14, 0x00) => c("switch profile/slot", Volatile),
        (0x14, 0x20) => c("key remap", Persist),
        (0x14, 0x21) => c("key remap with modifiers", Persist),
        (0x14, 0x2C) if at(2) == 0x0A && at(3) == 0x00 => c("enter custom mode", Volatile),
        (0x14, 0x2C) if at(2) == 0x00 && at(3) == 0x01 => c("per-key colours", Volatile),
        (0x14, 0x2C) => c("effect parameters", Persist),
        (0x14, 0x2D) => c("side-strip colours", Volatile),
        (0x14, 0xA0) => c("bind keys to static slot", Volatile),
        (0x17, 0xAA) => c("action payload", Persist),
        (0xAA, 0x55) => match (at(2), at(3)) {
            (0x21, 0x03) => c("picture target: dial", Transfer),
            (0x21, 0x04) => c("picture target: display keys", Transfer),
            (0x22, 0x00) => c("picture target confirm", Transfer),
            (0x10, _) => c("picture descriptor", Transfer),
            (0x7F, _) => c("device reset", Deny),
            _ => c("unknown flash command", Deny),
        },
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
    fn short_input_does_not_panic() {
        assert_eq!(classify(&[0x11]).class, SafetyClass::Deny);
        assert_eq!(classify(&[0xAA, 0x55]).class, SafetyClass::Deny);
    }
}
