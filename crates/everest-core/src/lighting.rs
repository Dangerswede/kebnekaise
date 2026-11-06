//! Phase 2 lighting: turns a request into an ordered plan of packets.
//!
//! [`LightingPlan::dry_run`] passes every packet through a [`Session`] with
//! [`Policy::Lighting`] over a [`DryRunTransport`], so the allowlist is
//! exercised but nothing reaches the keyboard. [`crate::Keyboard::apply_lighting`]
//! sends a plan for real. The CLI only allows that for a static colour
//! without saving (approved by the project owner on 2026-11-06).
//!
//! The sequences and pauses follow everest-mac (MIT), which follows Base
//! Camp; see "Lighting (phase 2)" in `docs/PROTOCOL.md`.

use std::time::Duration;

use everest_hid::{DryRunTransport, Policy, Session};
use everest_proto::lighting::Rgb;
use everest_proto::{
    Classification, CustomFrame, EffectSettings, FirmwareInfo, LightingError, Packet, PACKET_SIZE,
};
use serde::Serialize;

/// How long [`crate::Keyboard::apply_lighting`] keeps reading `11 00`
/// back before reporting the write as unverified.
pub const VERIFY_FOR: Duration = Duration::from_secs(3);

/// What a real run did.
#[derive(Clone, Debug, Serialize)]
pub struct ApplyOutcome {
    pub packets_sent: usize,
    /// `11 00` as read after the write.
    pub firmware_after: FirmwareInfo,
    /// The expected profile and slot were active afterwards.
    pub verified: bool,
}

/// Lighting slot of custom (per-key) mode.
pub const CUSTOM_SLOT: u8 = 6;

/// One packet of a plan and how long to wait after it.
#[derive(Clone, Debug)]
pub struct Step {
    pub packet: Packet,
    pub wait_after: Duration,
}

/// What the keyboard should report afterwards (`11 00`), so a real run can
/// verify the write by reading it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Expected {
    pub profile: u8,
    pub lighting_slot: u8,
}

/// An ordered list of lighting packets.
#[derive(Clone, Debug)]
pub struct LightingPlan {
    pub steps: Vec<Step>,
    pub expected: Expected,
    /// Whether the plan ends with `13 55` (save to flash).
    pub saves: bool,
}

const AFTER_SWITCH: Duration = Duration::from_millis(400);
const AFTER_EFFECT: Duration = Duration::from_millis(250);
const AFTER_FRAME_PACKET: Duration = Duration::from_millis(30);
const AFTER_BIND: Duration = Duration::from_millis(100);
const AFTER_SAVE: Duration = Duration::from_millis(500);

fn step(packet: Packet, wait_after: Duration) -> Step {
    Step { packet, wait_after }
}

/// Scales a colour by `brightness` percent (custom mode applies
/// brightness on the host, as everest-mac does).
fn dim(c: Rgb, brightness: u8) -> Rgb {
    let f = |v: u8| ((u16::from(v) * u16::from(brightness) + 50) / 100) as u8;
    Rgb::new(f(c.r), f(c.g), f(c.b))
}

impl LightingPlan {
    /// A built-in effect: switch to its slot, send the parameters, and
    /// (when `save`) save the slot to flash, as Base Camp does.
    pub fn effect(
        profile: u8,
        settings: &EffectSettings,
        save: bool,
    ) -> Result<Self, LightingError> {
        let slot = settings.effect().slot();
        let mut steps = vec![
            step(Packet::switch_slot(profile, slot)?, AFTER_SWITCH),
            step(Packet::effect(settings), AFTER_EFFECT),
        ];
        if save {
            steps.push(step(Packet::save_slot(slot), AFTER_SAVE));
        }
        Ok(LightingPlan {
            steps,
            expected: Expected {
                profile,
                lighting_slot: slot,
            },
            saves: save,
        })
    }

    /// Custom (per-key) lighting: switch to slot 6, enter custom mode, send
    /// the 11 frame packets, bind the keys, and (when `save`) save slot 6.
    pub fn custom(
        profile: u8,
        frame: &CustomFrame,
        brightness: u8,
        save: bool,
    ) -> Result<Self, LightingError> {
        if brightness > 100 {
            return Err(LightingError::BadBrightness(brightness));
        }
        let mut dimmed = frame.clone();
        for c in dimmed.keys.iter_mut().chain(dimmed.side.iter_mut()) {
            *c = dim(*c, brightness);
        }
        let mut steps = vec![
            step(Packet::switch_slot(profile, CUSTOM_SLOT)?, AFTER_SWITCH),
            step(Packet::enter_custom_mode(100)?, AFTER_SWITCH),
        ];
        steps.extend(
            Packet::custom_frame(&dimmed, 100)?
                .into_iter()
                .map(|p| step(p, AFTER_FRAME_PACKET)),
        );
        steps.extend((0..3).map(|chunk| step(Packet::bind_keys(chunk), AFTER_BIND)));
        if save {
            steps.push(step(Packet::save_slot(CUSTOM_SLOT), AFTER_SAVE));
        }
        Ok(LightingPlan {
            steps,
            expected: Expected {
                profile,
                lighting_slot: CUSTOM_SLOT,
            },
            saves: save,
        })
    }

    /// Runs the plan through the lighting allowlist without a device.
    /// Returns each packet with its classification, or the first refusal.
    pub fn dry_run(&self) -> everest_hid::Result<Vec<DryRunStep>> {
        let mut session = Session::new(DryRunTransport::default(), Policy::Lighting);
        let mut out = Vec::with_capacity(self.steps.len());
        for s in &self.steps {
            session.send(&s.packet)?;
            out.push(DryRunStep {
                bytes: *s.packet.as_bytes(),
                classification: s.packet.classification(),
                wait_after: s.wait_after,
            });
        }
        debug_assert_eq!(session.into_transport().written().len(), out.len());
        Ok(out)
    }

    /// Total pause time of the plan.
    pub fn duration(&self) -> Duration {
        self.steps.iter().map(|s| s.wait_after).sum()
    }
}

/// One packet as it would be sent.
#[derive(Clone, Debug, Serialize)]
pub struct DryRunStep {
    #[serde(serialize_with = "hex_packet")]
    pub bytes: [u8; PACKET_SIZE],
    pub classification: Classification,
    #[serde(serialize_with = "millis")]
    pub wait_after: Duration,
}

fn hex_packet<S: serde::Serializer>(b: &[u8; PACKET_SIZE], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&crate::hex(trimmed(b)))
}

fn millis<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(d.as_millis() as u64)
}

/// A packet up to its last non-zero byte (at least the command bytes).
pub fn trimmed(b: &[u8]) -> &[u8] {
    let end = b.iter().rposition(|&x| x != 0).map_or(2, |i| i + 1).max(2);
    &b[..end]
}

#[cfg(test)]
mod tests {
    use everest_proto::{Effect, Group, SafetyClass};

    use super::*;

    fn static_red(brightness: u8) -> EffectSettings {
        EffectSettings::new(
            Effect::Static,
            None,
            vec![Rgb::new(255, 0, 0)],
            None,
            brightness,
            None,
        )
        .unwrap()
    }

    #[test]
    fn effect_plan_matches_base_camp_order() {
        let plan = LightingPlan::effect(2, &static_red(100), true).unwrap();
        let steps = plan.dry_run().unwrap();
        let hex: Vec<_> = steps
            .iter()
            .map(|s| crate::hex(trimmed(&s.bytes)))
            .collect();
        assert_eq!(
            hex,
            ["14 00 00 00 02", "14 2c 00 00 ff 64 00 ff ff ff", "13 55",]
        );
        assert_eq!(steps[2].classification.class, SafetyClass::Persist);
        assert_eq!(
            plan.expected,
            Expected {
                profile: 2,
                lighting_slot: 0
            }
        );
    }

    #[test]
    fn effect_plan_without_save() {
        let plan = LightingPlan::effect(1, &static_red(50), false).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert!(!plan.saves);
        assert!(plan
            .dry_run()
            .unwrap()
            .iter()
            .all(|s| s.classification.class != SafetyClass::Persist
                || s.classification.name == "effect parameters"));
    }

    #[test]
    fn bad_profile_builds_nothing() {
        assert_eq!(
            LightingPlan::effect(6, &static_red(100), true).unwrap_err(),
            LightingError::BadProfile(6)
        );
    }

    #[test]
    fn custom_plan() {
        let mut frame = CustomFrame::filled(Rgb::new(200, 100, 0), Rgb::BLACK);
        frame.keys[0] = Rgb::new(255, 255, 255);
        let plan = LightingPlan::custom(1, &frame, 50, false).unwrap();
        let steps = plan.dry_run().unwrap();
        // switch, enter custom, 8 key + 3 side packets, 3 binds.
        assert_eq!(steps.len(), 16);
        assert!(steps
            .iter()
            .all(|s| s.classification.group == Group::Lighting));
        assert_eq!(crate::hex(trimmed(&steps[0].bytes)), "14 00 00 00 01 06");
        // Brightness is applied to the colours: 255 * 50 % = 128, 200 -> 100.
        assert_eq!(steps[2].bytes[7..13], [128, 128, 128, 100, 50, 0]);
        assert_eq!(plan.expected.lighting_slot, CUSTOM_SLOT);

        let saved = LightingPlan::custom(1, &frame, 100, true).unwrap();
        let last = saved.steps.last().unwrap();
        assert_eq!(format!("{:?}", last.packet), "Packet[13 55 00 00 06]");
    }
}
