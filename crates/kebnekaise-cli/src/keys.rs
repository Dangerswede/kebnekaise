//! `kebnekaise key ...` and `kebnekaise profile ...` (phase 4).
//!
//! Nothing here writes to the keyboard yet: the remap and display-key
//! action commands are not approved for hardware, so every command that
//! would send a packet requires `--dry-run` and exits with code 4 without it.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use everest_core::hex;
use everest_core::lighting::trimmed;
use everest_core::profile::{PlanStep, Profile};
use everest_proto::keymap::{all_keys, nordic_legend, Shape};
use everest_proto::{ActionText, DKey, KeyPos, Packet, StoredKind};

use crate::CliResult;

#[derive(Subcommand)]
pub enum KeyCommand {
    /// List the key positions remaps accept, with their code.
    Names,
    /// Make one key act as another (`14 20`). Dry run only for now.
    Remap {
        /// Key to change (position name, Nordic legend or 0xNN).
        key: String,
        /// Key it should act as.
        to: String,
        /// Print the packet instead of sending it.
        #[arg(long)]
        dry_run: bool,
    },
    /// Undo a remap by mapping the key to itself. Dry run only for now.
    Restore {
        key: String,
        #[arg(long)]
        dry_run: bool,
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
    /// Write a profile to the keyboard. Dry run only for now.
    Apply {
        file: PathBuf,
        /// Print the packets instead of sending them.
        #[arg(long)]
        dry_run: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn not_approved(what: &str) -> CliResult {
    eprintln!("error: {what} may not write to the keyboard yet.");
    eprintln!("It is waiting for the first hardware test. Add --dry-run to see the packets.");
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
            println!("\nCodes from everest-mac; none is verified on hardware yet.");
            Ok(ExitCode::SUCCESS)
        }
        KeyCommand::Remap { key, to, dry_run } => {
            let (k, t): (KeyPos, KeyPos) = (key.parse()?, to.parse()?);
            if !dry_run {
                return not_approved("`key remap`");
            }
            print_steps(&[one(Packet::remap(k, t), format!("remap: {k} acts as {t}"))]);
            Ok(ExitCode::SUCCESS)
        }
        KeyCommand::Restore { key, dry_run } => {
            let k: KeyPos = key.parse()?;
            if !dry_run {
                return not_approved("`key restore`");
            }
            print_steps(&[one(
                Packet::remap(k, k),
                format!("remap: {k} back to itself"),
            )]);
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
        } => {
            let (p, checked) = Profile::load(&file)?;
            if !dry_run {
                return not_approved("`profile apply`");
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
