# Kebnekaise

An open replacement for Mountain Base Camp for the **Mountain Everest Max**
keyboard, on **Linux and Windows**.

The keyboard keeps its original firmware. Kebnekaise talks to it over USB HID
through the operating system's own HID driver, the same way Base Camp does: no
driver installation, no firmware flashing.

> **Status: early.** Phase 0 (protocol research and plan) is done. No code yet.
> This project is not affiliated with Mountain or Listan.

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
| 1 | Read-only CLI: find the device, list HID interfaces, firmware, layout, attached modules, active profile. udev rule for Linux. |
| 2 | RGB: static colour, brightness, per-key colours, built-in effects |
| 3 | Display keys and dial screen: upload your own pictures |
| 4 | Profiles and key bindings: read, write, export, import |
| 5 | GUI or tray app, per-application profile switching, Home Assistant |

## Safety

- Commands are only sent when we know what they do, and every writing command
  is documented in `docs/PROTOCOL.md` before it is implemented.
- Firmware update, bootloader and factory-reset commands are never implemented.
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

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT), at your option.
