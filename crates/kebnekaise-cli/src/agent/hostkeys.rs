//! Keys on the computer, not on the keyboard: names from profile files
//! (`ctrl`, `f5`, `a`, ...) and text, turned into Linux input key codes.
//!
//! The agent types through a virtual keyboard (uinput). The computer then
//! reads those codes with its own layout, so typing text needs to know that
//! layout: on a Swedish layout the key right of `0` gives `+`, on a US
//! layout `-`. Only characters the layout has a key for can be typed; the
//! agent checks every macro when it starts.
//!
//! Key codes are from Linux `input-event-codes.h`.

use everest_core::profile::{Macro, MacroStep};

/// A virtual keyboard event, or a pause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Down(u16),
    Up(u16),
    SleepMs(u32),
}

/// The keyboard layout the computer uses, needed to type text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum HostLayout {
    /// Swedish / Finnish (Nordic ISO).
    Se,
    /// Swedish without dead keys (KDE: "Swedish (no dead keys)"):
    /// ´ ` ¨ ^ ~ come out at once, no space needed.
    SeNodeadkeys,
    /// US English.
    Us,
}

const SHIFT: u16 = 42; // KEY_LEFTSHIFT
const ALTGR: u16 = 100; // KEY_RIGHTALT
const SPACE: u16 = 57;

/// Pause between two keys of typed text, so slow applications keep up.
const TYPE_PAUSE_MS: u32 = 8;
/// Pause between a dead key and the space after it.
const DEAD_PAUSE_MS: u32 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    Plain,
    Shift,
    AltGr,
}

/// How to type one character: key, level, and whether the key is a dead
/// key (then a space follows to get the character itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stroke {
    code: u16,
    level: Level,
    dead: bool,
}

const fn s(code: u16, level: Level) -> Stroke {
    Stroke {
        code,
        level,
        dead: false,
    }
}

const fn dead(code: u16, level: Level) -> Stroke {
    Stroke {
        code,
        level,
        dead: true,
    }
}

/// Letter keys a..z.
const LETTERS: [u16; 26] = [
    30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45,
    21, 44,
];
/// Digit keys 1..9, 0.
const DIGITS: [u16; 10] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

fn digit_key(d: char) -> u16 {
    match d {
        '0' => 11,
        _ => DIGITS[(d as u8 - b'1') as usize],
    }
}

fn stroke(layout: HostLayout, c: char) -> Option<Stroke> {
    if layout == HostLayout::SeNodeadkeys {
        // Same keys as `se`; the dead keys type their character directly.
        return stroke(HostLayout::Se, c).map(|st| Stroke { dead: false, ..st });
    }
    use Level::*;
    if c.is_ascii_lowercase() {
        return Some(s(LETTERS[(c as u8 - b'a') as usize], Plain));
    }
    if c.is_ascii_uppercase() {
        return Some(s(LETTERS[(c as u8 - b'A') as usize], Shift));
    }
    if c.is_ascii_digit() {
        return Some(s(digit_key(c), Plain));
    }
    match c {
        ' ' => return Some(s(SPACE, Plain)),
        '\n' => return Some(s(28, Plain)),
        '\t' => return Some(s(15, Plain)),
        _ => {}
    }
    match layout {
        HostLayout::Se | HostLayout::SeNodeadkeys => Some(match c {
            '!' => s(2, Shift),
            '"' => s(3, Shift),
            '#' => s(4, Shift),
            '¤' => s(5, Shift),
            '%' => s(6, Shift),
            '&' => s(7, Shift),
            '/' => s(8, Shift),
            '(' => s(9, Shift),
            ')' => s(10, Shift),
            '=' => s(11, Shift),
            '@' => s(3, AltGr),
            '£' => s(4, AltGr),
            '$' => s(5, AltGr),
            '€' => s(6, AltGr),
            '{' => s(8, AltGr),
            '[' => s(9, AltGr),
            ']' => s(10, AltGr),
            '}' => s(11, AltGr),
            '+' => s(12, Plain),
            '?' => s(12, Shift),
            '\\' => s(12, AltGr),
            '´' => dead(13, Plain),
            '`' => dead(13, Shift),
            'å' => s(26, Plain),
            'Å' => s(26, Shift),
            '¨' => dead(27, Plain),
            '^' => dead(27, Shift),
            '~' => dead(27, AltGr),
            'ö' => s(39, Plain),
            'Ö' => s(39, Shift),
            'ä' => s(40, Plain),
            'Ä' => s(40, Shift),
            '§' => s(41, Plain),
            '½' => s(41, Shift),
            '\'' => s(43, Plain),
            '*' => s(43, Shift),
            ',' => s(51, Plain),
            ';' => s(51, Shift),
            '.' => s(52, Plain),
            ':' => s(52, Shift),
            '-' => s(53, Plain),
            '_' => s(53, Shift),
            '<' => s(86, Plain),
            '>' => s(86, Shift),
            '|' => s(86, AltGr),
            _ => return None,
        }),
        HostLayout::Us => Some(match c {
            '!' => s(2, Shift),
            '@' => s(3, Shift),
            '#' => s(4, Shift),
            '$' => s(5, Shift),
            '%' => s(6, Shift),
            '^' => s(7, Shift),
            '&' => s(8, Shift),
            '*' => s(9, Shift),
            '(' => s(10, Shift),
            ')' => s(11, Shift),
            '-' => s(12, Plain),
            '_' => s(12, Shift),
            '=' => s(13, Plain),
            '+' => s(13, Shift),
            '[' => s(26, Plain),
            '{' => s(26, Shift),
            ']' => s(27, Plain),
            '}' => s(27, Shift),
            ';' => s(39, Plain),
            ':' => s(39, Shift),
            '\'' => s(40, Plain),
            '"' => s(40, Shift),
            '`' => s(41, Plain),
            '~' => s(41, Shift),
            '\\' => s(43, Plain),
            '|' => s(43, Shift),
            ',' => s(51, Plain),
            '<' => s(51, Shift),
            '.' => s(52, Plain),
            '>' => s(52, Shift),
            '/' => s(53, Plain),
            '?' => s(53, Shift),
            _ => return None,
        }),
    }
}

/// A named key (see `HOST_KEYS` in the profile module) as a key code.
fn named(name: &str) -> Option<u16> {
    Some(match name {
        "ctrl" => 29,
        "shift" => SHIFT,
        "alt" => 56,
        "altgr" => ALTGR,
        "win" => 125,
        "enter" => 28,
        "esc" => 1,
        "tab" => 15,
        "space" => SPACE,
        "backspace" => 14,
        "delete" => 111,
        "insert" => 110,
        "home" => 102,
        "end" => 107,
        "pgup" => 104,
        "pgdn" => 109,
        "up" => 103,
        "down" => 108,
        "left" => 105,
        "right" => 106,
        "capslock" => 58,
        "numlock" => 69,
        "scrolllock" => 70,
        "prtsc" => 99,
        "pause" => 119,
        "menu" => 127,
        "mute" => 113,
        "volup" => 115,
        "voldown" => 114,
        "play" => 164,
        "next" => 163,
        "prev" => 165,
        "stop" => 166,
        _ => {
            let n = name.strip_prefix('f')?.parse::<u16>().ok()?;
            return match n {
                1..=10 => Some(58 + n),
                11 => Some(87),
                12 => Some(88),
                13..=24 => Some(170 + n),
                _ => None,
            };
        }
    })
}

/// The keys of a combination like `ctrl+shift+t`, in press order. A single
/// character is its key on `layout`, with Shift or AltGr added when the
/// layout needs them (`ctrl+?` on a Swedish layout is Ctrl+Shift++).
pub fn chord(layout: HostLayout, text: &str) -> Result<Vec<u16>, String> {
    let mut keys = Vec::new();
    for part in text.split('+').map(str::trim) {
        let lower = part.to_lowercase();
        if let Some(code) = named(&lower) {
            keys.push(code);
            continue;
        }
        let mut chars = lower.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(format!("unknown key {part:?} in {text:?}"));
        };
        let st = stroke(layout, c)
            .ok_or_else(|| format!("{c:?} in {text:?} has no key on the {layout:?} layout"))?;
        if st.dead {
            return Err(format!(
                "{c:?} in {text:?} is a dead key on the {layout:?} layout"
            ));
        }
        match st.level {
            Level::Plain => {}
            Level::Shift => keys.push(SHIFT),
            Level::AltGr => keys.push(ALTGR),
        }
        keys.push(st.code);
    }
    let mut seen = Vec::new();
    for k in &keys {
        if seen.contains(k) {
            return Err(format!("{text:?} names the same key twice"));
        }
        seen.push(*k);
    }
    Ok(keys)
}

/// Press and release a combination.
pub fn tap(layout: HostLayout, text: &str) -> Result<Vec<Event>, String> {
    let keys = chord(layout, text)?;
    let mut out: Vec<Event> = keys.iter().map(|k| Event::Down(*k)).collect();
    out.extend(keys.iter().rev().map(|k| Event::Up(*k)));
    Ok(out)
}

/// Type text, one character after another.
pub fn type_text(layout: HostLayout, text: &str) -> Result<Vec<Event>, String> {
    let mut out = Vec::new();
    for c in text.chars() {
        let st = stroke(layout, c)
            .ok_or_else(|| format!("cannot type {c:?}: the {layout:?} layout has no key for it"))?;
        let modifier = match st.level {
            Level::Plain => None,
            Level::Shift => Some(SHIFT),
            Level::AltGr => Some(ALTGR),
        };
        if let Some(m) = modifier {
            out.push(Event::Down(m));
        }
        out.push(Event::Down(st.code));
        out.push(Event::Up(st.code));
        if let Some(m) = modifier {
            out.push(Event::Up(m));
        }
        if st.dead {
            // Give the desktop time to register the dead key before the
            // space that completes it.
            out.push(Event::SleepMs(DEAD_PAUSE_MS));
            out.push(Event::Down(SPACE));
            out.push(Event::Up(SPACE));
        }
        out.push(Event::SleepMs(TYPE_PAUSE_MS));
    }
    Ok(out)
}

/// Every event of a macro, all repeats included. Fails on the first step
/// the layout cannot type, so the agent can refuse at start.
pub fn compile_macro(layout: HostLayout, m: &Macro) -> Result<Vec<Event>, String> {
    let mut once = Vec::new();
    for step in &m.steps {
        match step {
            MacroStep::Text(t) => once.extend(type_text(layout, t)?),
            MacroStep::Keys(k) => once.extend(tap(layout, k)?),
            MacroStep::Down(k) => once.extend(chord(layout, k)?.into_iter().map(Event::Down)),
            MacroStep::Up(k) => once.extend(chord(layout, k)?.into_iter().rev().map(Event::Up)),
            MacroStep::DelayMs(ms) => once.push(Event::SleepMs(*ms)),
        }
    }
    let mut out = Vec::with_capacity(once.len() * m.repeat as usize);
    for _ in 0..m.repeat {
        out.extend_from_slice(&once);
    }
    Ok(out)
}

/// Every key code the agent may ever press, for registering the virtual
/// keyboard.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn all_codes() -> Vec<u16> {
    let mut v: Vec<u16> = (1..=127).collect();
    v.extend(163..=166);
    v.extend(183..=194);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event::*;

    #[test]
    fn chords() {
        assert_eq!(
            chord(HostLayout::Se, "ctrl+shift+esc").unwrap(),
            vec![29, 42, 1]
        );
        assert_eq!(chord(HostLayout::Se, "f5").unwrap(), vec![63]);
        assert_eq!(chord(HostLayout::Se, "f13").unwrap(), vec![183]);
        assert_eq!(chord(HostLayout::Se, "win+e").unwrap(), vec![125, 18]);
        // `?` needs Shift on both layouts, on different keys.
        assert_eq!(chord(HostLayout::Se, "ctrl+?").unwrap(), vec![29, 42, 12]);
        assert_eq!(chord(HostLayout::Us, "ctrl+?").unwrap(), vec![29, 42, 53]);
        assert!(chord(HostLayout::Se, "shift+!").is_err());
        assert!(chord(HostLayout::Se, "ctrl+^").is_err());
        assert!(chord(HostLayout::Se, "hyper+x").is_err());
    }

    #[test]
    fn tap_releases_in_reverse() {
        assert_eq!(
            tap(HostLayout::Se, "ctrl+c").unwrap(),
            vec![Down(29), Down(46), Up(46), Up(29)]
        );
    }

    #[test]
    fn swedish_text() {
        assert_eq!(
            type_text(HostLayout::Se, "Hå@").unwrap(),
            vec![
                Down(SHIFT),
                Down(35),
                Up(35),
                Up(SHIFT),
                SleepMs(TYPE_PAUSE_MS),
                Down(26),
                Up(26),
                SleepMs(TYPE_PAUSE_MS),
                Down(ALTGR),
                Down(3),
                Up(3),
                Up(ALTGR),
                SleepMs(TYPE_PAUSE_MS),
            ]
        );
        // A dead key is followed by a space.
        assert_eq!(
            type_text(HostLayout::Se, "~").unwrap(),
            vec![
                Down(ALTGR),
                Down(27),
                Up(27),
                Up(ALTGR),
                SleepMs(DEAD_PAUSE_MS),
                Down(SPACE),
                Up(SPACE),
                SleepMs(TYPE_PAUSE_MS)
            ]
        );
        assert_eq!(
            type_text(HostLayout::SeNodeadkeys, "~").unwrap(),
            vec![
                Down(ALTGR),
                Down(27),
                Up(27),
                Up(ALTGR),
                SleepMs(TYPE_PAUSE_MS)
            ]
        );
        assert_eq!(
            chord(HostLayout::SeNodeadkeys, "ctrl+^").unwrap(),
            vec![29, 42, 27]
        );
        assert!(type_text(HostLayout::Se, "é").is_err());
        assert!(type_text(HostLayout::Us, "å").is_err());
    }

    #[test]
    fn every_printable_ascii_is_typeable() {
        for layout in [HostLayout::Se, HostLayout::SeNodeadkeys, HostLayout::Us] {
            for b in 0x20u8..0x7f {
                let c = b as char;
                assert!(stroke(layout, c).is_some(), "{layout:?} {c:?}");
            }
        }
    }

    #[test]
    fn macro_repeats() {
        let m = Macro {
            repeat: 2,
            steps: vec![
                MacroStep::Down("shift".into()),
                MacroStep::Keys("a".into()),
                MacroStep::Up("shift".into()),
                MacroStep::DelayMs(5),
            ],
        };
        let ev = compile_macro(HostLayout::Se, &m).unwrap();
        let once = vec![Down(42), Down(30), Up(30), Up(42), SleepMs(5)];
        assert_eq!(ev, [once.clone(), once].concat());
    }

    #[test]
    fn codes_cover_everything() {
        let all = all_codes();
        for layout in [HostLayout::Se, HostLayout::SeNodeadkeys, HostLayout::Us] {
            for b in 0x20u8..0x7f {
                let st = stroke(layout, b as char).unwrap();
                assert!(all.contains(&st.code));
            }
        }
        for n in 1..=24 {
            assert!(all.contains(&named(&format!("f{n}")).unwrap()));
        }
        for name in ["play", "next", "prev", "stop", "menu", "win", "altgr"] {
            assert!(all.contains(&named(name).unwrap()), "{name}");
        }
    }
}
