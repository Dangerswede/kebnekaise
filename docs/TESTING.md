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
kebnekaise rgb effect breathing --mode dual --color "ff0000,0000ff" --dry-run
kebnekaise rgb keys --all 101010 --key esc=ff0000 --dry-run
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

In PowerShell, quote colour lists: unquoted `ffffff,000040` is an array and
`000040` becomes the number 40.

Try each with `--no-save` first (RAM only, replug to undo), watch the board,
and note anything that looks different from Base Camp:

```
kebnekaise rgb effect wave --no-save --debug                 # rainbow, right
kebnekaise rgb effect wave --mode dual --color "ff0000,0000ff" --direction up --no-save
kebnekaise rgb effect wave --mode quad --color "ff0000,00ff00,0000ff,ffff00" --no-save
kebnekaise rgb effect tornado --direction ccw --speed 5 --no-save
kebnekaise rgb effect breathing --color 00ffaa --speed 2 --no-save
kebnekaise rgb effect breathing --mode dual --color "ff0000,0000ff" --no-save
kebnekaise rgb effect reactive --color "ffffff,000040" --no-save   # press keys
kebnekaise rgb effect matrix --color "00ff00,000000" --no-save
kebnekaise rgb effect yeti --color "ffffff,002040" --no-save
kebnekaise rgb effect off --no-save
```

Then one saved effect (no `--no-save`) and a replug. Go back with
`kebnekaise rgb static <colour>`.

Record: does each effect look right (direction, speed 1 vs 5, colours,
background), and does `quad` (four stops, from disassembly only) work?

## Phase 2: per-key colours (approved 2026-11-06)

`rgb keys` sends sequence B (16 packets, see PROTOCOL.md). Without `--save`
nothing is written to flash.

```
kebnekaise rgb keys --all 000000 --key esc=ff0000 --key w=00ff00 --key a=00ff00 --key s=00ff00 --key d=00ff00 --debug
kebnekaise rgb keys --all 202020 --side 0000ff
kebnekaise rgb keys --all 000000 --key iso-extra=ff0000 --key iso-hash=00ff00 --key grave=0000ff
kebnekaise rgb keys --all 000000 --key side0=ff0000 --key side44=00ff00
```

Then unplug and replug. The keyboard keeps the active slot, so an unsaved
per-key run comes back white (seen 2026-11-06); the CLI warns about this
whenever an unsaved command changes the slot. Finally try one
with `--save` and a replug. Go back with `kebnekaise rgb static <colour>`.

Record: are the right keys lit (Esc, WASD; on a Nordic board `iso-extra` is
`<>|`, `iso-hash` is `'*`, `grave` is `§`), where are side LEDs 0 and 44,
and does anything odd happen after the `14 A0` packets?

## Phase 3: pictures, dry run

No keyboard needed; nothing is opened or sent. The transfer runs against a
simulated keyboard, so every packet up to the last chunk is shown. Use any
PNG/JPEG/GIF/BMP; `--preview` writes the picture as the keyboard will show it
(after the RGB565 rounding).

```
kebnekaise picture key 1 logo.png --dry-run --preview key1.png
kebnekaise picture key 4 photo.jpg --fit contain --profile 2 --dry-run
kebnekaise picture dial wallpaper.png --dry-run --preview dial.png
kebnekaise picture reset-keys --keys "1,4" --dry-run
kebnekaise picture reset-dial --dry-run
```

Record: do the previews look right (crop, colours), and do the descriptors
match `docs/PROTOCOL.md`, "Pictures (phase 3)"?

Real runs refuse with exit code 4 until the first picture write is approved.
Plan for that test (one display key first, then the dial):

1. Close Base Camp, replug the keyboard, `kebnekaise info`. Note the profile.
2. `kebnekaise picture key 1 test.png --debug 2> key1-debug.txt`. Takes about
   20 s. Do not unplug or press Ctrl+C while it runs; if it fails after
   starting, unplug and replug.
3. Look at D1: right picture, right way up, right colours? Then replug and
   check that it stays.
4. Switch the dial to its image menu on the keyboard, then
   `kebnekaise picture dial test.png --debug 2> dial-debug.txt`.
5. Optionally `picture reset-keys --keys 1` and `picture reset-dial` to go
   back to the factory pictures.
