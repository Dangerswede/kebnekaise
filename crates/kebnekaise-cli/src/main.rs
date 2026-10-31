//! `kebnekaise`: command-line tool for the Mountain Everest Max.
//!
//! Phase 1 is read-only: `list` enumerates HID interfaces without opening
//! anything, `info` sends only the documented read commands.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use everest_core::{hex, DeviceReport, Keyboard, WriteGate};
use everest_hid::coexist::{find_conflicts, Conflict, ConflictReason};
use everest_hid::discovery::{list_mountain_interfaces, select_command_channel};
use everest_hid::hidapi::HidApi;
use everest_hid::{HidInterface, HidTransport, InterfaceRole};
use everest_proto::DisplayBrightness;

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
    },
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
        } => info(json, raw, device.as_deref(), force, ignore_conflicts),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

type CliResult = Result<ExitCode, Box<dyn std::error::Error>>;

fn hid_api() -> Result<HidApi, Box<dyn std::error::Error>> {
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

fn info(
    json: bool,
    raw: bool,
    device: Option<&str>,
    force: bool,
    ignore_conflicts: bool,
) -> CliResult {
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

    let transport = HidTransport::open(&api, &channel.path)?;
    let mut keyboard = Keyboard::new(transport);
    let report = keyboard.read_report(force)?;

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

fn print_conflicts(conflicts: &[Conflict]) {
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
    let verified = if r.layout.verified() {
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
        "Layout:        {} ({:?}, code {}{known}{verified})",
        r.layout,
        r.layout.shape(),
        r.layout_code
    );
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
        println!("  11 12  {}", hex(&r.raw.layout));
        println!("  11 14  {}", hex(&r.raw.extended_info));
    }
}
