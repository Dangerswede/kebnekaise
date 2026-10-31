# Capturing Base Camp traffic

Status: **Phase 0 draft** (2026-10-30).

We fill the gaps in `PROTOCOL.md` by watching what Mountain's Base Camp sends
to the keyboard, one action at a time. Capturing is passive: it never sends
anything to the keyboard. The decoder that reads the captures runs on Linux and
Windows.

## Privacy warning: captures contain your keystrokes

USB captures of a keyboard record **every key you type** on interfaces 0 and 2,
including passwords. Before you capture:

- Do not type anything sensitive while a capture runs. Use the mouse in Base Camp.
- Only share captures after running them through the decoder's `--strip`
  mode (planned), which keeps interface 3 and the control transfers addressed
  to it and drops everything else.

## Windows: USBPcap + Wireshark

### Install

1. Install [Wireshark](https://www.wireshark.org/download.html) for Windows.
   On the installer's component page, tick **USBPcap**. Reboot when asked
   (USBPcap is a filter driver; it is only active while a capture runs, and it
   does not replace the keyboard's driver).
2. Install Mountain Base Camp if it is not already installed.

### Find the right root hub

1. Plug the keyboard in directly (not through a hub, if possible).
2. Start Wireshark. The interface list shows `USBPcap1`, `USBPcap2`, ...: one
   per USB root hub.
3. To find which one carries the keyboard, run as administrator:

   ```
   "C:\Program Files\USBPcap\USBPcapCMD.exe"
   ```

   It lists each root hub with the devices under it. Note the hub with
   `VID_3282 PID_0001`. (Alternatively: start a short capture on each and see
   which one shows traffic when you press a key.)

### Capture one action per file

The goal is small, labelled captures, so each one shows a single command.

1. Close any other tool talking to the keyboard (ours included).
2. Start Base Camp and wait until it has connected (lighting/dial settled).
3. In Wireshark, start capturing on the hub from above. Optional capture
   setting: under the USBPcap options, untick "capture from all devices" and
   choose only the keyboard's device address, to keep files small.
4. Write down the time, then do **exactly one** thing in Base Camp, for
   example "set Static effect, colour red, brightness 100".
5. Wait 5 seconds, stop the capture.
6. Save as **pcapng** with a name that says what you did:
   `fw57_nordic_static_red_b100.pcapng`.
7. Add a line to `captures/INDEX.md` (planned): file name, firmware, layout,
   what you clicked, anything odd.

Good first captures (each in its own file):

| # | Action in Base Camp | Fills unknown |
|---|---|---|
| 1 | Start Base Camp with the keyboard connected (no clicks) | `11 00` full layout, session start |
| 2 | Switch profile 1 to 2 and back | `14 00` and persistence |
| 3 | Each built-in effect once, default settings | effect packets |
| 4 | Change only brightness | brightness encoding |
| 5 | Per-key colour: paint one key | custom-mode packets, LED index |
| 6 | Remap one key (e.g. CapsLock to Ctrl) | `14 20`/`14 21`, matrix codes |
| 7 | Create and assign a small macro | macro storage (unknown) |
| 8 | Change dial mode, then screensaver timing | `11 14` write constants |
| 9 | Export a profile, then import it | any profile read-back command |
| 10 | Unplug and replug the numpad, then the dock | module attach events |

**Never capture firmware updates.** If Base Camp offers an update, decline.
We do not want those bytes in our captures or in the project.

### Useful Wireshark display filters

```
usb.idVendor == 0x3282                         # descriptor packets of the device
usb.device_address == 7                        # replace 7 with the keyboard's address
usb.endpoint_address == 0x05 || usb.endpoint_address == 0x84   # interface 3 interrupt OUT/IN
usbhid.setup.bRequest == 0x09 && usb.setup.wIndex == 3         # SET_REPORT to interface 3 (feature)
usbhid.setup.bRequest == 0x01 && usb.setup.wIndex == 3         # GET_REPORT from interface 3
```

Interrupt traffic carries the commands; picture transfers show up as control
transfers (SET_REPORT/GET_REPORT, report type Feature) on endpoint 0 with
`wIndex = 3`.

## Linux: capturing too

Base Camp does not run on Linux, but captures can still be made there:

- **Our own tool** (phase 1+): `usbmon` lets us record and check what we send.

  ```
  sudo modprobe usbmon
  lsusb -d 3282:0001          # note the bus number, e.g. Bus 003
  # In Wireshark, capture on usbmon3 (bus 3)
  ```

- **Base Camp in a Windows VM with USB passthrough** (e.g. QEMU/virt-manager or
  VirtualBox) on a Linux host: capture on the host's `usbmonN`. This avoids
  booting into Windows for each capture.

## The decoder

Planned as a subcommand of our tool, sharing the packet definitions with the
driver code so the docs, the driver and the decoder cannot drift apart:

```
kebnekaise capture decode fw57_nordic_static_red_b100.pcapng
kebnekaise capture decode --strip in.pcapng --out shareable.pcapng
kebnekaise capture diff a.pcapng b.pcapng     # what differs between two actions
```

Design:

- Reads pcapng and pcap with link types **USBPcap** (Windows, `LINKTYPE_USBPCAP` 249)
  and **Linux usbmon** (`LINKTYPE_USB_LINUX_MMAPPED` 220). Pure parsing, no
  Wireshark or libpcap needed, so it runs the same on both OSes and in CI.
- Keeps only traffic for VID:PID `3282:0001`, interface 3.
- Prints one line per packet: time, direction, channel (interrupt/feature),
  the command name and decoded fields from `PROTOCOL.md`, and its confidence
  tag. Unknown commands are shown in hex and flagged `UNKNOWN`, which is our
  to-do list.
- `--json` output for scripting; `diff` aligns two captures by command and
  highlights changed bytes, which is how we find field meanings.
- Test fixtures: small hand-made captures in `tests/fixtures/`, so CI checks
  the decoder without hardware.

Optionally, a small **Wireshark Lua dissector** (`tools/wireshark/everest.lua`)
generated from the same command table, for people who prefer to browse in
Wireshark.
