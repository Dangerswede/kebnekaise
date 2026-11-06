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
   Then **unplug and replug the keyboard**: right after Base Camp has been
   stopped the keyboard may not answer at all until it is replugged.
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

## Phase 2: lighting dry run

No keyboard needed; nothing is opened or sent. Check that the commands run
on both systems and that the packets look like `docs/PROTOCOL.md`,
"Lighting (phase 2)":

```
kebnekaise rgb static ff0000 --dry-run
kebnekaise rgb effect wave --dry-run
kebnekaise rgb effect breathing --mode dual --color ff0000,0000ff --dry-run
kebnekaise rgb keys --all 101010 --key esc=ff0000 --dry-run
kebnekaise rgb keys --key esc=ff0000  # must refuse with exit code 4
```

## Phase 2: first lighting write (approved 2026-11-06)

`rgb static RRGGBB --no-save` is the only command that writes. It reads
`11 12`, `11 14`, `11 00`, then sends `14 00` (switch the active profile to
its static slot) and `14 2C` (static colour), and reads `11 00` back. It
sends no `13 55`, so nothing is saved to flash.

1. Close Base Camp and its service, replug the keyboard (as for phase 1).
2. `kebnekaise info` and note the profile and lighting slot.
3. `kebnekaise rgb static ff0000 --no-save --debug`
   - Expect all keys red and "Verified" at the end (exit code 0).
   - Exit code 6 means the read-back did not show the static slot: paste the output.
4. `kebnekaise rgb static 00ff00 --no-save` (green), and once with
   `--brightness 20` to see the brightness byte work.
5. Unplug and replug the keyboard, then `kebnekaise info` again.

## What to record (lighting)

- Did the colour change, and did the side strips change too?
- After the replug: is the old lighting back (slot and colour)? This answers
  questions 1 and 2 under "To find out at the first hardware test" in PROTOCOL.md.
- The `--debug` log of step 3.

## Phase 2: saving a static colour (approved 2026-11-06)

`rgb static RRGGBB` without `--no-save` adds `13 55 00 00 00` (save the
static slot to flash) after the colour.

1. `kebnekaise rgb static 0060ff --debug` (blue). Expect "Verified. Saved to flash".
2. Unplug and replug: the keys should still be blue.
3. To go back: `kebnekaise rgb static <your old colour>` (Markus: ff0000).

Record: does the colour survive the replug, and does the `13 55` get an echo
like the other writes?

## Phase 2: built-in effects (approved 2026-11-06)

Try each with `--no-save` first (RAM only, replug to undo), watch the board,
and note anything that looks different from Base Camp:

```
kebnekaise rgb effect wave --no-save --debug                 # rainbow, right
kebnekaise rgb effect wave --mode dual --color ff0000,0000ff --direction up --no-save
kebnekaise rgb effect wave --mode quad --color ff0000,00ff00,0000ff,ffff00 --no-save
kebnekaise rgb effect tornado --direction ccw --speed 5 --no-save
kebnekaise rgb effect breathing --color 00ffaa --speed 2 --no-save
kebnekaise rgb effect breathing --mode dual --color ff0000,0000ff --no-save
kebnekaise rgb effect reactive --color ffffff,000040 --no-save   # press keys
kebnekaise rgb effect matrix --color 00ff00,000000 --no-save
kebnekaise rgb effect yeti --color ffffff,002040 --no-save
kebnekaise rgb effect off --no-save
```

Then one saved effect (no `--no-save`) and a replug. Go back with
`kebnekaise rgb static <colour>`.

Record: does each effect look right (direction, speed 1 vs 5, colours,
background), and does `quad` (four stops, from disassembly only) work?
