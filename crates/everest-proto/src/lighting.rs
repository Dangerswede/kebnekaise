//! Lighting (phase 2): built-in effects, per-key colours and side strips.
//!
//! Byte layouts are described in `docs/PROTOCOL.md`, section "Lighting
//! (phase 2)". The facts come from the MIT-licensed everest-mac project
//! (<https://github.com/Amorem/everest-mac>), which recovered them from
//! Base Camp 1.9.10; this is our own implementation.

use std::fmt;
use std::str::FromStr;

/// Key LED slots in the per-key frame: 8 packets of 19 (126 used).
pub const KEY_LED_SLOTS: usize = 152;
/// Key LEDs per `14 2C 00 01` packet.
pub const KEYS_PER_PACKET: usize = 19;
/// Side-strip LEDs: main board 30, numpad 15.
pub const SIDE_LEDS: usize = 45;
/// Side LEDs per `14 2D` packet (19, 19, 7).
pub const SIDE_PACKET_SIZES: [usize; 3] = [19, 19, 7];
/// Number of hardware profiles.
pub const PROFILES: std::ops::RangeInclusive<u8> = 1..=5;

/// A 24-bit colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    fn bytes(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

impl FromStr for Rgb {
    type Err = LightingError;

    /// Accepts `RRGGBB` or `#RRGGBB`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let bad = || LightingError::BadColor(s.to_string());
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let v = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
        Ok(Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Everything that can be wrong with a lighting request. Checked before any
/// packet is built, so a bad request never produces bytes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LightingError {
    #[error("not a colour: {0:?} (expected RRGGBB)")]
    BadColor(String),
    #[error("profile {0} is out of range (1..=5)")]
    BadProfile(u8),
    #[error("brightness {0} is out of range (0..=100)")]
    BadBrightness(u8),
    #[error("speed {0} is out of range (1..=5, 1 = slowest)")]
    BadSpeed(u8),
    #[error("{effect} does not support the {mode} colour mode (supported: {supported})")]
    BadMode {
        effect: Effect,
        mode: ColorMode,
        supported: String,
    },
    #[error("{effect} with {mode} colours needs {needed} colour(s), got {got}")]
    WrongColorCount {
        effect: Effect,
        mode: ColorMode,
        needed: usize,
        got: usize,
    },
    #[error("{effect} has no direction")]
    NoDirection { effect: Effect },
    #[error("direction {direction} does not apply to {effect}")]
    BadDirection {
        effect: Effect,
        direction: Direction,
    },
    #[error("unknown key {0:?}")]
    UnknownKey(String),
    #[error("unknown effect {0:?}")]
    UnknownEffect(String),
    #[error("unknown colour mode {0:?}")]
    UnknownMode(String),
    #[error("unknown direction {0:?}")]
    UnknownDirection(String),
}

/// A built-in firmware effect. Each one lives in its own lighting slot of
/// the active profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Effect {
    Static,
    Wave,
    Tornado,
    Breathing,
    Reactive,
    Matrix,
    Yeti,
    Off,
}

impl Effect {
    pub const ALL: [Effect; 8] = [
        Effect::Static,
        Effect::Wave,
        Effect::Tornado,
        Effect::Breathing,
        Effect::Reactive,
        Effect::Matrix,
        Effect::Yeti,
        Effect::Off,
    ];

    /// Lighting slot of the profile (also `11 00` reply byte 11).
    pub fn slot(self) -> u8 {
        match self {
            Effect::Static => 0,
            Effect::Wave => 1,
            Effect::Tornado => 2,
            Effect::Breathing => 3,
            Effect::Reactive => 4,
            Effect::Matrix => 5,
            Effect::Yeti => 7,
            Effect::Off => 8,
        }
    }

    /// Effect id, byte 2 of `14 2C`.
    pub fn id(self) -> u8 {
        match self {
            Effect::Static => 0x00,
            Effect::Breathing => 0x01,
            Effect::Reactive => 0x03,
            Effect::Wave => 0x04,
            Effect::Yeti => 0x06,
            Effect::Tornado => 0x07,
            Effect::Matrix => 0x09,
            Effect::Off => 0x0C,
        }
    }

    /// Colour modes the firmware supports; the first is the default.
    pub fn modes(self) -> &'static [ColorMode] {
        use ColorMode::*;
        match self {
            Effect::Static => &[Single],
            Effect::Wave => &[Rainbow, Single, Dual, Quad],
            Effect::Tornado => &[Rainbow, Single],
            Effect::Breathing => &[Single, Dual, Rainbow],
            Effect::Reactive | Effect::Matrix | Effect::Yeti => &[KeyAndBackground],
            Effect::Off => &[None],
        }
    }

    pub fn has_speed(self) -> bool {
        !matches!(self, Effect::Static | Effect::Off)
    }

    /// Hardware speed byte for the five steps, slowest first. Smaller is
    /// faster. `FF` for effects without speed.
    fn speed_table(self) -> [u8; 5] {
        match self {
            Effect::Wave | Effect::Tornado => [10, 9, 8, 7, 6],
            Effect::Breathing | Effect::Reactive => [5, 4, 3, 1, 0],
            Effect::Matrix => [20, 15, 10, 5, 0],
            Effect::Yeti => [10, 7, 5, 3, 0],
            Effect::Static | Effect::Off => [0xFF; 5],
        }
    }

    fn is_block(self) -> bool {
        matches!(self, Effect::Wave | Effect::Tornado)
    }

    fn default_direction(self) -> Option<Direction> {
        match self {
            Effect::Wave => Some(Direction::Right),
            Effect::Tornado => Some(Direction::Clockwise),
            _ => None,
        }
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{self:?}").to_lowercase())
    }
}

impl FromStr for Effect {
    type Err = LightingError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Effect::ALL
            .into_iter()
            .find(|e| e.to_string() == s.to_ascii_lowercase())
            .ok_or_else(|| LightingError::UnknownEffect(s.to_string()))
    }
}

/// How an effect is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ColorMode {
    /// One colour.
    Single,
    /// Two colours (wave: c1 c2 c1 c2 stops; breathing: alternating).
    Dual,
    /// Four gradient stops (wave only).
    Quad,
    /// The firmware's own rainbow, no colours.
    Rainbow,
    /// Reactive, matrix, yeti: key colour, then background colour.
    KeyAndBackground,
    /// Off: no colours.
    None,
}

impl ColorMode {
    /// How many colours the mode takes.
    pub fn colors(self) -> usize {
        match self {
            ColorMode::Single => 1,
            ColorMode::Dual | ColorMode::KeyAndBackground => 2,
            ColorMode::Quad => 4,
            ColorMode::Rainbow | ColorMode::None => 0,
        }
    }
}

impl fmt::Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ColorMode::Single => "single",
            ColorMode::Dual => "dual",
            ColorMode::Quad => "quad",
            ColorMode::Rainbow => "rainbow",
            ColorMode::KeyAndBackground => "key-and-background",
            ColorMode::None => "none",
        })
    }
}

impl FromStr for ColorMode {
    type Err = LightingError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "single" => ColorMode::Single,
            "dual" => ColorMode::Dual,
            "quad" => ColorMode::Quad,
            "rainbow" => ColorMode::Rainbow,
            "key-and-background" | "background" => ColorMode::KeyAndBackground,
            "none" => ColorMode::None,
            _ => return Err(LightingError::UnknownMode(s.to_string())),
        })
    }
}

/// Wave or tornado direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Direction {
    Right,
    Down,
    Left,
    Up,
    Clockwise,
    Anticlockwise,
}

impl Direction {
    fn byte(self) -> u8 {
        match self {
            Direction::Right => 0,
            Direction::Down => 2,
            Direction::Left => 4,
            Direction::Up => 6,
            Direction::Clockwise => 9,
            Direction::Anticlockwise => 10,
        }
    }

    fn fits(self, effect: Effect) -> bool {
        match effect {
            Effect::Wave => !matches!(self, Direction::Clockwise | Direction::Anticlockwise),
            Effect::Tornado => matches!(self, Direction::Clockwise | Direction::Anticlockwise),
            _ => false,
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{self:?}").to_lowercase())
    }
}

impl FromStr for Direction {
    type Err = LightingError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "right" => Direction::Right,
            "down" => Direction::Down,
            "left" => Direction::Left,
            "up" => Direction::Up,
            "cw" | "clockwise" => Direction::Clockwise,
            "ccw" | "anticlockwise" | "counterclockwise" => Direction::Anticlockwise,
            _ => return Err(LightingError::UnknownDirection(s.to_string())),
        })
    }
}

/// A validated built-in effect request.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct EffectSettings {
    effect: Effect,
    mode: ColorMode,
    colors: Vec<Rgb>,
    /// 1..=5, 1 = slowest.
    speed: u8,
    /// 0..=100 %.
    brightness: u8,
    direction: Option<Direction>,
}

impl EffectSettings {
    /// Checks the request against what the firmware supports.
    ///
    /// `mode` defaults to the effect's first mode, `speed` to 3 (middle),
    /// and the direction to right (wave) or clockwise (tornado).
    pub fn new(
        effect: Effect,
        mode: Option<ColorMode>,
        colors: Vec<Rgb>,
        speed: Option<u8>,
        brightness: u8,
        direction: Option<Direction>,
    ) -> Result<Self, LightingError> {
        let mode = mode.unwrap_or(effect.modes()[0]);
        if !effect.modes().contains(&mode) {
            return Err(LightingError::BadMode {
                effect,
                mode,
                supported: effect
                    .modes()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        }
        if colors.len() != mode.colors() {
            return Err(LightingError::WrongColorCount {
                effect,
                mode,
                needed: mode.colors(),
                got: colors.len(),
            });
        }
        let speed = speed.unwrap_or(3);
        if !(1..=5).contains(&speed) {
            return Err(LightingError::BadSpeed(speed));
        }
        if brightness > 100 {
            return Err(LightingError::BadBrightness(brightness));
        }
        let direction = match (direction, effect.default_direction()) {
            (None, default) => default,
            (Some(_), None) => return Err(LightingError::NoDirection { effect }),
            (Some(d), Some(_)) if !d.fits(effect) => {
                return Err(LightingError::BadDirection {
                    effect,
                    direction: d,
                })
            }
            (Some(d), Some(_)) => Some(d),
        };
        Ok(EffectSettings {
            effect,
            mode,
            colors,
            speed,
            brightness,
            direction,
        })
    }

    pub fn effect(&self) -> Effect {
        self.effect
    }

    pub fn mode(&self) -> ColorMode {
        self.mode
    }

    pub fn brightness(&self) -> u8 {
        self.brightness
    }

    fn hardware_speed(&self) -> u8 {
        self.effect.speed_table()[usize::from(self.speed - 1)]
    }

    /// The `14 2C` packet body, bytes 0..64.
    pub(crate) fn encode(&self) -> [u8; 64] {
        let mut p = [0u8; 64];
        p[0] = 0x14;
        p[1] = 0x2C;
        p[2] = self.effect.id();
        p[3] = 0x00;
        p[4] = self.hardware_speed();
        p[5] = self.brightness;
        let put = |p: &mut [u8; 64], at: usize, c: Rgb| p[at..at + 3].copy_from_slice(&c.bytes());
        let stop = |p: &mut [u8; 64], at: usize, pos: u8, c: Rgb| {
            p[at] = pos;
            p[at + 1..at + 4].copy_from_slice(&c.bytes());
        };

        if self.effect.is_block() {
            // Block data: 6 random flag, 7 direction, 8 width, 9 stop count,
            // then 4-byte stops <pos %> R G B from byte 10.
            p[7] = self.direction.map_or(0, Direction::byte);
            match self.mode {
                ColorMode::Rainbow => {
                    p[6] = 0x02;
                    p[8] = 0x02;
                    p[9] = 0x00;
                    p[10] = 0xFF;
                    p[14] = 0xFF;
                }
                ColorMode::Dual => {
                    let (a, b) = (self.colors[0], self.colors[1]);
                    p[8] = 0x02;
                    p[9] = 0x04;
                    for (i, (pos, c)) in [(25, a), (50, b), (75, a), (100, b)]
                        .into_iter()
                        .enumerate()
                    {
                        stop(&mut p, 10 + 4 * i, pos, c);
                    }
                }
                ColorMode::Quad => {
                    p[8] = 0x02;
                    p[9] = 0x04;
                    for (i, pos) in [25u8, 50, 75, 100].into_iter().enumerate() {
                        stop(&mut p, 10 + 4 * i, pos, self.colors[i]);
                    }
                }
                _ => {
                    // Single colour: one stop at 100 %, byte 14 = FF.
                    p[9] = 0x01;
                    stop(&mut p, 10, 100, self.colors[0]);
                    p[14] = 0xFF;
                }
            }
        } else if self.effect == Effect::Off {
            p[6] = 0xFF;
            p[7] = 0xFF;
            p[8] = 0xFF;
        } else {
            // Effect data: 6 mode, 7..8 FF FF, 9..11 colour 1, 12..14
            // colour 2 (dual breathing), 18..20 background.
            p[7] = 0xFF;
            p[8] = 0xFF;
            match self.mode {
                ColorMode::Dual => {
                    p[6] = 0x10;
                    put(&mut p, 9, self.colors[0]);
                    put(&mut p, 12, self.colors[1]);
                }
                ColorMode::Rainbow => p[6] = 0x02,
                ColorMode::KeyAndBackground => {
                    put(&mut p, 9, self.colors[0]);
                    put(&mut p, 18, self.colors[1]);
                }
                _ => put(&mut p, 9, self.colors[0]),
            }
        }
        p
    }
}

/// One frame of custom (per-key) lighting: every key LED and side LED.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomFrame {
    pub keys: [Rgb; KEY_LED_SLOTS],
    pub side: [Rgb; SIDE_LEDS],
}

impl CustomFrame {
    /// All key and side LEDs in one colour.
    pub fn filled(keys: Rgb, side: Rgb) -> Self {
        CustomFrame {
            keys: [keys; KEY_LED_SLOTS],
            side: [side; SIDE_LEDS],
        }
    }

    /// The 8 key packets of this frame.
    pub(crate) fn key_chunk(&self, ix: usize) -> &[Rgb] {
        &self.keys[ix * KEYS_PER_PACKET..(ix + 1) * KEYS_PER_PACKET]
    }

    /// The 3 side packets of this frame.
    pub(crate) fn side_chunk(&self, ix: usize) -> &[Rgb] {
        let start: usize = SIDE_PACKET_SIZES[..ix].iter().sum();
        &self.side[start..start + SIDE_PACKET_SIZES[ix]]
    }
}

/// Writes RGB triplets from byte 7.
pub(crate) fn put_triplets(p: &mut [u8; 64], colors: &[Rgb]) {
    for (i, c) in colors.iter().enumerate() {
        p[7 + 3 * i..10 + 3 * i].copy_from_slice(&c.bytes());
    }
}

/// Valid effect ids for `14 2C <id> 00`, used by the classifier.
pub(crate) fn is_effect_id(id: u8) -> bool {
    Effect::ALL.iter().any(|e| e.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb::new(0xFF, 0, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 0xFF);

    fn hex(p: &[u8]) -> String {
        let end = p.iter().rposition(|&b| b != 0).map_or(2, |i| i + 1);
        p[..end]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn settings(
        effect: Effect,
        mode: Option<ColorMode>,
        colors: &[Rgb],
        speed: Option<u8>,
        direction: Option<Direction>,
    ) -> Result<EffectSettings, LightingError> {
        EffectSettings::new(effect, mode, colors.to_vec(), speed, 80, direction)
    }

    #[test]
    fn colour_parsing() {
        assert_eq!("ff8000".parse::<Rgb>().unwrap(), Rgb::new(255, 128, 0));
        assert_eq!(
            "#2053A9".parse::<Rgb>().unwrap(),
            Rgb::new(0x20, 0x53, 0xa9)
        );
        assert!("fff".parse::<Rgb>().is_err());
        assert!("gg0000".parse::<Rgb>().is_err());
        assert!("+12345".parse::<Rgb>().is_err());
    }

    #[test]
    fn static_colour() {
        let s = settings(Effect::Static, None, &[RED], None, None).unwrap();
        assert_eq!(hex(&s.encode()), "14 2c 00 00 ff 50 00 ff ff ff");
    }

    #[test]
    fn wave_rainbow_default() {
        let s = settings(Effect::Wave, None, &[], Some(5), None).unwrap();
        // speed 5 = fastest = 6; direction right = 0.
        assert_eq!(
            hex(&s.encode()),
            "14 2c 04 00 06 50 02 00 02 00 ff 00 00 00 ff"
        );
    }

    #[test]
    fn wave_dual_expands_to_four_stops() {
        let s = settings(
            Effect::Wave,
            Some(ColorMode::Dual),
            &[RED, BLUE],
            Some(1),
            Some(Direction::Up),
        )
        .unwrap();
        assert_eq!(
            hex(&s.encode()),
            "14 2c 04 00 0a 50 00 06 02 04 19 ff 00 00 32 00 00 ff 4b ff 00 00 64 00 00 ff"
        );
    }

    #[test]
    fn tornado_single() {
        let s = settings(
            Effect::Tornado,
            Some(ColorMode::Single),
            &[BLUE],
            Some(3),
            Some(Direction::Anticlockwise),
        )
        .unwrap();
        assert_eq!(
            hex(&s.encode()),
            "14 2c 07 00 08 50 00 0a 00 01 64 00 00 ff ff"
        );
    }

    #[test]
    fn breathing_dual_and_reactive_background() {
        let s = settings(
            Effect::Breathing,
            Some(ColorMode::Dual),
            &[RED, BLUE],
            Some(4),
            None,
        )
        .unwrap();
        assert_eq!(
            hex(&s.encode()),
            "14 2c 01 00 01 50 10 ff ff ff 00 00 00 00 ff"
        );
        let s = settings(Effect::Reactive, None, &[RED, BLUE], Some(2), None).unwrap();
        assert_eq!(
            hex(&s.encode()),
            "14 2c 03 00 04 50 00 ff ff ff 00 00 00 00 00 00 00 00 00 00 ff"
        );
    }

    #[test]
    fn off() {
        let s = settings(Effect::Off, None, &[], None, None).unwrap();
        assert_eq!(hex(&s.encode()), "14 2c 0c 00 ff 50 ff ff ff");
    }

    #[test]
    fn validation() {
        assert!(matches!(
            settings(Effect::Static, Some(ColorMode::Rainbow), &[], None, None),
            Err(LightingError::BadMode { .. })
        ));
        assert!(matches!(
            settings(Effect::Static, None, &[], None, None),
            Err(LightingError::WrongColorCount {
                needed: 1,
                got: 0,
                ..
            })
        ));
        assert!(matches!(
            settings(Effect::Wave, None, &[], Some(6), None),
            Err(LightingError::BadSpeed(6))
        ));
        assert!(matches!(
            settings(Effect::Wave, None, &[], None, Some(Direction::Clockwise)),
            Err(LightingError::BadDirection { .. })
        ));
        assert!(matches!(
            settings(Effect::Breathing, None, &[RED], None, Some(Direction::Up)),
            Err(LightingError::NoDirection { .. })
        ));
        assert!(matches!(
            EffectSettings::new(Effect::Static, None, vec![RED], None, 101, None),
            Err(LightingError::BadBrightness(101))
        ));
    }

    #[test]
    fn parsing_names() {
        for e in Effect::ALL {
            assert_eq!(e.to_string().parse::<Effect>().unwrap(), e);
        }
        assert_eq!(
            "ccw".parse::<Direction>().unwrap(),
            Direction::Anticlockwise
        );
        assert!("custom".parse::<Effect>().is_err());
    }

    #[test]
    fn frame_chunks() {
        let f = CustomFrame::filled(RED, BLUE);
        assert_eq!(f.key_chunk(7).len(), 19);
        assert_eq!(f.side_chunk(0).len(), 19);
        assert_eq!(f.side_chunk(2).len(), 7);
    }
}
