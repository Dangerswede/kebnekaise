# Kebnekaise

An open replacement for Mountain Base Camp for the **Mountain Everest Max**
keyboard, on **Linux and Windows**.

The keyboard keeps its original firmware. Kebnekaise talks to it over USB HID
through the operating system's own HID driver, the same way Base Camp does: no
driver installation, no firmware flashing.

> **Status: early.** Phase 1 (read-only CLI) works on Windows. Phase 2
> (lighting): static colour, built-in effects and per-key colours.
> Phase 3 (pictures on the display keys and the dial): dry run only.
> This project is not affiliated with Mountain or Listan.

## Usage

```
kebnekaise list            # HID interfaces of connected Mountain devices (opens nothing)
kebnekaise info            # firmware, layout, modules, active profile, lighting slot
kebnekaise info --raw      # ...plus decoded settings and the raw replies
kebnekaise info --json     # machine-readable
```

Lighting. These write to the keyboard:

```
kebnekaise rgb static ff8000 --no-save   # RAM only; replug to undo
kebnekaise rgb static ff8000             # saved to flash (13 55), stays after replug
kebnekaise rgb effect wave --mode dual --color "ff0000,0000ff" --speed 4
kebnekaise rgb effect breathing --color 00ffaa --no-save
kebnekaise rgb effect reactive --color "ffffff,000040"
kebnekaise rgb keys --all 101010 --key esc=ff0000 --key w=00ff00   # RAM only
kebnekaise rgb keys --all 101010 --key esc=ff0000 --save            # saved (slot 6)
kebnekaise rgb key-names    # names accepted by --key
```

Add `--dry-run` to any `rgb` command to print its packets instead (opens no
device, sends nothing). In PowerShell, quote colour lists.

Pictures (phase 3, dry run only for now). Images are converted to the
keyboard's format (72 x 72 for D1..D4, 240 x 204 for the dial):

```
kebnekaise picture key 1 logo.png --dry-run --preview key1.png
kebnekaise picture dial wallpaper.jpg --fit contain --dry-run
kebnekaise picture reset-keys --keys "1,2" --dry-run
kebnekaise picture reset-dial --dry-run
```

`info` sends only three documented read commands (`11 12`, `11 14`, `11 00`).
It refuses to run while Base Camp or another keyboard tool seems to be
running, because two programs talking to the keyboard disturb each other.

### Building

Install Rust from <https://rustup.rs>, then:

```
cargo build --release      # binary in target/release/kebnekaise(.exe)
cargo test                 # no keyboard needed
```

No system libraries are needed on Linux or Windows. CI builds both on every
push; the binaries are attached to each run as artifacts.

### Linux: access without root

```
sudo cp packaging/linux/70-mountain-everest.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
```

Then unplug and replug the keyboard. The rule gives the logged-in user access
to the keyboard's hidraw nodes only.

### Windows

Nothing to install: Kebnekaise uses Windows' built-in HID driver. Close Base
Camp first (including the tray icon and the `BaseCampService` service).

## Documents

- [docs/PROTOCOL.md](docs/PROTOCOL.md): the USB HID protocol, with what is
  confirmed and what is still unknown.
- [docs/CAPTURE.md](docs/CAPTURE.md): how to capture Base Camp's traffic to
  fill the gaps.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): language and architecture.

## Roadmap

| Phase | Content |
|---|---|
| 0 | Research and plan (done) |
| 1 | Read-only CLI: find the device, list HID interfaces, firmware, layout, attached modules, active profile. udev rule for Linux. (Windows verified; Linux to test) |
| 2 | RGB: static colour, brightness, per-key colours, built-in effects (static and effects verified on Windows; per-key testing) |
| 3 | Display keys and dial screen: upload your own pictures (spec and dry run done; hardware test next) |
| 4 | Profiles and key bindings: read, write, export, import |
| 5 | GUI or tray app, per-application profile switching, Home Assistant |

## Safety

- Commands are only sent when we know what they do, and every writing command
  is documented in `docs/PROTOCOL.md` before it is implemented.
- Firmware update, bootloader, device reset and flash reset commands are never
  implemented. The only resets are the picture resets for the dial and the
  display keys (phase 3), which only bring back the factory pictures.
- The firmware version is checked on connect; writes are refused on untested
  versions unless you pass `--force`.

## Acknowledgements

The protocol notes build on the work of
[everest-mac](https://github.com/Amorem/everest-mac) (MIT),
[K2](https://github.com/FrankieDedo/K2) and
[BaseCamp-Linux](https://github.com/ramisotti13-eng/BaseCamp-Linux).
No code from K2 or BaseCamp-Linux is used; see the licence notes in
`docs/PROTOCOL.md`.

## Licence

Kebnekaise is free software: you can redistribute it and/or modify it under
the terms of the [GNU General Public License](LICENSE), version 3 or (at your
option) any later version (`GPL-3.0-or-later`).
