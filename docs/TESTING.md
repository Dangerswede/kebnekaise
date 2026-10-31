# Hardware test checklist

Phase 1 sends only read commands (`11 12`, `11 14`, `11 00`). Run this on
both Linux and Windows and paste the output into the pull request or issue.

## Linux

1. Install the udev rule (see README) and replug the keyboard.
2. `kebnekaise list`
   - Expect one `commands (vendor 0xFF00)` row on interface 3, plus input rows.
   - If it shows `commands (probable)`, the kernel did not expose the usage page; note it.
3. `kebnekaise info --raw`
4. `kebnekaise info --json`
5. Without the udev rule (or as another user), `kebnekaise info` should fail
   with a hint about the rule, not crash.

## Windows

1. Quit Base Camp (tray icon) and stop the service: `sc stop BaseCampService`
   (admin prompt). Check Task Manager for `Base Camp.exe`,
   `BaseCamp.Service.exe` and `MountainDisplayPadWorker.exe`.
2. `kebnekaise list`: expect separate rows for MI_00..MI_03 collections and one
   `commands (vendor 0xFF00)` row.
3. `kebnekaise info --raw` and `kebnekaise info --json`.
4. Start Base Camp again and run `kebnekaise info`: it must refuse with exit
   code 3 and name the Base Camp processes.

## What to record

- Firmware version and whether `Writes:` says allowed or refused.
- Layout code (Nordic is expected to be 11) and whether the printed layout is right.
- Modules: try with the numpad and the media dock attached and detached.
- Active profile and lighting slot: switch profile on the keyboard
  (Fn shortcut) and run `info` again.
- The three raw replies. They fill the unknown bytes in `PROTOCOL.md`.
