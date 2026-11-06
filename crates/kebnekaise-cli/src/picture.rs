//! `kebnekaise picture`: phase 3, display keys and dial.
//!
//! Every command converts and validates first. With `--dry-run` it runs the
//! whole transfer against a simulated keyboard (nothing is opened) and
//! prints the packets. Real runs are refused until the project owner has
//! approved the first picture write on hardware.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Subcommand, ValueEnum};
use everest_core::lighting::trimmed;
use everest_core::pictures::{self, Fit, SimulatedKeyboard, TransferReport, TransferTiming};
use everest_core::{hex, Keyboard};
use everest_hid::coexist::find_conflicts;
use everest_hid::discovery::{list_mountain_interfaces, select_command_channel};
use everest_hid::session::QueryOptions;
use everest_hid::{HidTransport, Policy, Session, TracingTransport};
use everest_proto::{KeyResetMask, Packet, Picture, PictureTarget};

use crate::{hid_api, print_conflicts, CliResult};

/// Real picture writes approved by the project owner so far: none.
/// (Picture resets were allowed in principle on 2026-10-30, but the first
/// hardware run of any phase 3 command still needs an explicit yes.)
const APPROVED_FOR_HARDWARE: bool = false;

#[derive(Subcommand)]
pub enum PictureCommand {
    /// Upload a picture to display key D1..D4 (72 x 72).
    Key {
        /// Display key 1..4 (D1..D4).
        key: u8,
        /// Image file: PNG, JPEG, GIF (first frame) or BMP.
        file: PathBuf,
        #[command(flatten)]
        image: ImageArgs,
        #[command(flatten)]
        common: PictureCommon,
    },
    /// Upload a picture to the dial screen (240 x 204). The dial must show
    /// its image menu.
    Dial {
        /// Image file: PNG, JPEG, GIF (first frame) or BMP.
        file: PathBuf,
        #[command(flatten)]
        image: ImageArgs,
        #[command(flatten)]
        common: PictureCommon,
    },
    /// Put the factory logo back on the dial (`13 41`).
    ResetDial {
        #[command(flatten)]
        common: PictureCommon,
    },
    /// Put the factory pictures back on display keys (`13 42`). Also
    /// restores the keys' factory shortcuts (Win+1 ...).
    ResetKeys {
        /// Keys 1..4, comma separated. Default: all four.
        #[arg(long, value_delimiter = ',', default_values_t = [1u8, 2, 3, 4])]
        keys: Vec<u8>,
        #[command(flatten)]
        common: PictureCommon,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum FitArg {
    /// Fill the screen, crop the middle (keeps the aspect ratio).
    Cover,
    /// Fit inside the screen with black borders (keeps the aspect ratio).
    Contain,
    /// Stretch to the screen size.
    Stretch,
}

impl From<FitArg> for Fit {
    fn from(f: FitArg) -> Self {
        match f {
            FitArg::Cover => Fit::Cover,
            FitArg::Contain => Fit::Contain,
            FitArg::Stretch => Fit::Stretch,
        }
    }
}

#[derive(Args)]
pub struct ImageArgs {
    /// How the image is fitted to the screen.
    #[arg(long, value_enum, default_value_t = FitArg::Cover)]
    fit: FitArg,
    /// Also save the converted picture (as the keyboard will show it) to
    /// this file, e.g. preview.png.
    #[arg(long, value_name = "FILE")]
    preview: Option<PathBuf>,
}

#[derive(Args)]
pub struct PictureCommon {
    /// Convert and show the packets, run against a simulated keyboard.
    /// Opens no device.
    #[arg(long)]
    dry_run: bool,
    /// Profile 1..5 whose display-key pictures to write. Default: the
    /// active profile (profile 1 for a dry run). Ignored for the dial.
    #[arg(long)]
    profile: Option<u8>,
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

/// What was asked for; the profile is filled in once known.
enum Request {
    Key { number: u8, image: Source },
    Dial { image: Source },
    ResetDial,
    ResetKeys { keys: Vec<u8> },
}

struct Source {
    file: PathBuf,
    fit: Fit,
    preview: Option<PathBuf>,
}

impl Request {
    fn target(&self, profile: u8) -> Result<Option<PictureTarget>, everest_proto::PictureError> {
        Ok(match self {
            Request::Key { number, .. } => Some(PictureTarget::key(profile, *number)?),
            Request::Dial { .. } => Some(PictureTarget::Dial),
            _ => None,
        })
    }

    fn source(&self) -> Option<&Source> {
        match self {
            Request::Key { image, .. } | Request::Dial { image } => Some(image),
            _ => None,
        }
    }

    fn reset_packet(&self, profile: u8) -> Result<Option<Packet>, everest_proto::PictureError> {
        Ok(match self {
            Request::ResetDial => Some(Packet::reset_dial_picture()),
            Request::ResetKeys { keys } => Some(Packet::reset_key_pictures(
                &KeyResetMask::for_profile(profile, keys)?,
            )),
            _ => None,
        })
    }
}

pub fn run(command: PictureCommand) -> CliResult {
    let (request, common) = match command {
        PictureCommand::Key {
            key,
            file,
            image,
            common,
        } => (
            Request::Key {
                number: key,
                image: Source {
                    file,
                    fit: image.fit.into(),
                    preview: image.preview,
                },
            },
            common,
        ),
        PictureCommand::Dial {
            file,
            image,
            common,
        } => (
            Request::Dial {
                image: Source {
                    file,
                    fit: image.fit.into(),
                    preview: image.preview,
                },
            },
            common,
        ),
        PictureCommand::ResetDial { common } => (Request::ResetDial, common),
        PictureCommand::ResetKeys { keys, common } => (Request::ResetKeys { keys }, common),
    };

    if !common.dry_run {
        if !APPROVED_FOR_HARDWARE {
            eprintln!("error: picture commands may not write to the keyboard yet.");
            eprintln!(
                "They have not been approved for hardware. Add --dry-run to convert the \
                 image and see the packets."
            );
            return Ok(ExitCode::from(4));
        }
        return send(&request, &common);
    }
    dry_run(&request, &common)
}

/// Converts the image for `target`, prints what happened and saves the
/// preview if asked.
fn prepare(
    source: &Source,
    target: PictureTarget,
    quiet: bool,
) -> Result<Picture, Box<dyn std::error::Error>> {
    let (picture, (w, h)) = pictures::convert(&source.file, target, source.fit)?;
    if !quiet {
        println!(
            "Image {} ({w} x {h}) -> {} x {} RGB565 ({:?}), {} bytes, checksum {:04x}.",
            source.file.display(),
            target.width(),
            target.height(),
            source.fit,
            picture.bytes().len(),
            picture.checksum()
        );
    }
    if let Some(path) = &source.preview {
        pictures::save_preview(&picture, path)
            .map_err(|e| format!("cannot write preview {}: {e}", path.display()))?;
        if !quiet {
            println!("Preview saved to {}.", path.display());
        }
    }
    Ok(picture)
}

fn dry_run(request: &Request, common: &PictureCommon) -> CliResult {
    let profile = common.profile.unwrap_or(1);
    let quiet = common.json;

    if let Some(packet) = request.reset_packet(profile)? {
        let c = packet.classification();
        // Through the pictures allowlist, without a device.
        let mut s = Session::new(SimulatedKeyboard::default(), Policy::Pictures);
        s.send(&packet)?;
        if common.json {
            let out = serde_json::json!({
                "dry_run": true,
                "packet": hex(trimmed(packet.as_bytes())),
                "classification": c,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(ExitCode::SUCCESS);
        }
        println!("Dry run (nothing is sent to the keyboard):\n");
        println!(
            " 1. {:<26} {:<8} {}",
            c.name,
            format!("{:?}", c.class).to_lowercase(),
            hex(trimmed(packet.as_bytes()))
        );
        println!("\nThe packet passes the pictures allowlist. It writes the keyboard's flash.");
        if matches!(request, Request::ResetKeys { .. }) {
            println!("The keyboard also restores the factory shortcuts (Win+1 ...) of these keys.");
        }
        return Ok(ExitCode::SUCCESS);
    }

    let target = request.target(profile)?.expect("an upload has a target");
    let picture = prepare(
        request.source().expect("an upload has a source"),
        target,
        quiet,
    )?;

    let mut session = Session::new(SimulatedKeyboard::default(), Policy::Pictures);
    let report = pictures::upload(
        &mut session,
        &picture,
        &TransferTiming::instant(),
        &mut |_, _| {},
    )?;
    let sim = session.into_transport();
    assert_eq!(
        sim.stored,
        picture.bytes(),
        "simulated keyboard got the picture"
    );

    // Everything before the first chunk: target, confirm, descriptor.
    let head = &sim.features[..3];
    if common.json {
        let out = serde_json::json!({
            "dry_run": true,
            "target": target,
            "bytes": picture.bytes().len(),
            "checksum": format!("{:04x}", picture.checksum()),
            "setup": head.iter().map(|p| hex(trimmed(p))).collect::<Vec<_>>(),
            "chunks": picture.chunk_count(),
            "after": sim.written.iter().map(|p| hex(trimmed(p))).collect::<Vec<_>>(),
            "timing": TransferTiming::default(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(ExitCode::SUCCESS);
    }

    println!(
        "\nDry run for the {target} against a simulated keyboard (nothing is opened or sent):\n"
    );
    for (i, p) in head.iter().enumerate() {
        let c = everest_proto::classify(p);
        println!("{:>2}. feature  {:<30} {}", i + 1, c.name, hex(trimmed(p)));
    }
    let first = picture.chunk_at(0).expect("non-empty");
    let last = picture
        .chunk_at((picture.chunk_count() - 1) * everest_proto::PACKET_SIZE)
        .expect("non-empty");
    println!(
        " 4. feature  {} image chunks of 64 bytes, each answered with the byte count",
        picture.chunk_count()
    );
    println!("             first: {}", hex(&first.as_bytes()[..16]));
    println!("             last:  {}", hex(&last.as_bytes()[..16]));
    for p in &sim.written {
        let c = everest_proto::classify(p);
        println!(" 5. output   {:<30} {}", c.name, hex(trimmed(p)));
    }
    print_summary(&report);
    println!(
        "\nOn the real keyboard the descriptor is re-sent while it answers \"busy\" (it erases \
         the old picture), and once it is accepted the transfer must run to the end."
    );
    if matches!(target, PictureTarget::Key { .. }) {
        println!("A display-key picture takes about 20 s on firmware 57.");
    } else {
        println!("The dial must show its image menu; the real run checks that first.");
    }
    Ok(ExitCode::SUCCESS)
}

fn print_summary(r: &TransferReport) {
    println!(
        "\n{} chunks sent ({} re-sent), descriptor sent {} time(s), done {}, display refresh {}.",
        r.chunks_sent,
        r.chunk_resends,
        r.descriptor_sends,
        if r.confirmed_done {
            "confirmed"
        } else {
            "NOT confirmed"
        },
        if r.refresh_seen { "seen" } else { "not seen" },
    );
}

/// Sends an approved picture request to the real keyboard.
fn send(request: &Request, common: &PictureCommon) -> CliResult {
    // Convert before opening anything: a bad file must fail early. The
    // profile is only known once the keyboard has been read, so convert
    // for profile 1 now and re-target the same bytes later.
    let converted = match request.target(common.profile.unwrap_or(1))? {
        Some(t) => Some(prepare(
            request.source().expect("an upload has a source"),
            t,
            false,
        )?),
        None => None,
    };
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
    let conflicts = find_conflicts(Some(&channel.path));
    if !conflicts.is_empty() {
        print_conflicts(&conflicts);
        eprintln!("Close them and try again.");
        return Ok(ExitCode::from(3));
    }

    let transport = TracingTransport::new(HidTransport::open(&api, &channel.path)?, common.debug);
    let mut keyboard = Keyboard::for_pictures(transport).with_query_options(QueryOptions {
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
    if !report.write_gate.writes_allowed() {
        eprintln!(
            "error: firmware {} is untested; nothing was written. --force overrides.",
            report.firmware.version
        );
        return Ok(ExitCode::from(5));
    }
    let profile = match common.profile {
        Some(p) => p,
        None => report
            .firmware
            .active_profile
            .ok_or("the keyboard reported no valid active profile; nothing was written")?,
    };

    if let Some(packet) = request.reset_packet(profile)? {
        let seen =
            keyboard.reset_pictures(&packet, report.write_gate, Duration::from_millis(500))?;
        println!("Sent {}.", hex(trimmed(packet.as_bytes())));
        for p in seen {
            println!("  received {}", hex(trimmed(&p)));
        }
        return Ok(ExitCode::SUCCESS);
    }

    let target = request.target(profile)?.expect("an upload has a target");
    let converted = converted.expect("an upload was converted");
    let picture = Picture::from_rgb565(target, converted.bytes().to_vec())?;
    everest_core::check_picture_target(target, &report.extended)?;

    println!(
        "Firmware {}, uploading to the {target}. Do not unplug the keyboard or stop the program.",
        report.firmware.version
    );
    let mut last = usize::MAX;
    let result = keyboard.upload_picture(
        &picture,
        &report,
        &TransferTiming::default(),
        &mut |done, total| {
            let pct = done * 100 / total.max(1);
            if pct != last {
                last = pct;
                eprint!("\r{pct:>3} %");
            }
        },
    );
    eprintln!();
    match result {
        Ok(r) => {
            if common.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            }
            print_summary(&r);
            println!("Took {:.1} s.", r.elapsed.as_secs_f64());
            Ok(ExitCode::SUCCESS)
        }
        Err(everest_core::Error::Transfer(e)) => {
            eprintln!("error: {e}");
            if !e.started() {
                eprintln!("The keyboard was not changed.");
            }
            Ok(ExitCode::from(6))
        }
        Err(e) => Err(e.into()),
    }
}
