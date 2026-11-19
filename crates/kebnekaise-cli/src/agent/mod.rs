//! `kebnekaise agent`: runs the `[dkeys]` actions of a profile file.
//!
//! The keyboard only reports that D1..D4 was pressed (event `01`, byte 42;
//! the keys must be armed with `kebnekaise key arm` so the factory
//! shortcuts are gone). The agent listens for those presses and does the
//! rest on the computer: opens a URL, starts a program, presses keys or
//! plays a macro through a virtual keyboard (Linux uinput), or switches the
//! keyboard profile (`14 00`, the only packet the agent ever writes).
//!
//! Key events of other keys are never printed or stored.

mod hostkeys;
#[cfg(target_os = "linux")]
mod uinput;

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use everest_core::lighting::LightingPlan;
use everest_core::profile::{DKeyAction, Profile};
use everest_core::{DeviceReport, Keyboard, WriteGate};
use everest_hid::discovery::{list_mountain_interfaces, select_command_channel};
use everest_proto::DKey;

use hostkeys::Event;
pub use hostkeys::HostLayout;

use crate::keys::{active_profile, connect, query_options, Hw, HwArgs};
use crate::{hid_api, CliResult};

#[derive(clap::Args)]
pub struct AgentArgs {
    /// Profile file whose `[dkeys]` and `[macros]` to run.
    file: PathBuf,
    /// Keyboard layout the computer uses, for typing macro text.
    #[arg(long, value_enum, default_value_t = HostLayout::Se)]
    layout: HostLayout,
    /// Print what each press would do instead of doing it.
    #[arg(long)]
    dry_run: bool,
    #[command(flatten)]
    hw: HwArgs,
}

/// A display-key action, checked and ready to run.
enum Ready {
    Url(String),
    Run(Vec<String>),
    /// Key presses (a combination or a macro) with a label for the log.
    Keys(String, Vec<Event>),
    Profile(u8),
}

impl Ready {
    fn describe(&self) -> String {
        match self {
            Ready::Url(u) => format!("open {u}"),
            Ready::Run(argv) => format!("run {}", argv.join(" ")),
            Ready::Keys(label, _) => label.clone(),
            Ready::Profile(n) => format!("switch to keyboard profile {n}"),
        }
    }
}

fn prepare(profile: &Profile, layout: HostLayout) -> Result<Vec<(DKey, Ready)>, Vec<String>> {
    let mut ready = Vec::new();
    let mut errors = Vec::new();
    for (name, action) in &profile.dkeys {
        let Ok(key) = name.parse::<DKey>() else {
            continue; // `Profile::load` already rejected it
        };
        let r = match action {
            DKeyAction::Url(u) => Ok(Ready::Url(u.clone())),
            DKeyAction::Run(c) => Ok(Ready::Run(c.argv())),
            DKeyAction::Keys(k) => {
                hostkeys::tap(layout, k).map(|ev| Ready::Keys(format!("press {k}"), ev))
            }
            DKeyAction::Macro(m) => hostkeys::compile_macro(layout, &profile.macros[m])
                .map(|ev| Ready::Keys(format!("play macro {m}"), ev)),
            DKeyAction::Profile(n) => Ok(Ready::Profile(*n)),
        };
        match r {
            Ok(r) => ready.push((key, r)),
            Err(e) => errors.push(format!("[dkeys.{name}]: {e}")),
        }
    }
    if errors.is_empty() {
        Ok(ready)
    } else {
        Err(errors)
    }
}

pub fn run_agent(args: AgentArgs) -> CliResult {
    let (profile, _checked) = match Profile::load(&args.file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(ExitCode::from(1));
        }
    };
    let actions = match prepare(&profile, args.layout) {
        Ok(a) => a,
        Err(errors) => {
            eprintln!("error: cannot run these with the {:?} layout:", args.layout);
            for e in errors {
                eprintln!("  {e}");
            }
            return Ok(ExitCode::from(1));
        }
    };
    if actions.is_empty() {
        eprintln!("error: {} has no [dkeys] actions.", args.file.display());
        return Ok(ExitCode::from(1));
    }

    let needs_keys = actions.iter().any(|(_, r)| matches!(r, Ready::Keys(..)));
    let typist = if needs_keys && !args.dry_run {
        match Typist::start() {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("error: cannot create the virtual keyboard: {e}");
                eprintln!("{}", Typist::HELP);
                return Ok(ExitCode::from(1));
            }
        }
    } else {
        None
    };

    let (mut keyboard, report) = match open(&args.hw)? {
        Ok(k) => k,
        Err(code) => return Ok(code),
    };
    println!(
        "Firmware {}, profile {}. Agent for {:?}{}:",
        report.firmware.version,
        active_profile(&report),
        profile.name,
        if args.dry_run { " (dry run)" } else { "" }
    );
    for (key, r) in &actions {
        println!("  {key}: {}", r.describe());
    }
    if report.firmware.active_profile != Some(profile.hardware_profile) {
        println!(
            "note: the file is for keyboard profile {}; the agent runs it anyway.",
            profile.hardware_profile
        );
    }
    println!("Press D1..D4. Ctrl+C stops the agent.");

    let mut agent = Agent {
        actions,
        typist,
        gate: report.write_gate,
        slot: report.firmware.lighting_slot.as_byte(),
        dry_run: args.dry_run,
    };
    let mut held: Vec<DKey> = Vec::new();
    loop {
        let reports = match keyboard.listen(Duration::from_millis(200)) {
            Ok(r) => r,
            Err(e) => {
                println!("Lost the keyboard ({e}). Waiting for it to come back...");
                drop(keyboard);
                let (k, report) = reconnect(&args.hw)?;
                println!("Keyboard back, profile {}.", active_profile(&report));
                keyboard = k;
                agent.slot = report.firmware.lighting_slot.as_byte();
                held.clear();
                continue;
            }
        };
        for p in reports {
            if p[0] != 0x01 {
                continue;
            }
            let now = DKey::pressed_in(&p);
            for key in now.iter().filter(|k| !held.contains(k)) {
                agent.pressed(*key, &mut keyboard);
            }
            held = now;
        }
    }
}

type Kb = Keyboard<Hw>;

/// Opens the keyboard with a session that may switch profiles (`14 00` is
/// in the lighting group) and nothing else.
fn open(hw: &HwArgs) -> Result<Result<(Kb, DeviceReport), ExitCode>, Box<dyn std::error::Error>> {
    Ok(connect(hw)?.map(|(transport, report)| {
        (
            Keyboard::for_lighting(transport).with_query_options(query_options(hw)),
            report,
        )
    }))
}

/// Waits until the keyboard is connected again and opens it.
fn reconnect(hw: &HwArgs) -> Result<(Kb, DeviceReport), Box<dyn std::error::Error>> {
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let present = hid_api()
            .map(|api| {
                select_command_channel(&list_mountain_interfaces(&api), hw.device.as_deref())
                    .is_ok()
            })
            .unwrap_or(false);
        if !present {
            continue;
        }
        // The keyboard needs a moment after being plugged in.
        std::thread::sleep(Duration::from_secs(1));
        if let Ok(Ok(k)) = open(hw) {
            return Ok(k);
        }
    }
}

struct Agent {
    actions: Vec<(DKey, Ready)>,
    typist: Option<Typist>,
    gate: WriteGate,
    slot: u8,
    dry_run: bool,
}

impl Agent {
    fn pressed(&mut self, key: DKey, keyboard: &mut Kb) {
        let Some((_, action)) = self.actions.iter().find(|(k, _)| *k == key) else {
            println!("{key}: no action in the profile file");
            return;
        };
        if self.dry_run {
            println!("{key}: would {}", action.describe());
            return;
        }
        println!("{key}: {}", action.describe());
        let result = match action {
            Ready::Url(u) => open_url(u),
            Ready::Run(argv) => spawn(argv),
            Ready::Keys(_, events) => match &self.typist {
                Some(t) => t.play(events.clone()),
                None => Err("no virtual keyboard".into()),
            },
            Ready::Profile(n) => self.switch_profile(*n, keyboard),
        };
        if let Err(e) = result {
            println!("{key}: failed: {e}");
        }
    }

    fn switch_profile(&mut self, number: u8, keyboard: &mut Kb) -> Result<(), String> {
        // An unset profile reports slot ff; fall back to the static slot.
        let slot = if self.slot <= 8 { self.slot } else { 0 };
        let plan = LightingPlan::switch(number, slot).map_err(|e| e.to_string())?;
        let outcome = keyboard
            .apply_lighting(&plan, self.gate)
            .map_err(|e| e.to_string())?;
        self.slot = outcome.firmware_after.lighting_slot.as_byte();
        if outcome.firmware_after.active_profile == Some(number) {
            Ok(())
        } else {
            Err(format!(
                "the keyboard reports profile {:?}",
                outcome.firmware_after.active_profile
            ))
        }
    }
}

/// Starts a program without a shell and without waiting for it.
fn spawn(argv: &[String]) -> Result<(), String> {
    let (program, rest) = argv.split_first().ok_or("empty command")?;
    let mut child = Command::new(program)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    // Reap it when it exits so no zombie stays behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let argv = ["rundll32", "url.dll,FileProtocolHandler", url];
    #[cfg(target_os = "macos")]
    let argv = ["open", url];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let argv = ["xdg-open", url];
    spawn(&argv.map(String::from))
}

/// Plays key events on a thread of its own, so the agent keeps listening.
/// One job at a time: a press while a macro still plays is ignored.
struct Typist {
    jobs: mpsc::SyncSender<Vec<Event>>,
}

impl Typist {
    #[cfg(target_os = "linux")]
    const HELP: &'static str = "\
The agent needs write access to /dev/uinput. Check with `ls -l /dev/uinput`;
docs/AGENT.md shows a udev rule that grants it to the logged-in user.";
    #[cfg(not(target_os = "linux"))]
    const HELP: &'static str = "Key actions and macros only work on Linux so far.";

    #[cfg(target_os = "linux")]
    fn start() -> Result<Self, String> {
        let mut device =
            uinput::VirtualKeyboard::create(&hostkeys::all_codes()).map_err(|e| e.to_string())?;
        let (jobs, rx) = mpsc::sync_channel::<Vec<Event>>(0);
        std::thread::spawn(move || {
            for events in rx {
                if let Err(e) = device.play(&events) {
                    println!("virtual keyboard failed: {e}; recreating it");
                    // Dropping the device releases every key it held.
                    drop(device);
                    match uinput::VirtualKeyboard::create(&hostkeys::all_codes()) {
                        Ok(d) => device = d,
                        Err(e) => {
                            println!("cannot recreate the virtual keyboard: {e}");
                            return;
                        }
                    }
                }
            }
        });
        Ok(Typist { jobs })
    }

    #[cfg(not(target_os = "linux"))]
    fn start() -> Result<Self, String> {
        Err("not supported on this system".into())
    }

    fn play(&self, events: Vec<Event>) -> Result<(), String> {
        match self.jobs.try_send(events) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err("still playing the last one".into()),
            Err(mpsc::TrySendError::Disconnected(_)) => Err("the virtual keyboard is gone".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_prepares() {
        let p = Profile::parse(Profile::template()).unwrap();
        let ready = prepare(&p, HostLayout::Se).unwrap();
        assert_eq!(ready.len(), 4);
        assert!(matches!(ready[0].1, Ready::Url(_)));
        assert!(matches!(ready[3].1, Ready::Profile(2)));
    }

    #[test]
    fn untypeable_text_is_refused_at_start() {
        let text = r#"
format = 1
name = "x"
hardware_profile = 1
[dkeys.d1]
action = "macro"
target = "m"
[macros.m]
steps = [{ text = "å" }]
"#;
        let p = Profile::parse(text).unwrap();
        p.check(None).unwrap();
        assert!(prepare(&p, HostLayout::Se).is_ok());
        let err = prepare(&p, HostLayout::Us).err().unwrap();
        assert!(err[0].contains("dkeys.d1"), "{err:?}");
    }
}
