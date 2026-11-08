//! Profile files (phase 4).
//!
//! The keyboard has no read-back for key remaps, display-key actions,
//! lighting or pictures, so a profile file on the computer is the source of
//! truth. The format is TOML and identical on Linux and Windows; it is
//! described in `docs/PROFILES.md`.
//!
//! This module only parses, validates and plans. Nothing here talks to the
//! keyboard: [`Profile::plan`] returns the packets a later `profile apply`
//! would send, so they can be reviewed with `--dry-run` first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use everest_proto::{
    ActionText, ColorMode, DKey, Direction, Effect, EffectSettings, KeyPos, Packet, Rgb, StoredKind,
};
use serde::{Deserialize, Serialize};

use crate::lighting::LightingPlan;

/// The only format version this build reads and writes.
pub const FORMAT: u32 = 1;

/// Pause after each remap or action packet. The keyboard can need up to
/// ~3 s after a settings write; these are not settings writes in that sense,
/// and the value is a guess to be checked on hardware.
pub const KEY_PACKET_PAUSE_MS: u64 = 150;

/// A profile file as written on disk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Format version, must be [`FORMAT`].
    pub format: u32,
    /// Free text shown to the user.
    pub name: String,
    /// The keyboard profile (1..=5) this file is written to.
    pub hardware_profile: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lighting: Option<LightingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pictures: Option<PictureSpec>,
    /// Physical key position = position it should act as.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remap: BTreeMap<String, String>,
    /// `d1`..`d4` = what the agent does when the key is pressed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dkeys: BTreeMap<String, DKeyAction>,
    /// Named macros, played by the agent on the computer.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub macros: BTreeMap<String, Macro>,
}

/// A built-in effect, with the same options as `kebnekaise rgb`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingSpec {
    pub effect: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<u8>,
    #[serde(default = "full_brightness")]
    pub brightness: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

fn full_brightness() -> u8 {
    100
}

/// Picture files, relative to the profile file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PictureSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d1: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d2: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d3: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d4: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dial: Option<PathBuf>,
}

impl PictureSpec {
    /// (target name, path) for every picture given.
    pub fn entries(&self) -> Vec<(&'static str, &Path)> {
        [
            ("d1", &self.d1),
            ("d2", &self.d2),
            ("d3", &self.d3),
            ("d4", &self.d4),
            ("dial", &self.dial),
        ]
        .into_iter()
        .filter_map(|(n, p)| p.as_deref().map(|p| (n, p)))
        .collect()
    }
}

/// What a display key does. Run by the agent, not by the keyboard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "target", rename_all = "snake_case")]
pub enum DKeyAction {
    /// Open a URL in the default browser.
    Url(String),
    /// Start a program (no shell): first word is the program, the rest
    /// arguments; use the list form for arguments with spaces.
    Run(Command),
    /// Press a key combination, e.g. `ctrl+shift+esc`.
    Keys(String),
    /// Play a macro from `[macros]`.
    Macro(String),
    /// Switch the keyboard to another profile (1..=5).
    Profile(u8),
}

/// A program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Command {
    Line(String),
    Argv(Vec<String>),
}

impl Command {
    pub fn argv(&self) -> Vec<String> {
        match self {
            Command::Line(s) => s.split_whitespace().map(String::from).collect(),
            Command::Argv(v) => v.clone(),
        }
    }
}

/// A macro: steps played in order, `repeat` times.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Macro {
    #[serde(default = "once")]
    pub repeat: u32,
    pub steps: Vec<MacroStep>,
}

fn once() -> u32 {
    1
}

/// One macro step. Exactly one field per step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroStep {
    /// Type text (any Unicode).
    Text(String),
    /// Press and release a key combination.
    Keys(String),
    /// Hold a key down until a later `up`.
    Down(String),
    /// Release a held key.
    Up(String),
    /// Wait, in milliseconds.
    DelayMs(u32),
}

/// Most steps a macro may have, and the longest delay, so that a typo
/// cannot keep the agent busy for hours.
pub const MAX_MACRO_STEPS: usize = 1000;
pub const MAX_DELAY_MS: u32 = 60_000;
pub const MAX_REPEAT: u32 = 100;

/// Key names the agent can press on the computer (not keyboard positions).
const HOST_KEYS: &[&str] = &[
    "ctrl",
    "shift",
    "alt",
    "altgr",
    "win",
    "enter",
    "esc",
    "tab",
    "space",
    "backspace",
    "delete",
    "insert",
    "home",
    "end",
    "pgup",
    "pgdn",
    "up",
    "down",
    "left",
    "right",
    "capslock",
    "numlock",
    "scrolllock",
    "prtsc",
    "pause",
    "menu",
    "mute",
    "volup",
    "voldown",
    "play",
    "next",
    "prev",
    "stop",
];

/// Checks a key combination like `ctrl+shift+esc`.
pub fn check_chord(chord: &str) -> Result<(), String> {
    let parts: Vec<&str> = chord.split('+').map(str::trim).collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return Err(format!("bad key combination {chord:?}"));
    }
    for p in parts {
        let l = p.to_lowercase();
        let single_char = l.chars().count() == 1;
        let fkey = l
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=24).contains(&n));
        if !(single_char || fkey || HOST_KEYS.contains(&l.as_str())) {
            return Err(format!("unknown key {p:?} in {chord:?}"));
        }
    }
    Ok(())
}

/// Everything that can be wrong with a profile file. All problems are
/// collected, not just the first.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid profile:\n  {}", .0.join("\n  "))]
    Invalid(Vec<String>),
}

/// The checked contents of a profile, ready to plan.
#[derive(Clone, Debug)]
pub struct Checked {
    pub lighting: Option<EffectSettings>,
    pub remaps: Vec<(KeyPos, KeyPos)>,
    pub dkeys: Vec<DKey>,
}

/// One packet of a profile plan, with a note for the dry run.
#[derive(Clone, Debug)]
pub struct PlanStep {
    pub packet: Packet,
    pub wait_after_ms: u64,
    pub note: String,
}

impl Profile {
    /// Reads and validates a file.
    pub fn load(path: &Path) -> Result<(Self, Checked), ProfileError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| ProfileError::Io(path.to_path_buf(), e))?;
        let profile = Self::parse(&text)?;
        let checked = profile.check(path.parent())?;
        Ok((profile, checked))
    }

    pub fn parse(text: &str) -> Result<Self, ProfileError> {
        Ok(toml::from_str(text)?)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("a profile always serialises")
    }

    /// Validates everything. `base` is the folder picture paths are relative
    /// to; `None` skips the file-exists check.
    pub fn check(&self, base: Option<&Path>) -> Result<Checked, ProfileError> {
        let mut errors = Vec::new();
        if self.format != FORMAT {
            errors.push(format!(
                "format = {}: this build reads format {FORMAT}",
                self.format
            ));
        }
        if !(1..=5).contains(&self.hardware_profile) {
            errors.push(format!(
                "hardware_profile = {}: must be 1..5",
                self.hardware_profile
            ));
        }

        let lighting = self.lighting.as_ref().and_then(|l| match l.settings() {
            Ok(s) => Some(s),
            Err(e) => {
                errors.push(format!("[lighting]: {e}"));
                None
            }
        });

        if let (Some(p), Some(base)) = (&self.pictures, base) {
            for (name, path) in p.entries() {
                if !base.join(path).is_file() {
                    errors.push(format!("[pictures] {name}: {} not found", path.display()));
                }
            }
        }

        let mut remaps = Vec::new();
        for (from, to) in &self.remap {
            match (KeyPos::from_name(from), KeyPos::from_name(to)) {
                (Ok(f), Ok(t)) => {
                    if remaps.iter().any(|(k, _): &(KeyPos, KeyPos)| *k == f) {
                        errors.push(format!("[remap] {f} is given twice"));
                    }
                    remaps.push((f, t));
                }
                (Err(e), _) | (_, Err(e)) => errors.push(format!("[remap] {from} = {to}: {e}")),
            }
        }

        let mut dkeys = Vec::new();
        for (name, action) in &self.dkeys {
            match name.parse::<DKey>() {
                Ok(k) => dkeys.push(k),
                Err(e) => errors.push(format!("[dkeys.{name}]: {e}")),
            }
            if let Err(e) = self.check_action(action) {
                errors.push(format!("[dkeys.{name}]: {e}"));
            }
        }

        for (name, m) in &self.macros {
            if let Err(e) = check_macro(m) {
                errors.push(format!("[macros.{name}]: {e}"));
            }
        }

        if errors.is_empty() {
            Ok(Checked {
                lighting,
                remaps,
                dkeys,
            })
        } else {
            Err(ProfileError::Invalid(errors))
        }
    }

    fn check_action(&self, action: &DKeyAction) -> Result<(), String> {
        match action {
            DKeyAction::Url(u) => {
                if !(u.starts_with("https://") || u.starts_with("http://")) {
                    return Err(format!("url {u:?} must start with http:// or https://"));
                }
            }
            DKeyAction::Run(c) => {
                if c.argv().is_empty() {
                    return Err("run: empty command".into());
                }
            }
            DKeyAction::Keys(k) => check_chord(k)?,
            DKeyAction::Macro(m) => {
                if !self.macros.contains_key(m) {
                    return Err(format!("macro {m:?} is not defined under [macros]"));
                }
            }
            DKeyAction::Profile(p) => {
                if !(1..=5).contains(p) {
                    return Err(format!("profile {p} must be 1..5"));
                }
            }
        }
        Ok(())
    }

    /// The packets `profile apply` would send, in order: lighting (which
    /// also makes `hardware_profile` active), remaps, display-key markers.
    /// Pictures are uploaded separately and are not part of this list.
    pub fn plan(&self, checked: &Checked) -> Result<Vec<PlanStep>, everest_proto::LightingError> {
        let mut steps = Vec::new();
        if let Some(settings) = &checked.lighting {
            let plan = LightingPlan::effect(self.hardware_profile, settings, true)?;
            for s in plan.steps {
                steps.push(PlanStep {
                    note: format!("lighting: {}", s.packet.classification().name),
                    wait_after_ms: s.wait_after.as_millis() as u64,
                    packet: s.packet,
                });
            }
        }
        for (from, to) in &checked.remaps {
            steps.push(PlanStep {
                packet: Packet::remap(*from, *to),
                wait_after_ms: KEY_PACKET_PAUSE_MS,
                note: if from == to {
                    format!("remap: {from} back to itself")
                } else {
                    format!("remap: {from} acts as {to}")
                },
            });
        }
        for key in &checked.dkeys {
            steps.push(PlanStep {
                packet: Packet::action_slot(*key),
                wait_after_ms: KEY_PACKET_PAUSE_MS,
                note: format!("{key}: select action slot"),
            });
            steps.push(PlanStep {
                packet: Packet::action_text(StoredKind::Url, &ActionText::marker(*key)),
                wait_after_ms: KEY_PACKET_PAUSE_MS,
                note: format!("{key}: store marker (the agent runs the action)"),
            });
        }
        Ok(steps)
    }

    /// A commented example to start from.
    pub fn template() -> &'static str {
        TEMPLATE
    }
}

impl LightingSpec {
    pub fn settings(&self) -> Result<EffectSettings, everest_proto::LightingError> {
        let effect: Effect = self.effect.parse()?;
        let mode = self
            .mode
            .as_deref()
            .map(str::parse::<ColorMode>)
            .transpose()?;
        let colors = self
            .colors
            .iter()
            .map(|c| c.parse::<Rgb>())
            .collect::<Result<Vec<_>, _>>()?;
        let direction = self
            .direction
            .as_deref()
            .map(str::parse::<Direction>)
            .transpose()?;
        EffectSettings::new(effect, mode, colors, self.speed, self.brightness, direction)
    }
}

fn check_macro(m: &Macro) -> Result<(), String> {
    if m.steps.is_empty() {
        return Err("no steps".into());
    }
    if m.steps.len() > MAX_MACRO_STEPS {
        return Err(format!("{} steps (max {MAX_MACRO_STEPS})", m.steps.len()));
    }
    if !(1..=MAX_REPEAT).contains(&m.repeat) {
        return Err(format!("repeat = {} (1..{MAX_REPEAT})", m.repeat));
    }
    let mut held: Vec<String> = Vec::new();
    for step in &m.steps {
        match step {
            MacroStep::Text(t) if t.is_empty() => return Err("empty text step".into()),
            MacroStep::Text(_) => {}
            MacroStep::Keys(k) => check_chord(k)?,
            MacroStep::Down(k) => {
                check_chord(k)?;
                held.push(k.to_lowercase());
            }
            MacroStep::Up(k) => {
                let k = k.to_lowercase();
                match held.iter().position(|h| *h == k) {
                    Some(i) => {
                        held.remove(i);
                    }
                    None => return Err(format!("up {k:?} without a down")),
                }
            }
            MacroStep::DelayMs(d) if *d > MAX_DELAY_MS => {
                return Err(format!("delay_ms = {d} (max {MAX_DELAY_MS})"))
            }
            MacroStep::DelayMs(_) => {}
        }
    }
    if !held.is_empty() {
        return Err(format!("keys still held at the end: {}", held.join(", ")));
    }
    Ok(())
}

const TEMPLATE: &str = r#"# Kebnekaise profile. Same file on Linux and Windows.
# Check it with:  kebnekaise profile check <file>
# See the packets: kebnekaise profile apply <file> --dry-run
format = 1
name = "Example"
hardware_profile = 1          # the keyboard profile (1..5) this file is for

# Optional: a built-in effect, same options as `kebnekaise rgb effect`.
[lighting]
effect = "static"
colors = ["ff8800"]
brightness = 80

# Optional: pictures, paths relative to this file (uploaded by `picture`).
# [pictures]
# d1 = "icons/mail.png"
# dial = "dial.png"

# Key positions (US names, Nordic legends like ö/ä/å also work):
# the key on the left acts as the key on the right.
# `kebnekaise key names` lists them. A key mapped to itself is restored.
[remap]
caps = "lctrl"

# Display keys. The keyboard only reports the press; `kebnekaise agent`
# runs the action on the computer.
[dkeys.d1]
action = "url"
target = "https://github.com/Dangerswede/kebnekaise"

[dkeys.d2]
action = "keys"
target = "ctrl+shift+esc"

[dkeys.d3]
action = "macro"
target = "greeting"

[dkeys.d4]
action = "profile"
target = 2

# Macros are played by the agent: text, keys, down/up, delay_ms.
[macros.greeting]
repeat = 1
steps = [
  { text = "Hej!" },
  { delay_ms = 100 },
  { keys = "enter" },
]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_valid_and_plans() {
        let p = Profile::parse(Profile::template()).unwrap();
        let checked = p.check(None).unwrap();
        assert_eq!(checked.remaps.len(), 1);
        assert_eq!(checked.dkeys.len(), 4);
        let plan = p.plan(&checked).unwrap();
        // Every packet in the plan passes the classifier.
        for s in &plan {
            assert_ne!(
                s.packet.classification().class,
                everest_proto::SafetyClass::Deny,
                "{}",
                s.note
            );
        }
        // 3 lighting packets, 1 remap, 4 x 2 action packets.
        assert_eq!(plan.len(), 3 + 1 + 8);
    }

    #[test]
    fn round_trip() {
        let p = Profile::parse(Profile::template()).unwrap();
        let again = Profile::parse(&p.to_toml()).unwrap();
        assert_eq!(p, again);
    }

    #[test]
    fn errors_are_collected() {
        let text = r#"
format = 2
name = "x"
hardware_profile = 9
[remap]
caps = "esc"
[dkeys.d5]
action = "url"
target = "file:///etc/passwd"
[dkeys.d1]
action = "macro"
target = "missing"
"#;
        let err = Profile::parse(text).unwrap().check(None).unwrap_err();
        let ProfileError::Invalid(list) = err else {
            panic!("expected Invalid")
        };
        assert_eq!(list.len(), 6, "{list:#?}");
    }

    #[test]
    fn unknown_fields_rejected() {
        let text = "format = 1\nname = \"x\"\nhardware_profile = 1\ncolour = 3\n";
        assert!(Profile::parse(text).is_err());
    }

    #[test]
    fn macro_rules() {
        let ok = Macro {
            repeat: 1,
            steps: vec![
                MacroStep::Down("shift".into()),
                MacroStep::Keys("a".into()),
                MacroStep::Up("shift".into()),
            ],
        };
        assert!(check_macro(&ok).is_ok());
        let held = Macro {
            repeat: 1,
            steps: vec![MacroStep::Down("shift".into())],
        };
        assert!(check_macro(&held).is_err());
        assert!(check_chord("ctrl+alt+delete").is_ok());
        assert!(check_chord("ctrl++").is_err());
        assert!(check_chord("hyper+x").is_err());
        assert!(check_chord("f13").is_ok());
    }

    #[test]
    fn run_command_forms() {
        let text = r#"
format = 1
name = "x"
hardware_profile = 1
[dkeys.d1]
action = "run"
target = ["C:\\Program Files\\App\\app.exe", "--flag"]
[dkeys.d2]
action = "run"
target = "notepad"
"#;
        let p = Profile::parse(text).unwrap();
        p.check(None).unwrap();
        assert_eq!(
            p.dkeys["d1"],
            DKeyAction::Run(Command::Argv(vec![
                "C:\\Program Files\\App\\app.exe".into(),
                "--flag".into()
            ]))
        );
    }
}
