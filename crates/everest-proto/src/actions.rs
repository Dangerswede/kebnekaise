//! Display-key (D1..D4) action slots (phase 4).
//!
//! The keyboard stores one short text per display key and, once a key has
//! an action, reports its presses as `01` events (byte 42). It does not run
//! the action itself: a program on the computer has to (Base Camp's service
//! did). Our profile files keep the real action; the text stored in the
//! keyboard is only a marker. See "Keys and display-key actions (phase 4)"
//! in `docs/PROTOCOL.md`.

use std::fmt;

/// Longest text the payload packet can carry.
pub const MAX_ACTION_TEXT: usize = 55;

/// One of the four display keys on the numpad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DKey(u8);

impl DKey {
    pub const ALL: [DKey; 4] = [DKey(1), DKey(2), DKey(3), DKey(4)];

    /// D1..D4 from 1..=4.
    pub fn new(n: u8) -> Result<Self, ActionError> {
        (1..=4)
            .contains(&n)
            .then_some(DKey(n))
            .ok_or(ActionError::BadKey(n.to_string()))
    }

    /// 1..=4.
    pub fn number(self) -> u8 {
        self.0
    }

    /// The bit of this key in byte 42 of a `01` event report.
    pub fn event_bit(self) -> u8 {
        0x02 << (self.0 - 1)
    }

    /// Which display keys a `01` event report says are pressed.
    pub fn pressed_in(report: &[u8]) -> Vec<DKey> {
        if report.first() != Some(&0x01) {
            return Vec::new();
        }
        let byte = report.get(42).copied().unwrap_or(0);
        Self::ALL
            .into_iter()
            .filter(|k| byte & k.event_bit() != 0)
            .collect()
    }
}

impl fmt::Display for DKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "D{}", self.0)
    }
}

impl std::str::FromStr for DKey {
    type Err = ActionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        let n = t
            .strip_prefix(['d', 'D'])
            .unwrap_or(t)
            .parse::<u8>()
            .map_err(|_| ActionError::BadKey(s.into()))?;
        DKey::new(n).map_err(|_| ActionError::BadKey(s.into()))
    }
}

/// The type byte of the payload, as Base Camp uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoredKind {
    /// `0x02`: open a URL.
    Url,
    /// `0x04`: run a command.
    Shell,
}

impl StoredKind {
    pub fn byte(self) -> u8 {
        match self {
            StoredKind::Url => 0x02,
            StoredKind::Shell => 0x04,
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x02 => Some(StoredKind::Url),
            0x04 => Some(StoredKind::Shell),
            _ => None,
        }
    }
}

/// A validated text for the payload packet: printable ASCII, 1..=55 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionText(String);

impl ActionText {
    pub fn new(text: &str) -> Result<Self, ActionError> {
        if text.is_empty() || text.len() > MAX_ACTION_TEXT {
            return Err(ActionError::BadLength(text.len()));
        }
        if !text.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return Err(ActionError::NotPrintableAscii(text.into()));
        }
        Ok(ActionText(text.into()))
    }

    /// The marker our tool stores for a display key: a URL that points
    /// nowhere, so that even Base Camp could not do harm with it.
    pub fn marker(key: DKey) -> Self {
        ActionText(format!("kebnekaise:d{}", key.number()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Everything that can be wrong with an action request.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ActionError {
    #[error("not a display key: {0:?} (expected D1..D4)")]
    BadKey(String),
    #[error("action text is {0} bytes; the keyboard takes 1..=55")]
    BadLength(usize),
    #[error("action text {0:?} is not printable ASCII")]
    NotPrintableAscii(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse() {
        assert_eq!("D1".parse::<DKey>().unwrap().number(), 1);
        assert_eq!("d4".parse::<DKey>().unwrap().number(), 4);
        assert_eq!("3".parse::<DKey>().unwrap().number(), 3);
        assert!("D5".parse::<DKey>().is_err());
        assert!("D0".parse::<DKey>().is_err());
    }

    #[test]
    fn event_bits() {
        let mut report = [0u8; 64];
        report[0] = 0x01;
        report[42] = 0x08;
        assert_eq!(DKey::pressed_in(&report), vec![DKey(3)]);
        report[42] = 0x02 | 0x10;
        assert_eq!(DKey::pressed_in(&report), vec![DKey(1), DKey(4)]);
        report[0] = 0x11;
        assert!(DKey::pressed_in(&report).is_empty());
    }

    #[test]
    fn text_rules() {
        assert!(ActionText::new("").is_err());
        assert!(ActionText::new(&"x".repeat(56)).is_err());
        assert!(ActionText::new(&"x".repeat(55)).is_ok());
        assert!(ActionText::new("åäö").is_err());
        assert!(ActionText::new("a\nb").is_err());
        assert_eq!(ActionText::marker(DKey(2)).as_str(), "kebnekaise:d2");
    }
}
