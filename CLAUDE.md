# CLAUDE.md

Kebnekaise: open Base Camp replacement for the Mountain Everest Max (USB `3282:0001`), Linux and Windows, Rust + hidapi. Code, comments and docs are English.

## Hard rules (always apply)

- Never send a command to the keyboard unless `docs/PROTOCOL.md` documents what it does. Every writing command is documented there before it is implemented.
- Never implement, call or explore firmware update, bootloader, `aa 55 7f` (device reset) or `13 40` (flash reset). Picture transfers use only the exact Base Camp sequence with targets `03`/`04`.
- Check the firmware version (`11 00`, byte 4) on connect. Refuse writes on versions not in the supported list unless the user passes `--force`.
- Reads come first. Ask the project owner before the first writing command is run against real hardware.
- Do not copy code or text from K2 (no licence) or BaseCamp-Linux (GPL-3.0 + non-commercial). everest-mac is MIT; credit it when its findings are used.
- Never drive the keyboard while Base Camp is running; detect it and ask the user to close it.
