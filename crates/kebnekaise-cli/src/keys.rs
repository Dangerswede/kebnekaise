//! `kebnekaise key ...` and `kebnekaise profile ...` (phase 4).
//!
//! Remaps (`14 20`) between keys in the position table are approved for
//! hardware (first test 2026-11-08, all table keys the same day). Display-key
//! actions (`12 08` + `17 AA`) are not: `key arm` requires `--dry-run`, and a
//! real `profile apply` sends lighting and remaps but skips the markers.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Subcommand;
use everest_core::hex;
use everest_core::lighting::trimmed;
use everest_core::profile::{Checked, PlanStep, Profile};
use everest_proto::keymap::{all_keys, nordic_legend, Shape};
use everest_proto::{ActionText, DKey, KeyPos, Packet, StoredKind};

use everest_core::lighting::LightingPlan;
use everest_core::{DeviceReport, Keyboard};
use everest_hid::coexist::find_conflicts;
use everest_hid::discovery::{list_mountain_interfaces, select_command_channel};
use everest_hid::session::QueryOptions;
use everest_hid::{HidTransport, TracingTransport};

use crate::{hid_api, print_conflicts, CliResult};

/// Options for a real run.
#[derive(clap::Args)]
pub struct HwArgs {
    /// Command interface to open, as printed by `list`.
    #[arg(long, value_name = "PATH")]
    device: Option<String>,
    /// Write even on untested firmware.
    #[arg(long)]
    force: bool,
    /// Log every packet written and read to stderr.
    #[arg(long)]
    debug: bool,
    /// How long to wait for each reply, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 800)]
    timeout_ms: u64,
}

#[derive(Subcommand)]
pub enum KeyCommand {
    /// List the key positions remaps accept, with their code.
    Names,
    /// Make one key act as another (`14 20`). Stored in the keyboard's flash.
    Remap {
        /// Key to change (position name, Nordic legend or 0xNN).
        key: String,
        /// Key it should act as.
        to: String,
        /// Print the packet instead of sending it.
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        hw: HwArgs,
    },
    /// Undo a remap by mapping the key to itself.
    Restore {
        /// Key to restore, or `all` for every key in `key names`.
        key: String,
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        hw: HwArgs,
    },
    /// Arm a display key so the keyboard reports its presses (`12 08` +
    /// `17 AA`). Dry run only for now.
    Arm {
        /// D1..D4.
        key: String,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// Print a commented example profile.
    Template {
        /// Write it to this file instead of stdout (refuses to overwrite).
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Check a profile file and list every problem.
    Check { file: PathBuf },
    /// Show what a profile file contains.
    Show { file: PathBuf },
    /// Write a profile's lighting and remaps to the keyboard. Display-key
    /// markers are not sent yet.
    Apply {
        file: PathBuf,
        /// Print the packets instead of sending them.
        #[arg(long)]
        dry_run: bool,
        /// Print JSON instead of text (dry run only).
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        hw: HwArgs,
    },
}

fn not_approved(what: &str) -> CliResult {
    eprintln!("error: {what} may not write to the keyboard yet.");
    eprintln!("It is waiting for its first hardware test. Add --dry-run to see the packets.");
    Ok(ExitCode::from(4))
}

fn print_steps(steps: &[PlanStep]) {
    println!("Dry run (nothing is sent to the keyboard):\n");
    for (i, s) in steps.iter().enumerate() {
        let c = s.packet.classification();
        println!(
            "{:>2}. {:<46} {:<8} wait {:>3} ms  {}",
            i + 1,
            s.note,
            format!("{:?}", c.class).to_lowercase(),
            s.wait_after_ms,
            hex(trimmed(s.packet.as_bytes())),
        );
    }
}

fn one(packet: Packet, note: String) -> PlanStep {
    PlanStep {
        packet,
        wait_after_ms: everest_core::profile::KEY_PACKET_PAUSE_MS,
        note,
    }
}

pub fn run_key(command: KeyCommand) -> CliResult {
    match command {
        KeyCommand::Names => {
            println!("{:<12} {:<5} {:<7} nordic", "name", "code", "shape");
            for (k, shape) in all_keys() {
                let shape = match shape {
                    Shape::Both => "",
                    Shape::IsoOnly => "ISO",
                    Shape::AnsiOnly => "ANSI",
                };
                println!(
                    "{:<12} 0x{:02x}  {:<7} {}",
                    k.name(),
                    k.code(),
                    shape,
                    nordic_legend(k).unwrap_or("")
                );
            }
            println!(
                "\nCodes from everest-mac and Base Camp's database; grave (§) is verified on hardware."
            );
            Ok(ExitCode::SUCCESS)
        }
        KeyCommand::Remap {
            key,
            to,
            dry_run,
            hw,
        } => {
            let (k, t): (KeyPos, KeyPos) = (key.parse()?, to.parse()?);
            if !dry_run {
                return send_remaps(&[(k, t)], &hw);
            }
            print_steps(&[one(Packet::remap(k, t), format!("remap: {k} acts as {t}"))]);
            Ok(ExitCode::SUCCESS)
        }
        KeyCommand::Restore { key, dry_run, hw } => {
            let keys: Vec<KeyPos> = if key.trim().eq_ignore_ascii_case("all") {
                all_keys().map(|(k, _)| k).collect()
            } else {
                vec![key.parse()?]
            };
            let pairs: Vec<(KeyPos, KeyPos)> = keys.iter().map(|k| (*k, *k)).collect();
            if !dry_run {
                return send_remaps(&pairs, &hw);
            }
            let steps: Vec<_> = pairs
                .iter()
                .map(|(k, _)| one(Packet::remap(*k, *k), format!("remap: {k} back to itself")))
                .collect();
            print_steps(&steps);
            Ok(ExitCode::SUCCESS)
        }
        KeyCommand::Arm { key, dry_run } => {
            let k: DKey = key.parse()?;
            if !dry_run {
                return not_approved("`key arm`");
            }
            print_steps(&[
                one(Packet::action_slot(k), format!("{k}: select action slot")),
                one(
                    Packet::action_text(StoredKind::Url, &ActionText::marker(k)),
                    format!("{k}: store marker"),
                ),
            ]);
            Ok(ExitCode::SUCCESS)
        }
    }
}

type Hw = TracingTransport<HidTransport>;

/// Finds the keyboard, checks for other programs, reads the report and the
/// firmware gate. `Err(code)` means stop with that exit code (the reason is
/// already printed); nothing has been written then.
fn connect(
    hw: &HwArgs,
) -> Result<Result<(Hw, DeviceReport), ExitCode>, Box<dyn std::error::Error>> {
    let api = hid_api()?;
    let interfaces = list_mountain_interfaces(&api);
    let channel = match select_command_channel(&interfaces, hw.device.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Run `kebnekaise list` to see what is connected.");
            return Ok(Err(ExitCode::from(2)));
        }
    };
    let conflicts = find_conflicts(Some(&channel.path));
    if !conflicts.is_empty() {
        print_conflicts(&conflicts);
        eprintln!("Close them and try again.");
        return Ok(Err(ExitCode::from(3)));
    }
    let transport = TracingTransport::new(HidTransport::open(&api, &channel.path)?, hw.debug);
    let mut keyboard = Keyboard::new(transport).with_query_options(query_options(hw));
    let report = match keyboard.read_report(hw.force) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!(
                "Nothing was written. If Base Camp ran recently, unplug and replug the keyboard."
            );
            return Ok(Err(ExitCode::from(1)));
        }
    };
    if !report.write_gate.writes_allowed() {
        eprintln!(
            "error: firmware {} is untested; nothing was written. --force overrides.",
            report.firmware.version
        );
        return Ok(Err(ExitCode::from(5)));
    }
    Ok(Ok((keyboard.into_transport(), report)))
}

fn query_options(hw: &HwArgs) -> QueryOptions {
    QueryOptions {
        timeout: Duration::from_millis(hw.timeout_ms),
        attempts: 3,
    }
}

/// How long to listen for the echo of one remap.
const REMAP_LISTEN: Duration = Duration::from_millis(300);

/// Sends remaps one by one and checks each echo. Returns how many were not
/// echoed. Key events (`01 ...`) can carry keystrokes: they are counted,
/// never printed.
fn remap_all(
    keyboard: &mut Keyboard<Hw>,
    report: &DeviceReport,
    pairs: &[(KeyPos, KeyPos)],
) -> Result<usize, everest_core::Error> {
    let mut missing = 0;
    let mut events = 0;
    for (key, to) in pairs {
        let packet = Packet::remap(*key, *to);
        let seen = keyboard.remap_key(&packet, report.write_gate, REMAP_LISTEN)?;
        events += seen.iter().filter(|p| p[0] == 0x01).count();
        let sent = trimmed(packet.as_bytes());
        let echoed = seen.iter().any(|p| trimmed(p) == sent);
        let what = if key == to {
            format!("{key} back to itself")
        } else {
            format!("{key} acts as {to}")
        };
        if echoed {
            println!("  ok    {:<28} {}", what, hex(sent));
        } else {
            missing += 1;
            println!("  NO ECHO {:<26} {}", what, hex(sent));
            for p in seen.iter().filter(|p| p[0] != 0x01) {
                println!("          received {}", hex(trimmed(p)));
            }
        }
        std::thread::sleep(Duration::from_millis(
            everest_core::profile::KEY_PACKET_PAUSE_MS,
        ));
    }
    if events > 0 {
        println!("  ({events} key event report(s) not shown)");
    }
    Ok(missing)
}

fn active_profile(report: &DeviceReport) -> String {
    report
        .firmware
        .active_profile
        .map(|p| p.to_string())
        .unwrap_or_else(|| "?".into())
}

/// Sends remaps to the real keyboard. Every pair names keys in the position
/// table (parsing guarantees it, and the allowlist checks it again).
fn send_remaps(pairs: &[(KeyPos, KeyPos)], hw: &HwArgs) -> CliResult {
    let (transport, report) = match connect(hw)? {
        Ok(c) => c,
        Err(code) => return Ok(code),
    };
    let mut keyboard = Keyboard::for_keymap(transport).with_query_options(query_options(hw));
    println!(
        "Firmware {}, profile {}: sending {} remap(s).",
        report.firmware.version,
        active_profile(&report),
        pairs.len()
    );
    let missing = remap_all(&mut keyboard, &report, pairs)?;
    if missing > 0 {
        eprintln!("warning: {missing} remap(s) were not echoed; check those keys.");
        return Ok(ExitCode::from(1));
    }
    match pairs {
        [(key, to)] if key == to => {
            println!("Now press {key}: it should type its own character again.")
        }
        [(key, to)] => println!("Now press {key}: it should act as {to}."),
        _ => println!("Done. The keyboard keeps these after a replug."),
    }
    Ok(ExitCode::SUCCESS)
}

/// `profile apply` for real: lighting (which also makes `hardware_profile`
/// active and saves the slot), then remaps. Display-key markers are skipped.
fn apply_profile(p: &Profile, checked: &Checked, hw: &HwArgs) -> CliResult {
    let (transport, report) = match connect(hw)? {
        Ok(c) => c,
        Err(code) => return Ok(code),
    };
    println!(
        "Firmware {}, profile {} active.",
        report.firmware.version,
        active_profile(&report)
    );
    let transport = match &checked.lighting {
        Some(settings) => {
            let plan = LightingPlan::effect(p.hardware_profile, settings, true)?;
            let mut keyboard =
                Keyboard::for_lighting(transport).with_query_options(query_options(hw));
            let outcome = keyboard.apply_lighting(&plan, report.write_gate)?;
            println!(
                "Lighting: sent {} packets; now profile {}, lighting slot {}{}.",
                outcome.packets_sent,
                outcome
                    .firmware_after
                    .active_profile
                    .map_or("?".into(), |n| n.to_string()),
                outcome.firmware_after.lighting_slot,
                if outcome.verified {
                    " (verified, saved)"
                } else {
                    ""
                }
            );
            if !outcome.verified {
                eprintln!(
                    "error: keyboard profile {} did not become active; remaps were not sent.",
                    p.hardware_profile
                );
                return Ok(ExitCode::from(1));
            }
            keyboard.into_transport()
        }
        None => {
            if report.firmware.active_profile != Some(p.hardware_profile)
                && !checked.remaps.is_empty()
            {
                eprintln!(
                    "error: the profile is for keyboard profile {}, but profile {} is active, and \
                     without [lighting] this tool does not switch profiles. Nothing was written.",
                    p.hardware_profile,
                    active_profile(&report)
                );
                return Ok(ExitCode::from(1));
            }
            transport
        }
    };
    let mut missing = 0;
    if !checked.remaps.is_empty() {
        println!("Remaps:");
        let mut keyboard = Keyboard::for_keymap(transport).with_query_options(query_options(hw));
        missing = remap_all(&mut keyboard, &report, &checked.remaps)?;
    }
    if !checked.dkeys.is_empty() {
        println!("Display keys: not sent (display-key actions are not approved for hardware yet).");
    }
    if p.pictures.is_some() {
        println!("Pictures are uploaded separately with `kebnekaise picture`.");
    }
    if missing > 0 {
        eprintln!("warning: {missing} remap(s) were not echoed; check those keys.");
        return Ok(ExitCode::from(1));
    }
    println!("Done.");
    Ok(ExitCode::SUCCESS)
}

pub fn run_profile(command: ProfileCommand) -> CliResult {
    match command {
        ProfileCommand::Template { out: None } => {
            print!("{}", Profile::template());
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::Template { out: Some(path) } => {
            if path.exists() {
                return Err(format!("{} exists; not overwriting it", path.display()).into());
            }
            std::fs::write(&path, Profile::template())?;
            println!("Wrote {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::Check { file } => {
            let (p, _) = Profile::load(&file)?;
            println!(
                "{}: OK ({:?}, keyboard profile {})",
                file.display(),
                p.name,
                p.hardware_profile
            );
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::Show { file } => {
            let (p, checked) = Profile::load(&file)?;
            println!("Name:             {}", p.name);
            println!("Keyboard profile: {}", p.hardware_profile);
            match &p.lighting {
                Some(l) => println!(
                    "Lighting:         {} {} (brightness {})",
                    l.effect,
                    l.colors.join(","),
                    l.brightness
                ),
                None => println!("Lighting:         (not set)"),
            }
            if let Some(pics) = &p.pictures {
                for (name, path) in pics.entries() {
                    println!("Picture {name:<5}     {}", path.display());
                }
            }
            for (from, to) in &checked.remaps {
                println!("Remap:            {from} -> {to}");
            }
            for (name, action) in &p.dkeys {
                println!("{:<17} {:?}", format!("{}:", name.to_uppercase()), action);
            }
            for (name, m) in &p.macros {
                println!(
                    "Macro {name}: {} step(s), repeat {}",
                    m.steps.len(),
                    m.repeat
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        ProfileCommand::Apply {
            file,
            dry_run,
            json,
            hw,
        } => {
            let (p, checked) = Profile::load(&file)?;
            if !dry_run {
                if json {
                    return Err("--json is only for --dry-run".into());
                }
                return apply_profile(&p, &checked, &hw);
            }
            let steps = p.plan(&checked)?;
            if json {
                let list: Vec<_> = steps
                    .iter()
                    .map(|s| {
                        let c = s.packet.classification();
                        serde_json::json!({
                            "note": s.note,
                            "class": c.class,
                            "group": c.group,
                            "wait_after_ms": s.wait_after_ms,
                            "bytes": hex(trimmed(s.packet.as_bytes())),
                        })
                    })
                    .collect();
                let out = serde_json::json!({
                    "dry_run": true,
                    "hardware_profile": p.hardware_profile,
                    "packets": list,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(ExitCode::SUCCESS);
            }
            print_steps(&steps);
            if p.lighting.is_none() && !steps.is_empty() {
                println!(
                    "\nNo [lighting]: a real run would require keyboard profile {} to be active already.",
                    p.hardware_profile
                );
            }
            if p.pictures.is_some() {
                println!("Pictures are uploaded separately with `kebnekaise picture`.");
            }
            if !p.dkeys.is_empty() || !p.macros.is_empty() {
                println!(
                    "Display-key actions and macros run in `kebnekaise agent` (not built yet)."
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
