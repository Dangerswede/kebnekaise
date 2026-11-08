//! `kebnekaise`: command-line tool for the Mountain Everest Max.
//!
//! `list` enumerates HID interfaces without opening anything, `info` sends
//! only the documented read commands. `rgb` (phase 2) sets the lighting;
//! with `--dry-run` it only prints the packets and opens no device.
//! `picture` (phase 3) converts images for the display keys and the dial;
//! uploads write to the keyboard, the picture resets are dry run only.

mod keys;
mod picture;

use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use everest_core::lighting::{trimmed, LightingPlan};
use everest_core::{hex, DeviceReport, Keyboard, WriteGate};
use everest_hid::coexist::{find_conflicts, Conflict, ConflictReason};
use everest_hid::discovery::{list_mountain_interfaces, select_command_channel};
use everest_hid::hidapi::HidApi;
use everest_hid::session::QueryOptions;
use everest_hid::{HidInterface, HidTransport, InterfaceRole, TracingTransport};
use everest_proto::lighting::{CustomFrame, Rgb};
use everest_proto::{
    ledmap, ColorMode, Direction, DisplayBrightness, Effect, EffectSettings, LayoutShape,
};

#[derive(Parser)]
#[command(
    name = "kebnekaise",
    version,
    about = "Open-source tool for the Mountain Everest Max keyboard"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the HID interfaces of connected Mountain devices (opens nothing).
    List {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Read firmware version, layout, attached modules and active profile.
    Info {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Also print the raw replies (useful for protocol research).
        #[arg(long)]
        raw: bool,
        /// Command interface to open, as printed by `list`.
        #[arg(long, value_name = "PATH")]
        device: Option<String>,
        /// Report writes as allowed on untested firmware (no writes exist yet).
        #[arg(long)]
        force: bool,
        /// Read even if Base Camp or another tool seems to be using the keyboard.
        #[arg(long)]
        ignore_conflicts: bool,
        /// Log every packet written and read to stderr.
        #[arg(long)]
        debug: bool,
        /// How long to wait for each reply, in milliseconds.
        #[arg(long, value_name = "MS", default_value_t = 800)]
        timeout_ms: u64,
    },
    /// Lighting (phase 2). Dry run only for now: prints the packets.
    Rgb {
        #[command(subcommand)]
        command: RgbCommand,
    },
    /// Pictures on the display keys and the dial (phase 3). The resets
    /// still need --dry-run.
    Picture {
        #[command(subcommand)]
        command: picture::PictureCommand,
    },
    /// Key remaps and display-key arming (phase 4, dry run only).
    Key {
        #[command(subcommand)]
        command: keys::KeyCommand,
    },
    /// Profile files: template, check, show, apply (phase 4, dry run only).
    Profile {
        #[command(subcommand)]
        command: keys::ProfileCommand,
    },
}

#[derive(Subcommand)]
enum RgbCommand {
    /// One colour on every key (the built-in static effect).
    Static {
        /// Colour as RRGGBB.
        color: String,
        #[command(flatten)]
        effect: EffectArgs,
        #[command(flatten)]
        common: RgbCommon,
    },
    /// A built-in effect: wave, tornado, breathing, reactive, matrix, yeti, off.
    Effect {
        /// Effect name.
        name: String,
        /// Colour mode: single, dual, quad, rainbow, key-and-background.
        /// Default: the effect's first mode (rainbow for wave/tornado).
        #[arg(long)]
        mode: Option<String>,
        /// Colours as RRGGBB, comma separated or repeated.
        #[arg(long = "color", value_delimiter = ',')]
        colors: Vec<String>,
        /// Speed 1 (slowest) to 5 (fastest). Default 3.
        #[arg(long)]
        speed: Option<u8>,
        /// Wave: right, down, left, up. Tornado: cw, ccw.
        #[arg(long)]
        direction: Option<String>,
        #[command(flatten)]
        effect: EffectArgs,
        #[command(flatten)]
        common: RgbCommon,
    },
    /// Per-key colours (custom mode, slot 6).
    Keys {
        /// Colour of every key not set with --key.
        #[arg(long, default_value = "000000")]
        all: String,
        /// Colour of the side strips (all 45 LEDs).
        #[arg(long, default_value = "000000")]
        side: String,
        /// NAME=RRGGBB, repeatable. Names: see `kebnekaise rgb key-names`;
        /// side LEDs are side0..side44.
        #[arg(long = "key", value_name = "NAME=RRGGBB")]
        keys: Vec<String>,
        /// Physical shape. Nordic, UK, German and the other ISO layouts are iso.
        #[arg(long, value_enum, default_value_t = ShapeArg::Iso)]
        shape: ShapeArg,
        /// Brightness 0..100 (applied to the colours).
        #[arg(long, default_value_t = 100)]
        brightness: u8,
        /// Save the picture to flash (slot 6) so it survives a replug.
        #[arg(long)]
        save: bool,
        #[command(flatten)]
        common: RgbCommon,
    },
    /// List the key names `rgb keys --key` accepts, with their LED index.
    KeyNames {
        #[arg(long, value_enum, default_value_t = ShapeArg::Iso)]
        shape: ShapeArg,
    },
}

#[derive(Args)]
struct EffectArgs {
    /// Brightness 0..100.
    #[arg(long, default_value_t = 100)]
    brightness: u8,
    /// Do not save to flash (`13 55`). Base Camp always saves; whether an
    /// unsaved effect survives a replug is not known yet.
    #[arg(long)]
    no_save: bool,
}

#[derive(Args)]
struct RgbCommon {
    /// Print the packets instead of sending them (opens no device).
    #[arg(long)]
    dry_run: bool,
    /// Profile 1..5 for a dry run. A real run uses the active profile.
    #[arg(long, default_value_t = 1)]
    profile: u8,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
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

/// What `rgb` was asked to do; turned into a plan once the profile is known.
enum Request {
    Effect(EffectSettings, bool),
    Custom(Box<CustomFrame>, u8, bool),
}

impl Request {
    fn plan(&self, profile: u8) -> Result<LightingPlan, everest_proto::LightingError> {
        match self {
            Request::Effect(s, save) => LightingPlan::effect(profile, s, *save),
            Request::Custom(f, b, save) => LightingPlan::custom(profile, f, *b, *save),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ShapeArg {
    Iso,
    Ansi,
}

impl From<ShapeArg> for LayoutShape {
    fn from(s: ShapeArg) -> Self {
        match s {
            ShapeArg::Iso => LayoutShape::Iso,
            ShapeArg::Ansi => LayoutShape::Ansi,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::List { json } => list(json),
        Command::Info {
            json,
            raw,
            device,
            force,
            ignore_conflicts,
            debug,
            timeout_ms,
        } => info(
            InfoOptions {
                json,
                raw,
                force,
                ignore_conflicts,
                debug,
                timeout_ms,
            },
            device.as_deref(),
        ),
        Command::Rgb { command } => rgb(command),
        Command::Picture { command } => picture::run(command),
        Command::Key { command } => keys::run_key(command),
        Command::Profile { command } => keys::run_profile(command),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

pub(crate) type CliResult = Result<ExitCode, Box<dyn std::error::Error>>;

pub(crate) fn hid_api() -> Result<HidApi, Box<dyn std::error::Error>> {
    HidApi::new().map_err(|e| format!("cannot initialise HID: {e}").into())
}

fn list(json: bool) -> CliResult {
    let interfaces = list_mountain_interfaces(&hid_api()?);
    if json {
        println!("{}", serde_json::to_string_pretty(&interfaces)?);
        return Ok(ExitCode::SUCCESS);
    }
    if interfaces.is_empty() {
        println!("No Mountain devices found (USB vendor 3282).");
        return Ok(ExitCode::from(2));
    }
    println!(
        "{:<10} {:>5} {:>6} {:>6}  {:<26} PATH",
        "VID:PID", "IFACE", "PAGE", "USAGE", "ROLE"
    );
    for i in &interfaces {
        println!(
            "{:04x}:{:04x}  {:>5} {:>#06x} {:>#06x}  {:<26} {}",
            i.vendor_id,
            i.product_id,
            i.interface_number,
            i.usage_page,
            i.usage,
            role_text(i),
            i.path
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn role_text(i: &HidInterface) -> String {
    match i.role {
        InterfaceRole::VendorCommands => "commands (vendor 0xFF00)".into(),
        InterfaceRole::VendorCommandsProbable => "commands (probable)".into(),
        InterfaceRole::Input => "input (owned by the OS)".into(),
        InterfaceRole::OtherProduct(name) => format!("other: {name}"),
    }
}

struct InfoOptions {
    json: bool,
    raw: bool,
    force: bool,
    ignore_conflicts: bool,
    debug: bool,
    timeout_ms: u64,
}

fn info(opts: InfoOptions, device: Option<&str>) -> CliResult {
    let InfoOptions {
        json,
        raw,
        force,
        ignore_conflicts,
        debug,
        timeout_ms,
    } = opts;
    let api = hid_api()?;
    let interfaces = list_mountain_interfaces(&api);
    let channel = match select_command_channel(&interfaces, device) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Run `kebnekaise list` to see what is connected.");
            return Ok(ExitCode::from(2));
        }
    };

    let conflicts = find_conflicts(Some(&channel.path));
    if !conflicts.is_empty() {
        print_conflicts(&conflicts);
        if !ignore_conflicts {
            eprintln!("Close them and try again (or pass --ignore-conflicts to read anyway).");
            return Ok(ExitCode::from(3));
        }
    }

    if debug {
        eprintln!("Opening {}", channel.path);
    }
    let transport = TracingTransport::new(HidTransport::open(&api, &channel.path)?, debug);
    let mut keyboard = Keyboard::new(transport).with_query_options(QueryOptions {
        timeout: Duration::from_millis(timeout_ms),
        attempts: 3,
    });
    let report = match keyboard.read_report(force) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            let skipped = keyboard.skipped();
            if skipped.is_empty() {
                eprintln!("Nothing at all was received from the keyboard.");
                eprintln!(
                    "If Base Camp was running recently, unplug and replug the keyboard, then try again."
                );
            } else {
                eprintln!("Packets received that were not the expected reply:");
                for p in skipped {
                    eprintln!("  {}", hex(p));
                }
            }
            if !debug {
                eprintln!("Run again with --debug to see every packet.");
            }
            return Ok(ExitCode::from(1));
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report, channel, raw);
        if raw && !keyboard.skipped().is_empty() {
            println!("\nOther packets received while waiting:");
            for p in keyboard.skipped() {
                println!("  {}", hex(p));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn print_conflicts(conflicts: &[Conflict]) {
    eprintln!("Other programs may be using the keyboard:");
    for c in conflicts {
        let why = match c.reason {
            ConflictReason::BaseCamp => "Mountain Base Camp",
            ConflictReason::KnownTool => "another keyboard tool",
            ConflictReason::HoldsDevice => "has the command interface open",
        };
        eprintln!("  pid {:>6}  {:<32} {}", c.pid, c.name, why);
    }
    eprintln!("Two programs talking to the keyboard at once disturb each other.");
}

fn print_report(r: &DeviceReport, channel: &HidInterface, raw: bool) {
    let fw = &r.firmware;
    println!("Device:        Mountain Everest Max ({})", channel.path);
    let tested = if fw.version.is_tested() {
        "tested"
    } else {
        "NOT tested"
    };
    println!("Firmware:      {} ({tested})", fw.version);
    match (r.layout, r.layout_code) {
        (Some(layout), Some(code)) => {
            let verified = if layout.verified() {
                ""
            } else {
                ", not yet verified on hardware"
            };
            let known = if r.layout_code_known {
                ""
            } else {
                ", unknown code: Base Camp falls back to US"
            };
            println!(
                "Layout:        {layout} ({:?}, code {code}{known}{verified})",
                layout.shape()
            );
        }
        _ => println!("Layout:        unknown (no answer to 11 12)"),
    }
    println!("Modules:       {}", r.extended.modules.describe());
    match fw.active_profile {
        Some(p) => println!("Profile:       {p} of 5"),
        None => println!("Profile:       unknown"),
    }
    println!("Lighting slot: {}", fw.lighting_slot);
    println!("Dial mode:     {}", r.extended.dial_mode);
    let gate = match r.write_gate {
        WriteGate::Allowed { .. } => "allowed".to_string(),
        WriteGate::Forced { firmware } => {
            format!("allowed by --force on untested firmware {firmware}")
        }
        WriteGate::Refused { firmware } => {
            format!("refused (firmware {firmware} is untested; --force overrides)")
        }
    };
    println!("Writes:        {gate}");

    if raw {
        let e = &r.extended;
        println!(
            "\nDial colour:   #{:02x}{:02x}{:02x}",
            e.dock_color[0], e.dock_color[1], e.dock_color[2]
        );
        println!(
            "Screensaver:   {} s, turn off after {} s",
            e.screensaver_seconds, e.turn_off_seconds
        );
        let b = |d: DisplayBrightness| match d {
            DisplayBrightness::FollowLighting => "follow".to_string(),
            DisplayBrightness::Fixed(v) => format!("{v}%"),
        };
        for (i, (dial, numpad)) in e.display_brightness.iter().enumerate() {
            println!("Displays [{i}]:  dial {}, keys {}", b(*dial), b(*numpad));
        }
        println!("\nRaw replies:");
        println!("  11 00  {}", hex(&r.raw.firmware_info));
        match &r.raw.layout {
            Some(l) => println!("  11 12  {}", hex(l)),
            None => println!("  11 12  (no answer)"),
        }
        println!("  11 14  {}", hex(&r.raw.extended_info));
    }
}

fn rgb(command: RgbCommand) -> CliResult {
    let (request, common) = match command {
        RgbCommand::KeyNames { shape } => {
            for (name, led) in ledmap::key_names(shape.into()) {
                println!("{name:<12} {led:>3}");
            }
            println!("side0..side44  side strips");
            return Ok(ExitCode::SUCCESS);
        }
        RgbCommand::Static {
            color,
            effect,
            common,
        } => {
            let settings = EffectSettings::new(
                Effect::Static,
                None,
                vec![color.parse()?],
                None,
                effect.brightness,
                None,
            )?;
            (Request::Effect(settings, !effect.no_save), common)
        }
        RgbCommand::Effect {
            name,
            mode,
            colors,
            speed,
            direction,
            effect,
            common,
        } => {
            let which: Effect = name.parse()?;
            if which == Effect::Static {
                return Err("use `kebnekaise rgb static RRGGBB` for a static colour".into());
            }
            let mode = mode.map(|m| m.parse::<ColorMode>()).transpose()?;
            let colors = colors
                .iter()
                .map(|c| c.parse::<Rgb>())
                .collect::<Result<Vec<_>, _>>()?;
            let direction = direction.map(|d| d.parse::<Direction>()).transpose()?;
            let settings =
                EffectSettings::new(which, mode, colors, speed, effect.brightness, direction)?;
            (Request::Effect(settings, !effect.no_save), common)
        }
        RgbCommand::Keys {
            all,
            side,
            keys,
            shape,
            brightness,
            save,
            common,
        } => {
            // Only LEDs that exist on this shape get --all; the unused
            // slots of the 152-slot frame stay black.
            let mut frame = CustomFrame::filled(Rgb::BLACK, side.parse()?);
            let all: Rgb = all.parse()?;
            for (_, led) in ledmap::key_names(shape.into()) {
                frame.keys[usize::from(led)] = all;
            }
            for spec in &keys {
                let (name, color) = spec
                    .split_once('=')
                    .ok_or_else(|| format!("--key {spec:?}: expected NAME=RRGGBB"))?;
                let color: Rgb = color.parse()?;
                match ledmap::side_index(name) {
                    Some(i) => frame.side[i] = color,
                    None => frame.keys[usize::from(ledmap::key_index(name, shape.into())?)] = color,
                }
            }
            (Request::Custom(Box::new(frame), brightness, save), common)
        }
    };

    // Every lighting command is approved for hardware (static colour,
    // effects and per-key colours, all by the project owner on 2026-11-06).
    if !common.dry_run {
        return rgb_send(&request, &common);
    }

    let plan = request.plan(common.profile)?;
    let steps = plan.dry_run()?;
    if common.json {
        let out = serde_json::json!({
            "dry_run": true,
            "profile": plan.expected.profile,
            "expected": plan.expected,
            "saves_to_flash": plan.saves,
            "packets": steps,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(ExitCode::SUCCESS);
    }

    println!(
        "Dry run for profile {} (nothing is sent to the keyboard):\n",
        plan.expected.profile
    );
    for (i, s) in steps.iter().enumerate() {
        println!(
            "{:>2}. {:<26} {:<8} wait {:>3} ms  {}",
            i + 1,
            s.classification.name,
            format!("{:?}", s.classification.class).to_lowercase(),
            s.wait_after.as_millis(),
            hex(trimmed(&s.bytes)),
        );
    }
    println!(
        "\nAll {} packets pass the lighting allowlist. About {} ms in total.",
        steps.len(),
        plan.duration().as_millis()
    );
    println!(
        "A real run would then read 11 00 and expect profile {}, lighting slot {}.",
        plan.expected.profile, plan.expected.lighting_slot
    );
    if plan.saves {
        println!("The last packet (13 55) saves the slot to the keyboard's flash.");
    } else {
        println!("Not saved to flash.");
    }
    Ok(ExitCode::SUCCESS)
}

/// Sends an approved lighting request to the real keyboard.
fn rgb_send(request: &Request, common: &RgbCommon) -> CliResult {
    let api = hid_api()?;
    let interfaces = list_mountain_interfaces(&api);
    let channel = match select_command_channel(&interfaces, common.device.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Run `kebnekaise list` to see what is connected.");
            return Ok(ExitCode::from(2));
        }
    };
    // Never write while another program talks to the keyboard.
    let conflicts = find_conflicts(Some(&channel.path));
    if !conflicts.is_empty() {
        print_conflicts(&conflicts);
        eprintln!("Close them and try again.");
        return Ok(ExitCode::from(3));
    }

    let transport = TracingTransport::new(HidTransport::open(&api, &channel.path)?, common.debug);
    let mut keyboard = Keyboard::for_lighting(transport).with_query_options(QueryOptions {
        timeout: Duration::from_millis(common.timeout_ms),
        attempts: 3,
    });
    let report = match keyboard.read_report(common.force) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!(
                "Nothing was written. If Base Camp ran recently, unplug and replug the keyboard."
            );
            return Ok(ExitCode::from(1));
        }
    };
    let before = report.firmware.clone();
    let profile = before
        .active_profile
        .ok_or("the keyboard reported no valid active profile; nothing was written")?;
    let plan = request.plan(profile)?;

    println!(
        "Firmware {}, profile {profile}, lighting slot {} before.",
        before.version, before.lighting_slot
    );
    let outcome = match keyboard.apply_lighting(&plan, report.write_gate) {
        Ok(o) => o,
        Err(everest_core::Error::WritesRefused(fw)) => {
            eprintln!("error: firmware {fw} is untested; nothing was written. --force overrides.");
            return Ok(ExitCode::from(5));
        }
        Err(e) => return Err(e.into()),
    };
    if common.json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
    }
    println!(
        "Sent {} packets. Now: profile {}, lighting slot {}.",
        outcome.packets_sent,
        outcome
            .firmware_after
            .active_profile
            .map_or("unknown".into(), |p| p.to_string()),
        outcome.firmware_after.lighting_slot
    );
    if outcome.verified {
        if plan.saves {
            println!("Verified. Saved to flash: the lighting stays after a replug.");
        } else if before.lighting_slot.as_byte() == plan.expected.lighting_slot {
            println!("Verified. Not saved to flash: unplug and replug the keyboard to get the saved lighting back.");
        } else {
            println!("Verified. Not saved to flash.");
            eprintln!(
                "warning: the keyboard was on lighting slot {} and is now on slot {}. It remembers \
                 the active slot across a replug, so a replug will show what is saved in slot {} \
                 (white if nothing was saved there), not your previous lighting.",
                before.lighting_slot,
                outcome.firmware_after.lighting_slot,
                outcome.firmware_after.lighting_slot
            );
            eprintln!(
                "To keep this lighting, run the command again with {}. To go back, set the \
                 previous lighting again (for example `kebnekaise rgb static RRGGBB`).",
                if matches!(request, Request::Custom(..)) {
                    "--save"
                } else {
                    "saving (without --no-save)"
                }
            );
        }
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "warning: expected profile {}, slot {} afterwards; the keyboard reports otherwise.",
            plan.expected.profile, plan.expected.lighting_slot
        );
        Ok(ExitCode::from(6))
    }
}
