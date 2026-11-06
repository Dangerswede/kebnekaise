# Mountain Everest Max: USB HID protocol

Status: **Phase 2 draft** (2026-11-06). Read commands are verified by this
project on Windows (tag [W]). The first lighting write is approved and
waiting for its hardware test. Every entry carries a
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
| **[W]** seen by us on Windows | Observed by this project on real hardware on Windows; Linux still to do. See [Hardware observations](#hardware-observations). |

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
| 4 | Firmware version, BCD-like: `0x57` = firmware 57 | [W] |
| 8 | `0x06` seen; meaning unknown | [U] |
| 10 | Active profile, 1..5 | [W] |
| 11 | Active lighting slot of that profile (see slots below) | [W] |
| other | zero on the unit seen (possibly sub-versions, numpad/dock firmware) | [U] |

### `11 12`: layout

Reply byte 4 = layout code. Mapping used by Base Camp:

| Code | Layout | Shape | Conf. |
|---|---|---|---|
| 4 | UK | ISO | [C] (only one verified on hardware) |
| 3 | German | ISO | [P] |
| 5 | French | ISO | [P] |
| 8 | Italian | ISO | [P] |
| 11 | Nordic | ISO | [W] (code seen on a Nordic board; legends not yet checked) |
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
| 4 | media dock attached (non-zero = yes; `02` seen with the dock attached) | [W] |
| 5 | dock: show menu | [P] |
| 6 | dock: menu index | [P] |
| 7..9 | dock menu colour R,G,B (must be echoed in the write form) | [W] |
| 10 | screen setup: dial mode (`0x01` image, `0x11` clock, `0x71` volume, `0x91` CPU, `0xA1` GPU, `0xB1` disk, `0xC1` network, `0xD1` RAM, `0xE1` APM). High nibble also described as screensaver type, low bits as screensaver/turn-off enables; the two readings need reconciling. | [P] |
| 11..12 | screensaver delay, seconds, LE (`1e 00` = 30 s seen) | [P] |
| 13..14 | turn-off delay, seconds, LE (`1e 00` = 30 s seen) | [P] |
| 15..19 | "show profile" per profile | [P] |
| 20 | numpad attached (non-zero = yes; `02` seen with the numpad attached) | [W] |
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
Exact sequences, byte layouts and allowlist rules: [Lighting (phase 2)](#lighting-phase-2).

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

Note from our first hardware read (2026-11-06): that unit's read reply had
`02` in bytes 4 and 20, and bytes 11..22 were exactly the "constants" of the
write form (`1e 00 1e 00 01 12 13 14 15 02 00 32`). So these bytes are most
likely the current settings (delays, show-profile, numpad, double-click)
rather than constants, and the write form probably echoes them. To be
confirmed by capture before any write is implemented.

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

## Lighting (phase 2)

This section is the specification `everest-proto` implements for phase 2
(`crates/everest-proto/src/lighting.rs`, `ledmap.rs`). It refines the
lighting rows of the tables above. Status on 2026-11-06: implemented as a dry
run, except sequence A for a static colour without step 3, which the project
owner approved for the first hardware test (`rgb static ... --no-save`).

Source: everest-mac (MIT), which recovered the sequences from Base Camp
1.9.10 (`SDKDLL.dll`, `BaseCamp.UI.dll`) and verified them on a UK-ISO board
with firmware 57. Tags below are relative to that: [C] = everest-mac
hardware-verified, [P] = everest-mac only from disassembly, [U] = unknown.

### What the keyboard offers (and does not)

- **No lighting read-back.** The only lighting state we can read is the
  active profile and lighting slot (`11 00` bytes 10 and 11). Effect
  parameters, colours and brightness cannot be read. [C] (no source has a
  read command)
- **No separate brightness command.** Brightness is byte 5 of every effect
  packet. Changing only the brightness means re-sending the whole effect.
  Since the current parameters cannot be read, a brightness change needs the
  rest of the effect from the user (or, later, from our own profile file). [C]
- Every effect lives in its own slot of the active profile, so applying an
  effect also makes that slot active. [C]

### Sequence A: built-in effect (static colour included)

| # | Packet | Class | Pause after |
|---|---|---|---|
| 1 | `14 00 00 00 <profile> <slot>` switch to the effect's slot | VOLATILE | 400 ms |
| 2 | `14 2C <effect id> 00 ...` effect parameters (below) | PERSIST* | 250 ms |
| 3 | `13 55 00 00 <slot>` save the slot to flash (optional) | PERSIST | 500 ms |
| 4 | `11 00` read back: expect `<profile>` in byte 10 and `<slot>` in byte 11 | READ | — |

`<profile>` is read from `11 00` byte 10 just before step 1, so the active
profile does not change. Base Camp always sends step 3 [C]. *Step 2 alone is
believed to be lost on replug (that is why Base Camp saves), but this is
[U]; our first hardware test leaves out step 3 to find out. Pauses are
everest-mac's (300 ms + 120 ms, 250 ms) rounded up; the keyboard can need up
to ~3 s after a settings write [C], so step 4 retries before failing.

### Sequence B: per-key colours (custom mode)

| # | Packet | Class | Pause after |
|---|---|---|---|
| 1 | `14 00 00 00 <profile> 06` switch to the custom slot | VOLATILE | 400 ms |
| 2 | `14 2C 0A 00 FF 64 00` enter custom mode, brightness 100 | VOLATILE | 400 ms |
| 3..10 | `14 2C 00 01 <ix 0..7> 64 00 <19 x RGB>` key colours | VOLATILE | 30 ms |
| 11..13 | `14 2D 0A 00 <ix 0..2> FF 00 <19/19/7 x RGB>` side strips | VOLATILE | 30 ms |
| 14..16 | `14 A0 <0..2> 01` bind keys to the static slot | VOLATILE | 100 ms |
| 17 | `13 55 00 00 06` save the custom picture (optional) | PERSIST | 500 ms |

[C] for the order and bytes. Brightness is applied on the host by scaling the
colours, with the brightness bytes of steps 2..10 fixed at 100 (`0x64`), as
everest-mac does. What the firmware does with other values there is [U].
For animation, only packets whose content changed are re-sent (20..30 fps
[C]); step 17 must not be repeated per frame (flash wear).

`14 A0` "binds every key to the static slot" in everest-mac's words; its real
meaning is [U]. It is sent once, after the first frame.

### Packet layouts

`14 2C` effect parameters (64 bytes, zero-padded):

| Byte | Content |
|---|---|
| 0..1 | `14 2C` |
| 2 | effect id (table under "Built-in effects": `00 01 03 04 06 07 09 0C`) |
| 3 | `00` |
| 4 | speed: hardware value from the effect's speed table; `FF` for static and off |
| 5 | brightness 0..100 |
| 6.. | block data (wave, tornado) or effect data (the rest), below |

Block data (wave, tornado):

| Mode | 6 | 7 | 8 | 9 | 10.. |
|---|---|---|---|---|---|
| rainbow | `02` | direction | `02` | `00` | `FF 00 00 00 FF` |
| single | `00` | direction | `00` | `01` | `64 R G B FF` |
| dual (c1 c2) | `00` | direction | `02` | `04` | `19 c1 32 c2 4B c1 64 c2` |
| quad (wave only) | `00` | direction | `02` | `04` | `19 c1 32 c2 4B c3 64 c4` |

Stops are `<position %> R G B`. Direction: wave `00` right, `02` down, `04`
left, `06` up; tornado `09` clockwise, `0A` anticlockwise. [C] for rainbow,
single and dual; quad is the firmware's native four-stop form, [P].

Effect data (static, breathing, reactive, matrix, yeti):

| Mode | 6 | 7..8 | 9..11 | 12..14 | 18..20 |
|---|---|---|---|---|---|
| single (static, breathing) | `00` | `FF FF` | colour | 0 | 0 |
| dual (breathing) | `10` | `FF FF` | colour 1 | colour 2 | 0 |
| rainbow (breathing) | `02` | `FF FF` | 0 | 0 | 0 |
| key + background (reactive, matrix, yeti) | `00` | `FF FF` | key colour | 0 | background |

Off: bytes 6..8 `FF FF FF`, nothing else. [C]

Speed: the CLI takes 1 (slowest) to 5 (fastest) and maps it to the five
hardware steps of the effect (Base Camp's 0..100 slider thresholds are
12/37/62/87). Smaller hardware values are faster:

| Effect | 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| wave, tornado | 10 | 9 | 8 | 7 | 6 |
| breathing, reactive | 5 | 4 | 3 | 1 | 0 |
| matrix | 20 | 15 | 10 | 5 | 0 |
| yeti | 10 | 7 | 5 | 3 | 0 |

### Allowlist rules

The classifier (`classify.rs`) only accepts lighting packets in exactly these
forms; any other value is `DENY` and never sent:

| Packet | Accepted only when |
|---|---|
| `14 00` | bytes 2..3 `00`, profile 1..5, slot 0..8, rest zero |
| `13 55` | bytes 2..3 `00`, slot 0..8, rest zero |
| `14 2C 0A 00` | byte 4 `FF`, brightness ≤ 100, rest zero |
| `14 2C 00 01` | packet index 0..7, brightness ≤ 100, byte 6 `00` |
| `14 2C <id> 00` | id one of the eight effect ids, brightness ≤ 100 |
| `14 2D 0A 00` | packet index 0..2, byte 5 `FF`, byte 6 `00`, zero after the packet's LEDs |
| `14 A0` | chunk 0..2, byte 3 `01`, rest zero |

Each classification also carries a feature group. A session opened with the
`Lighting` policy may send `READ` packets and `VOLATILE`/`PERSIST` packets of
the lighting group, nothing else: no settings write, display values, actions,
remaps, pictures or resets.

### Key LED map

Index = `column * 9 + row`. Names are physical positions on a US board; on a
Nordic board `semicolon` is Ö, `quote` is Ä, `lbracket` is Å, `grave` is §,
`iso-extra` is `<>|` and `iso-hash` is `'*`. [C] on UK-ISO (everest-mac,
LED by LED); [P] on Nordic until we see it.

| Row | Keys (name = LED index) |
|---|---|
| F row | esc 0, f1 9, f2 18, f3 27, f4 36, f5 45, f6 54, f7 63, f8 72, f9 81, f10 90, f11 99, f12 108, prtsc 117, scrlk 114, pause 123 |
| Numbers | grave 1, 1 10, 2 19, 3 28, 4 37, 5 46, 6 55, 7 64, 8 73, 9 82, 0 91, minus 100, equal 109, backspace 87, insert 96, home 105, pgup 115 |
| Top | tab 2, q 11, w 20, e 29, r 38, t 47, y 56, u 65, i 74, o 83, p 92, lbracket 101, rbracket 110, enter 120, delete 88, end 97, pgdn 106 |
| Home | caps 3, a 12, s 21, d 30, f 39, g 48, h 57, j 66, k 75, l 84, semicolon 93, quote 102, iso-hash 111 (ISO) |
| Bottom | lshift 4, iso-extra 13 (ISO), z 22, x 31, c 40, v 49, b 58, n 67, m 76, comma 85, period 94, slash 103, rshift 121, up 124 |
| Space | lctrl 5, lwin 14, lalt 23, space 41, ralt 68, rwin 77, fn 86, rctrl 95, left 104, down 113, right 122 |
| Numpad | numlock 6, kp-slash 24, kp-star 16, kp-minus 15, kp7 61, kp8 69, kp9 70, kp-plus 7, kp4 51, kp5 52, kp6 60, kp1 34, kp2 42, kp3 43, kp-enter 33, kp0 78, kp-dot 79 |

ANSI boards have `backslash` 119 instead of `iso-hash` and `iso-extra`.
The other slots of the 152-slot frame are unused and sent black.

Side strips, 45 LEDs in their own index space (Base Camp's matrix ids
126..170): main board top 13 14 15 7 6 5 4 3 2 1 0, bottom 20..30 12, left
16..19, right 9 8 10 11; numpad top 44 43 42, bottom 35 36 37, left 31..34,
right 41 40 39 38. [P] for us (re-check on hardware; the order is irregular).

### To find out at the first hardware test

1. ~~Does an effect sent without `13 55` survive a replug?~~ No: it is lost
   on replug (seen 2026-11-06, see Hardware observations).
2. Does `14 00` (profile/slot switch) survive a replug?
3. Does `13 55 00 00 06` keep the per-key picture?
4. Nordic LED positions: `iso-extra`, `iso-hash`, `grave`.
5. Side-strip order.
6. Are the pauses long enough? Does the read-back see the new slot at once?

First write (approved 2026-11-06): `kebnekaise rgb static <colour>
--no-save` on the active profile (sequence A without step 3).
It switches to the static slot and sets one colour, both believed to be RAM
only; unplugging the keyboard should bring back the saved lighting.

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
3. Nordic layout legends (code 11 itself is confirmed on a Nordic board).
4. Whether `14 00` (profile switch) persists across replug.
5. Meaning of bytes 11..22 of `11 14` (probably current settings, see the note under the write form).
13. Byte 8 of the `11 00` reply (`06` seen).
6. How to **read back** key remaps, macros and per-key colours from the
   keyboard (lighting has no read-back either, see [Lighting (phase 2)](#lighting-phase-2)). No source documents a read command. If none exists, phase 4
   "read profiles" can only mean reading our own files or Base Camp's DB.
7. Macro storage format (Base Camp has macros; no protocol documented).
8. Key remap `14 20`/`14 21`: matrix code table and hardware behaviour.
9. Full key-event layout in `01` reports.
10. Dial "custom" menu entry (bit 7 of the menu mask) and dock button functions.
11. Everest Core behaviour (no modules).
12. Pixel row order and orientation of the dial image.

## Hardware observations

### 2026-11-06, Windows 11, Nordic Everest Max, firmware 57

Numpad and media dock attached, Base Camp closed (process and service).

- `kebnekaise list`: six HID collections. `MI_00` keyboard (0x0001/0x0006),
  `MI_01` Col01..03 (0x000C/0x0001, 0x0001/0x0080, 0x0001/0x0002),
  `MI_02` keyboard, `MI_03` vendor 0xFF00/0x0001. Matches the interface table.
- The first `info` right after Base Camp had been stopped got **no reply at
  all** to `11 12` (three tries, 800 ms each). After unplugging and replugging
  the keyboard, every query was answered at once. Base Camp may leave the
  vendor channel in a state that only a replug clears; to be confirmed.
- Raw replies after the replug:

```
11 00  11 00 00 00 57 00 00 00 06 00 01 00 00 ...
11 12  11 12 00 00 0b 00 00 ...
11 14  11 14 00 00 02 bf 00 20 53 a9 01 1e 00 1e 00 01 12 13 14 15 02 00 32 00 00 ...
```

Decoded: firmware 57, profile 1, lighting slot 0 (static), layout code 11
(Nordic), dock and numpad attached, dial mode image, dial colour `#2053a9`,
display brightness "follow lighting" in all five slots.

### 2026-11-06, first lighting write (same keyboard, Windows 11)

`kebnekaise rgb static ff0000 --no-save`, then `00ff00 --brightness 20`,
on profile 1 with the static slot already active:

```
write 14 00 00 00 01                      (switch: profile 1, slot 0)
write 14 2c 00 00 ff 64 00 ff ff ff       (static, brightness 100, red)
read  14 00 00 00 01
read  14 2c 00 00 ff 64 00 ff ff ff
read  11 00 ... 57 ... 01 00              (profile 1, slot 0)
```

- The keys turned red, then dim green. Sequence A without `13 55` and the
  static `14 2C` layout are [W].
- **The keyboard echoes each lighting write** back on the interrupt IN
  endpoint, byte for byte, about 250 ms later. Readers must skip these echoes
  (the session already does, since they do not match the awaited command).
  Whether an echo means "applied" is [U].
- The side strips changed colour together with the keys: the static effect
  covers them. [W]
- After unplugging and replugging, the keyboard showed the saved colour
  again (red). So `14 2C` without `13 55` lives in RAM only. [W]

## Sources and licences

| Source | Licence | How we use it |
|---|---|---|
| [Amorem/everest-mac](https://github.com/Amorem/everest-mac) (Swift, macOS) | MIT | Main protocol reference. MIT is compatible with any licence we pick; we still write our own implementation (different language) and credit the project. |
| [FrankieDedo/K2](https://github.com/FrankieDedo/K2) (C#, Windows) | **No licence file** = all rights reserved | Facts only (Windows interface layout, Base Camp process names, `BaseCamp.db`). No code, no text copied. |
| [ramisotti13-eng/BaseCamp-Linux](https://github.com/ramisotti13-eng/BaseCamp-Linux) (Python, Linux) | GPL-3.0 **plus a non-commercial restriction** (not OSI open source) | Facts only (endpoint numbers, udev approach, `GetExtendInfo` field order). No code, no text copied. |
| Mountain DisplayPad SDK (bundled in K2) | MIT (Mountain) | Not used yet; relevant only if we add DisplayPad support. |

Protocol facts (byte values, field order) are not copyrightable expression;
this document states them in our own words and our own structure.
