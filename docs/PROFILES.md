# Profile files

The keyboard cannot report its key remaps, display-key actions, lighting or
pictures back to us (see "Keys and display-key actions (phase 4)" in
`PROTOCOL.md`). A Kebnekaise profile is therefore a file on the computer,
and that file is the source of truth. The format is the same on Linux and
Windows: TOML, UTF-8, paths relative to the file.

Status (phase 4): `kebnekaise profile template|check|show|apply` work.
`profile apply` writes `[lighting]` and `[remap]` to the keyboard;
display-key markers are not sent yet, and display-key actions and macros
need `kebnekaise agent`, which is not built yet.

## Commands

```
kebnekaise profile template --out work.toml   # commented example
kebnekaise profile check work.toml            # lists every problem
kebnekaise profile show work.toml
kebnekaise profile apply work.toml --dry-run  # the packets, nothing sent
kebnekaise profile apply work.toml            # lighting + remaps for real
```

## Format 1

```toml
format = 1                 # required, this build reads format 1
name = "Work"              # free text
hardware_profile = 1       # keyboard profile 1..5 the file is written to
```

### `[lighting]` (optional)

A built-in effect with the same options as `kebnekaise rgb effect`:
`effect` (static, wave, tornado, breathing, reactive, matrix, yeti, off),
`mode`, `colors` (list of `RRGGBB`), `speed` 1..5, `brightness` 0..100
(default 100), `direction`. Applying it also makes `hardware_profile`
active and saves the slot (`13 55`).

### `[pictures]` (optional)

`d1`..`d4` and `dial`: image files. Uploaded with `kebnekaise picture`;
`profile apply` does not upload them yet.

### `[remap]` (optional)

`<key> = "<acts as>"`, both physical key positions. Names are US positions
(`caps`, `lctrl`, `semicolon`, ...); Nordic legends (`ö`, `ä`, `å`, `§`,
`<`, `+`, ...) and `0xNN` codes work too. `kebnekaise key names` lists them.
A key mapped to itself is restored. Keys not listed are not touched.

### `[dkeys.d1]` .. `[dkeys.d4]` (optional)

`action` and `target`:

| action | target | Example |
|---|---|---|
| `url` | http(s) URL | `"https://example.org"` |
| `run` | program and arguments, a string or a list | `["C:\\Tools\\app.exe", "--flag"]` |
| `keys` | key combination | `"ctrl+shift+esc"` |
| `macro` | name of a macro below | `"greeting"` |
| `profile` | keyboard profile 1..5 | `2` |

The keyboard only stores the marker `kebnekaise:d<n>` and reports the
press; the agent on the computer runs the action.

### `[macros.<name>]` (optional)

Played on the computer by the agent (Windows `SendInput`, Linux `uinput`).

```toml
[macros.greeting]
repeat = 1                       # 1..100
steps = [
  { text = "Hej!" },             # type text, any Unicode
  { delay_ms = 100 },            # wait, max 60000
  { keys = "enter" },            # press and release a combination
  { down = "shift" },            # hold ...
  { keys = "a" },
  { up = "shift" },              # ... and release (must match a down)
]
```

Key names for `keys`, `down`, `up`: one character, `f1`..`f24`, or `ctrl`,
`shift`, `alt`, `altgr`, `win`, `enter`, `esc`, `tab`, `space`, `backspace`,
`delete`, `insert`, `home`, `end`, `pgup`, `pgdn`, `up`, `down`, `left`,
`right`, `capslock`, `numlock`, `scrolllock`, `prtsc`, `pause`, `menu`,
`mute`, `volup`, `voldown`, `play`, `next`, `prev`, `stop`, joined with `+`.

## Rules

- Unknown fields are errors, so a typo is never silently ignored.
- `check` collects all problems before it reports.
- Limits: 1000 steps per macro, 60 s per delay, 100 repeats.
