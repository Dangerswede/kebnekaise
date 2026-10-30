# Language and architecture proposal

Status: **Decided 2026-10-30:** Rust with hidapi, licensed MIT OR Apache-2.0.
The comparison below is kept as the record of why.

## Requirements that drive the choice

1. One codebase, Linux and Windows 11, both first-class.
2. HID through the OS's own driver: hidraw on Linux, the built-in HID class
   driver on Windows. No WinUSB, no libusb detach.
3. Byte-exact packet building and a hard safety gate (allowlist, firmware check).
4. A standalone binary or one package per platform, built and tested in GitHub Actions.
5. Later: GUI or tray app, per-application profile switching on both OSes,
   Home Assistant integration.
6. A capture decoder that runs on both OSes and shares the protocol tables.

## Options

| | **Rust + `hidapi` crate** | **Python + `hidapi` (cython-hidapi)** | **C# / .NET 8 + HidSharp** | **Go + go-hid** |
|---|---|---|---|---|
| HID on Linux | hidapi with the **hidraw** backend (`linux-static-hidraw` feature) | hidraw via the wheel's backend (some wheels default to libusb; must pick hidraw) | HidSharp uses hidraw | cgo + hidapi |
| HID on Windows | native HID API, no driver | same | native HID API | cgo + hidapi |
| Each Windows collection as its own device | exposed (`interface_number`, `usage_page`, `usage`) | same | same | same |
| Distribution | one static ~3-6 MB `.exe` / ELF, no runtime | PyInstaller/Nuitka bundle (30-80 MB), PyInstaller exes are often flagged by Windows Defender | self-contained single file (60+ MB) or requires runtime | static binary, but cgo makes cross-builds harder |
| Linux packaging | `.deb`/`.rpm`/AppImage via `cargo-deb`, `cargo-generate-rpm` | AppImage or pip | AppImage | `.deb` via nfpm |
| Safety (byte-level code) | strong types, enums for commands, exhaustive matching | dynamic, relies on tests | good | good |
| Unit tests in CI without hardware | `cargo test` on `ubuntu-latest` and `windows-latest` | pytest | dotnet test | go test |
| GUI options | **Tauri 2** (web UI, small), **Slint**, **egui**; tray: `tray-icon` | **PySide6/Qt** (mature, LGPL), Tkinter | Avalonia (good, cross-platform) | Fyne, Wails |
| Per-app profile switching | `windows` crate (foreground window hooks); X11/Wayland per compositor | `pywin32`; same Linux issues | P/Invoke; same Linux issues | syscall/cgo |
| Home Assistant | MQTT (`rumqttc`) with HA discovery | `paho-mqtt`, very easy | MQTTnet | paho |
| Iteration speed while reverse engineering | slower to write | **fastest** | medium | medium |

Wayland note (all options): per-application profile switching has no common
Wayland API. It works on X11, on KDE (KWin scripting) and on GNOME (via a small
shell extension); this is the same work in any language.

## Recommendation: Rust, with hidapi

- **Distribution** is the decisive point: one small binary per platform, no
  runtime, no antivirus trouble, trivial GitHub Actions matrix.
- **Safety:** the allowlist and the firmware gate become types the compiler
  checks: a write packet cannot be built without a `WriteToken` that only the
  firmware check (or `--force`) hands out.
- **GUI later:** Tauri 2 gives a modern UI and a tray icon on both OSes, and
  reuses the same core crate. Slint is the alternative if we want no web view.
- **Decoder:** the same crate parses pcapng (`pcap-file` or `pcap-parser`),
  so the capture decoder and the driver share one command table.

When **Python** would be the better call: if you want the fastest possible
exploration in phases 1-2 and accept bigger, less tidy distribution. A hybrid
(Python prototype, Rust later) costs a rewrite; I would rather start in Rust.

## Proposed structure (Rust workspace)

```
kebnekaise/
  Cargo.toml                   # workspace
  crates/
    everest-proto/             # pure: packet builders/parsers, command table,
                               #   safety classes, LED/layout tables. No I/O. #![no_std]-friendly
    everest-hid/               # transport: hidapi wrapper, device discovery,
                               #   report-ID shim, reply matching, session lock
      src/platform/linux.rs    #   hidraw specifics, /proc scan for other users
      src/platform/windows.rs  #   collection selection, Base Camp process/service detection
    everest-core/              # high-level device API: open (with firmware gate),
                               #   info, lighting, displays, profiles; profile file format
    everest-capture/           # pcapng/usbmon/USBPcap decoder built on everest-proto
    kebnekaise-cli/            # binary `kebnekaise` (clap)
    kebnekaise-gui/            # phase 5 (Tauri), not started
  packaging/
    linux/70-mountain-everest.rules   # udev: hidraw only, TAG+="uaccess"
  docs/ PROTOCOL.md CAPTURE.md ARCHITECTURE.md
  tests/fixtures/              # captures and recorded replies for offline tests
  .github/workflows/ci.yml     # fmt, clippy, test on ubuntu + windows; release artifacts
```

### Key design rules

- **Allowlist at the lowest layer.** `everest-hid` only accepts packets built
  by `everest-proto`, each tagged with its safety class. `DENY` packets have no
  builder at all.
- **Firmware gate on open.** Read `11 00`; writes are only possible on a
  version in `SUPPORTED_FIRMWARE` (starts as `[0x57]` once we verify it), or
  with `--force`, which prints a clear warning.
- **One session at a time.** Base Camp detection on Windows (processes
  `Base Camp.exe`, `BaseCamp.Service.exe`, `MountainDisplayPadWorker.exe`,
  service `BaseCampService`) and a hidraw-holder scan on Linux; refuse to write
  while one is found, ask the user to close it. We never kill Base Camp
  ourselves.
- **Dry run everywhere.** Every write command supports `--dry-run`, which
  prints the exact bytes and their decoded meaning without opening the device.
- **Recorded-reply tests.** The transport has a mock backend fed with real
  replies from captures, so logic is tested in CI on both OSes.
- **Profile files:** versioned TOML or JSON, identical on both OSes, stored in
  the platform config dir (`~/.config/kebnekaise`, `%APPDATA%\kebnekaise`).

### udev rule (Linux, phase 1)

Narrower than other projects' rules: only the hidraw node, only our device,
granted to the logged-in user (`uaccess`) instead of `MODE="0666"` for everyone.

```
# /etc/udev/rules.d/70-mountain-everest.rules
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3282", ATTRS{idProduct}=="0001", TAG+="uaccess"
```

## Licence for our project

everest-mac (MIT) is compatible with anything. K2 (no licence) and
BaseCamp-Linux (GPL-3.0 plus non-commercial clause) contribute facts only, so
they do not constrain our choice. Chosen: **MIT OR Apache-2.0** (the Rust
convention, widest reuse). Contributions are accepted under the same terms.
