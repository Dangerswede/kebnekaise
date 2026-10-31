# Mountain Everest Max: USB HID protocol

Status: **Phase 0 draft** (2026-10-30). Nothing in this document has been
verified by this project on real hardware yet. Every entry carries a
confidence tag and a source, so we can tell what we know from what we hope.

This is our own description of the protocol, written for interoperability.
No code from other projects is included (see [Sources and licences](#sources-and-licences)).

## Confidence tags

| Tag | Meaning |
|---|---|
| **[C]** confirmed | Reported by at least one source as verified on hardware *and* consistent across sources. Still to be re-verified by us. |
| **[P]** probable | One source, derived from disassembly of Base Camp, not reported as hardware-verified, or only verified on one layout/firmware. |
| **[U]** unknown | We do not know. Needs a capture (see `CAPTURE.md`) before any implementation. |
| **[V]** verified by us | Re-checked by this project on Linux *and* Windows. (None yet.) |

## Safety classes

Every command we implement belongs to exactly one class. The transport layer
enforces an **allowlist**: a packet whose class is not explicitly allowed for
the current operation is refused before it reaches the device.

| Class | Meaning | Policy |
|---|---|---|
| `READ` | Query, no side effect known. | Allowed on any firmware. |
| `VOLATILE` | Changes live state (RAM) only, as far as we know. | Writes: requires a supported firmware or `--force`. |
| `PERSIST` | Writes the keyboard's flash (settings, profiles, actions). | As `VOLATILE`, plus explicit user action. |
| `TRANSFER` | Picture transfer. Same mechanism as the firmware updater. | Phase 3 only. Exact Base Camp sequence, no argument exploration. |
| `DENY` | Reset, recovery, firmware, bootloader, or unknown. | **Never sent.** Not implemented. Rejected by the allowlist. |

## Device identification

| Item | Value | Conf. |
|---|---|---|
| USB VID:PID | `3282:0001` (Everest Max and Everest Core share it) | [C] |
| Other Mountain PIDs (not in scope, but must not be mistaken for the keyboard) | `0002` Makalu Max, `0003` Makalu 67, `0005`/`0006` Everest 60 ANSI/ISO, `0008` MacroPad, `0009` DisplayPad | [C] |
| Telling Max from Core | No USB difference. Only the "attached modules" fields of `11 14` (numpad, media dock) tell them apart, and a Max with both modules removed looks like a Core. | [P] |

## USB interfaces

| Interface | Content | Windows driver | Use for us |
|---|---|---|---|
| 0 (`MI_00`) | Boot keyboard | `kbdhid` (exclusive) | none |
| 1 (`MI_01`) | Top-level collections COL01..COL03: consumer control, system control, mouse | OS class drivers | none |
| 2 (`MI_02`) | Keyboard (NKRO/full) | `kbdhid` (exclusive) | none |
| 3 (`MI_03`) | **Vendor collection**, usage page `0xFF00`, usage `0x0001`, 64-byte reports | `HidUsb`, plain HIDClass, no exclusive owner | **all commands** |

Interface 3 endpoints: interrupt OUT `0x05`, interrupt IN `0x84` [C].

Notes:

- On Windows each top-level collection becomes its own device path
  (`HID\VID_3282&PID_0001&MI_03...`). Select by **interface number 3 and usage
  page `0xFF00` / usage `0x0001`**, never by enumeration order. [C]
- On macOS more than one interface reports the same usage page; the vendor one
  has the larger input reports. Keep that as a fallback check. [P]
- On Linux, old kernels may not expose the usage page; fall back to interface
  number 3. [P]
- No driver change (WinUSB/Zadig, libusb detach) is needed on any OS. [C]

## Transport

### Two channels on interface 3

1. **Interrupt channel:** 64-byte **unnumbered** output and input reports
   (report ID 0, so all 64 bytes are payload). Used for queries, settings,
   lighting and events. [C]
2. **Feature reports:** 64-byte **unnumbered** feature reports, written with
   SET_REPORT(Feature) and answered by reading GET_REPORT(Feature). Used only
   for the picture transfer (`aa 55 ...`). [C]

### Report ID byte in hidapi

Because the reports are unnumbered, hidapi expects a leading `0x00` report-ID
byte on every write: `hid_write([0x00] + 64 bytes)` and
`hid_send_feature_report([0x00] + 64 bytes)`. On Windows the write length must
match the collection's `OutputReportByteLength` (65). How each backend returns
`hid_get_feature_report` data for report ID 0 (with or without the leading
zero) must be checked on both OSes in phase 1, behind a unit-tested shim. [P]

### Packet framing

- Packet = 64 bytes, zero-padded. Byte 0 is the **command family**, byte 1 the
  **command**. [C]
- **No checksum, no sequence number** on the interrupt channel, as far as all
  sources show. [P]
- Families seen: `0x11` queries and display settings, `0x12` action-slot
  select, `0x13` flash save / resets, `0x14` lighting and profile switching,
  `0x17` action payload, `0x01` key events (device to host), `0xff` status
  notifications (device to host). [C]
- A reply to `11 xx` starts with `11 xx`. [C]

### Session rules (important)

- **Replies are broadcast** to every process that has the vendor collection
  open. The next packet you read is not necessarily the answer to your query:
  match the reply on bytes 0..1. [C]
- **Single command buffer:** queries sent back to back are only answered for
  the last one. Send one, wait for its reply (or timeout), then send the next. [C]
- The keyboard can take **up to ~3 s** to apply a settings write and drops
  commands that arrive meanwhile. Writes need read-back verification. [C]
- Two programs driving the keyboard at once (e.g. Base Camp and us) will
  interleave commands and break each other. Base Camp detection is mandatory
  (see [Coexistence with Base Camp](#coexistence-with-base-camp)). [C]
- Unsolicited input: `01 ...` key/display-key events, `ff aa` "display
  refreshed". Readers must skip them while waiting for a reply. [C]

### Session start ("wake")

1. `11 12` (READ). Reply byte 4 = layout code. [C]
2. `11 14` (READ). Reply = extended info (below). [C]
3. Drain the input queue.

All three source projects open the session this way before anything else.

## Commands: READ

### `11 00`: firmware info

| Reply byte | Meaning | Conf. |
|---|---|---|
| 0..1 | `11 00` echo | [C] |
| 4 | Firmware version, BCD-like: `0x57` = firmware 57 | [C] |
| 10 | Active profile, 1..5 | [C] |
| 11 | Active lighting slot of that profile (see slots below) | [C] |
| other | unknown (possibly sub-versions, numpad/dock firmware) | [U] |

### `11 12`: layout

Reply byte 4 = layout code. Mapping used by Base Camp:

| Code | Layout | Shape | Conf. |
|---|---|---|---|
| 4 | UK | ISO | [C] (only one verified on hardware) |
| 3 | German | ISO | [P] |
| 5 | French | ISO | [P] |
| 8 | Italian | ISO | [P] |
| 11 | Nordic | ISO | [P] |
| 12 | Portuguese | ISO | [P] |
| 15 | Spanish | ISO | [P] |
| 13 | Hebrew | ANSI | [P] |
| 22 | Korean | ANSI | [P] |
| 17 or anything else | US | ANSI | [P] |

### `11 14` (read form): extended info

Request: `11 14`, rest zero. The reply carries a 29-byte structure at offset 4
(Base Camp's `GetExtendInfo`):

| Reply offset | Field | Conf. |
|---|---|---|
| 4 | media dock attached (non-zero = yes) | [C] |
| 5 | dock: show menu | [P] |
| 6 | dock: menu index | [P] |
| 7..9 | dock menu colour R,G,B (must be echoed in the write form) | [C] |
| 10 | screen setup: dial mode (`0x01` image, `0x11` clock, `0x71` volume, `0x91` CPU, `0xA1` GPU, `0xB1` disk, `0xC1` network, `0xD1` RAM, `0xE1` APM). High nibble also described as screensaver type, low bits as screensaver/turn-off enables; the two readings need reconciling. | [P] |
| 11..12 | screensaver delay, seconds, LE | [P] |
| 13..14 | turn-off delay, seconds, LE | [P] |
| 15..19 | "show profile" per profile | [P] |
| 20 | numpad attached (non-zero = yes) | [C] |
| 21 | pixel-shift time | [P] |
| 22 | dock double-click | [P] |
| 23..32 | five (dial, numpad-displays) brightness pairs, one per lighting slot: `00` = follow lighting, `0x80 | v` = fixed v % (0/25/50/75/100) | [C] |

## Commands: VOLATILE (believed RAM-only)

| Packet | Meaning | Conf. |
|---|---|---|
| `11 81 <i> 00 <v>` | Dial metric value. i: 0 CPU, 1 GPU, 2 disk, 3 network MB/s, 4 RAM | [C] |
| `11 83 00 00 <level>` | Dial volume readout | [C] |
| `11 80 00 00 01`, then `11 84 00 00`, then `11 84 00 01 00 00 MM DD HH MM SS <style>` | Set dial clock | [C] (meaning of `11 80` on its own [U]) |
| `14 00 00 00 <profile 1..5> <slot>` | Switch active profile and lighting slot (`SwitchProfile`). Whether the switch persists across replug: [U] | [C] |
| `14 2C 0A 00 FF <brightness> 00` | Enter custom (per-key) mode in slot 6 | [C] |
| `14 2C 00 01 <ix 0..7> <brightness> 00 <19 x RGB>` | Per-key colours, 8 packets x 19 keys (152 slots, 126 used). RGB triplets from byte 7. | [C] |
| `14 2D 0A 00 <ix 0..2> FF 00 <RGB...>` | Side-strip LEDs, 3 packets (19, 19, 7 LEDs = 45) | [C] |
| `14 A0 <0..2> 01` | Bind keys to the static slot after a custom frame (exact meaning [U]) | [P] |

Per-key frames can be streamed at about 20-30 fps, sending only the packets that changed. [C]

## Commands: PERSIST (flash)

| Packet | Meaning | Conf. |
|---|---|---|
| `14 2C <effect> 00 <speed> <brightness> ...` | Built-in effect parameters for the current slot (details below). Persists only after `13 55`. | [C] |
| `13 55 00 00 <slot>` | Save the slot to flash (`SaveFlash`) | [C] |
| `11 14` write form | Dial mode, dock colour, display brightness, screensaver. Layout below. Persistence assumed, not proven. | [C] |
| `12 08 00 <key 1..4>` then `17 AA <len> 00 <type> <ascii>` | Store a host action for display key D1..D4 (`len` = 1 + string length, max 55 chars; type `0x02` URL, `0x04` shell). Also arms the D-key event reports. | [C] |
| `11 00` then `11 02 00 01 01 <iid> 02` | Select a built-in icon for a display key; `iid = 0x40 + key*9 + variant` | [P] |
| `14 20 <key> 00 <newkey>` | Remap a key in the current profile (matrix codes) | [P], not hardware-verified by any source |
| `14 21 <key> 00 <newkey> <mods>` | Remap with modifier bitmask | [P], not hardware-verified |
| `13 41 00 00 01` | `ResetMMDockPic`: dial picture back to the factory logo. Phase 3 only, explicit user command. | [C] |
| `13 42 00 00 <b1> <b2> <b3> <b4> <b5>` | `ResetNumpadPic`: D1..D4 pictures back to factory, one key bitmap per profile (bit n = key Dn+1, `0F` = all four). Restores the factory D-key shortcuts too. Phase 3 only, explicit user command. | [C] |

Picture resets (`13 41`, `13 42`) only touch the dial and display-key
pictures. They were approved for use by the project owner on 2026-10-30.
The general flash reset `13 40` stays denied.

### `11 14` write form

Built from the current read form, never from scratch:

| Byte | Value |
|---|---|
| 0..1 | `11 14` |
| 3 | `01` (write flag) |
| 4 | `02` |
| 5 | `FF` |
| 7..9 | echo of reply bytes 7..9 (the firmware ignores foreign values) |
| 10 | dial mode byte |
| 11, 13 | `1E` |
| 15 | `01` |
| 16..19 | `12 13 14 15` |
| 20 | `02` |
| 22 | `32` |
| 23..32 | brightness pairs (as in the read form) |

Echoing the read reply with the write flag does **not** work: the reply has
`01` in bytes 4 and 20 (attached), the write needs `02`. [C] The meaning of the
fixed bytes 11..22 is [U]; we treat them as opaque constants taken from Base
Camp's behaviour and confirm them by capture before use.

### Built-in effects (`14 2C`)

Each effect lives in a fixed slot of the active profile. Applying one is
`14 00` (switch to the slot) then `14 2C` (parameters) then `13 55` (save). [C]

| Effect | Slot | Effect id | Speed table (slow to fast, 5 steps) | Colour modes |
|---|---|---|---|---|
| Static | 0 | `00` | n/a (`FF`) | single |
| Wave | 1 | `04` | 10 9 8 7 6 | single, dual, 4 stops, rainbow |
| Tornado | 2 | `07` | 10 9 8 7 6 | single, rainbow |
| Breathing | 3 | `01` | 5 4 3 1 0 | single, dual, rainbow |
| Reactive | 4 | `03` | 5 4 3 1 0 | key + background |
| Matrix | 5 | `09` | 20 15 10 5 0 | key + background |
| Custom | 6 | `0A` | n/a | per-key |
| Yeti | 7 | `06` | 10 7 5 3 0 | key + background |
| Off | 8 | `0C` | n/a | none |

Common bytes: 2 effect id, 3 `00`, 4 speed, 5 brightness 0..100. [C]

- **Wave/Tornado** ("block data"): byte 6 random flag, 7 direction (wave: 0
  right, 2 down, 4 left, 6 up; tornado: 9 clockwise, 10 anticlockwise),
  8 width, 9 number of stops, then stops of 4 bytes `<pos %> R G B` from
  byte 10. Rainbow: 6=`02`, 8=`02`, 9=`00`, 10=`FF`, 14=`FF`. Dual colour is sent
  as four stops c1 c2 c1 c2 at 25/50/75/100 %. [C]
- **Other effects** ("effect data"): byte 6 mode (`00` normal, `02` rainbow,
  `10` dual breathing), 7..8 `FF FF`, 9..11 colour 1, 12..14 colour 2 (dual
  breathing), 18..20 background colour (reactive/matrix/yeti). [C]
- **Off**: bytes 6..8 `FF FF FF`. [C]

### LED map

- Key LED index = `column * 9 + row`, independent of language. [C]
- ANSI vs ISO only changes which indices exist: ANSI has `\` at 119 and a wide
  Enter at 120; ISO has tall Enter 120, `#` 111 and the extra key next to Z at 13. [C]
- Side strips are 45 LEDs (main board 30, numpad 15). The order around the
  board is documented in everest-mac; we will re-derive it from a capture
  before relying on it. [P]
- Base Camp's own legend tables contain index errors for some layouts (keys on
  F1/F11/F12 LEDs in German, Italian, Nordic, Spanish). [P]

## Commands: TRANSFER (pictures) — phase 3 only

**This uses the same mechanism as the firmware update.** We implement exactly
the sequence Base Camp uses, with fixed targets, and never send other target
or sector values.

Image format: RGB565, little-endian, row-major (row order [P]).

| Target | Size | Bytes |
|---|---|---|
| Display key D1..D4 | 72 x 72 | 10 368 |
| Dial | 240 x 204 | 97 920 |

Sequence (feature reports, each followed by a GET_REPORT):

1. `aa 55 21 <target>` with target `04` = display keys, `03` = dial. Expect `aa 55 21 fa`. [C]
2. `aa 55 22 00`. Expect `aa 55 22 fa`. [C]
3. Descriptor `aa 55 10 <size: 3 bytes LE> <checksum: 2 bytes LE> 00 00 02 <profile 1..5> <key 0..3>`
   (dial: profile 1, key 0). **Checksum = sum of all image bytes, modulo 65536.**
   Re-send the descriptor on every `aa 55 10 fb` (busy: the keyboard is erasing
   the old picture) until `aa 55 10 fa`. `fe` = rejected, abort. [C]
4. 64-byte chunks (last one zero-padded). Each answer is
   `aa 55 10 fa <bytes received: 3 LE> ... `; on `fb` resend the same chunk.
   Offset 9 = `fe` in the final answer means done. [C]

Rules:

- There is **no host-side erase**. `aa 55 21 xx` only selects a target. Other
  target values (e.g. `05`+) are rejected by the keyboard and must never be sent. [C]
- A started transfer must be finished: an abandoned one leaves the keyboard
  waiting for data and the interrupt channel answers `ff aa` until replug.
  Validate everything (size, target, profile) **before** the first packet. [C]
- On firmware 57 a key picture takes ~20 s; older firmware is far slower. [C]
- All other traffic must pause during a transfer (process-wide lock). [C]
- After a picture reset the keyboard restores Windows-style shortcuts on the
  D keys (`Win+1`...). Phase 3 decides whether we neutralise them. [C]

## Commands: DENY (never sent by this project)

| Packet | What it is | Why denied |
|---|---|---|
| `aa 55 7f` | Base Camp `ResetDevice` | Reset/recovery |
| `13 40 00 00 <arg>` | `ResetFlash` | Factory reset of flash |
| any other `aa 55 xx`, or `aa 55 21 xx` with xx not in {03, 04} | Firmware update / unknown flash ops | Firmware/bootloader |
| any command not listed in this document | Unknown | Rule: never send what we do not understand |

If the flash gets wedged after an interrupted transfer, the safe recovery is
**unplug and replug**. Settings and pictures stay in flash.

## Events (device to host)

| Packet | Meaning | Conf. |
|---|---|---|
| `01 ...`, byte 42 = `02`/`04`/`08`/`10` | D1/D2/D3/D4 pressed. Only reported once an action has been written for the key. | [C] |
| `01 ...` other bytes | Full key matrix events | [U] |
| `ff aa` | Display refreshed / channel busy after a transfer | [P] |

## Coexistence with Base Camp

Windows processes and services that talk to the keyboard [C]:

- `Base Camp.exe` (Electron UI)
- `BaseCamp.Service.exe`, Windows service `BaseCampService`
- `MountainDisplayPadWorker.exe`

Base Camp also keeps its profiles in `BaseCamp.db` (SQLite, in the install
directory's `resources\bin\`). Tables seen: profiles, keyboard lightings,
key bindings. Possible import source for phase 4. [P]

On Linux, other community tools (BaseCamp-Linux, a Wine'd Base Camp) can hold
the same hidraw node; detect them by scanning `/proc/*/fd` for the node. [P]

## Known unknowns (to fill by capture)

1. Full `11 00` reply layout (other bytes, module firmware versions).
2. Firmware versions other than 57: what changes, and which versions are in use in the field.
3. Nordic layout (code 11) and its legends, unverified on hardware.
4. Whether `14 00` (profile switch) persists across replug.
5. Meaning of the constant bytes 11..22 in the `11 14` write form.
6. How to **read back** key remaps, macros and per-key colours from the
   keyboard. No source documents a read command. If none exists, phase 4
   "read profiles" can only mean reading our own files or Base Camp's DB.
7. Macro storage format (Base Camp has macros; no protocol documented).
8. Key remap `14 20`/`14 21`: matrix code table and hardware behaviour.
9. Full key-event layout in `01` reports.
10. Dial "custom" menu entry (bit 7 of the menu mask) and dock button functions.
11. Everest Core behaviour (no modules).
12. Pixel row order and orientation of the dial image.

## Sources and licences

| Source | Licence | How we use it |
|---|---|---|
| [Amorem/everest-mac](https://github.com/Amorem/everest-mac) (Swift, macOS) | MIT | Main protocol reference. MIT is compatible with any licence we pick; we still write our own implementation (different language) and credit the project. |
| [FrankieDedo/K2](https://github.com/FrankieDedo/K2) (C#, Windows) | **No licence file** = all rights reserved | Facts only (Windows interface layout, Base Camp process names, `BaseCamp.db`). No code, no text copied. |
| [ramisotti13-eng/BaseCamp-Linux](https://github.com/ramisotti13-eng/BaseCamp-Linux) (Python, Linux) | GPL-3.0 **plus a non-commercial restriction** (not OSI open source) | Facts only (endpoint numbers, udev approach, `GetExtendInfo` field order). No code, no text copied. |
| Mountain DisplayPad SDK (bundled in K2) | MIT (Mountain) | Not used yet; relevant only if we add DisplayPad support. |

Protocol facts (byte values, field order) are not copyrightable expression;
this document states them in our own words and our own structure.
