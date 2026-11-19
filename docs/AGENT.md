# The agent (`kebnekaise agent`)

The keyboard cannot run programs or type macros. Once a display key is
armed (`kebnekaise key arm D1`), it only reports that it was pressed. The
agent runs on the computer, listens for those presses and carries out the
`[dkeys]` actions of a profile file (format in `PROFILES.md`).

```
kebnekaise agent work.toml             # run until Ctrl+C
kebnekaise agent work.toml --dry-run   # only print what each press would do
kebnekaise agent work.toml --layout us # the computer uses a US layout
```

## What it does

| action | How |
|---|---|
| `url` | `xdg-open <url>` (Windows: `rundll32 url.dll,FileProtocolHandler`) |
| `run` | Starts the program directly, without a shell, and does not wait |
| `keys` | Presses the combination on a virtual keyboard (Linux uinput) |
| `macro` | Plays the steps on the virtual keyboard |
| `profile` | Sends `14 00` to switch the keyboard profile |

The agent writes nothing to the keyboard, with one exception: `profile`
sends the profile switch (`14 00`, see `PROTOCOL.md`). If the active
lighting slot is unknown (`ff`, an unused profile), it uses slot 0.

A press is acted on once, when the key goes down. While a macro is still
playing, another key or macro press is ignored and logged.

Other keys are never printed or stored. The agent only reads byte 42 of
the event report.

If the keyboard is unplugged, the agent waits and reconnects.

## Typing needs the computer's layout

The virtual keyboard sends key codes, and the computer turns them into
characters with its own layout. To type `@`, the agent must know that it is
AltGr+2 on a Swedish layout and Shift+2 on a US layout. `--layout`
(default `se`) chooses the table. The agent checks every macro at start and
refuses to run if a character has no key on that layout. Both tables
include all printable ASCII. `se` also has `åäöÅÄÖ§½¤£€´¨`. Dead keys
(´ ¨ ^ ~ and the backtick) are typed as the key and then a space.

Some Linux applications ignore dead keys altogether, also when typed by
hand. If ~ does not work by hand either, choose the layout "Swedish (no
dead keys)" in the desktop settings and run the agent with
`--layout se-nodeadkeys`.

## Linux: access to /dev/uinput

`keys` and `macro` need write access to `/dev/uinput`. Some systems
already grant it, for example when Steam is installed. Check:

```
test -w /dev/uinput; and echo ok    # fish
test -w /dev/uinput && echo ok      # bash
```

If not, install the rule, which gives the logged-in user access:

```
sudo cp packaging/linux/71-kebnekaise-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo modprobe -r uinput; sudo modprobe uinput
```

If `test -w` still fails, log out and in again.

`url`, `run` and `profile` do not need uinput. The agent only opens
`/dev/uinput` when the file has a `keys` or `macro` action.

## Windows

`url`, `run` and `profile` work. `keys` and `macro` are not implemented on
Windows yet (planned: `SendInput`). The agent refuses to start if the file
uses them.

## Starting it automatically

Not packaged yet. On Linux, a systemd user service works:

```
# ~/.config/systemd/user/kebnekaise-agent.service
[Unit]
Description=Kebnekaise display-key agent

[Service]
ExecStart=%h/Projekt/kebnekaise/target/release/kebnekaise agent %h/.config/kebnekaise/profile.toml
Restart=on-failure

[Install]
WantedBy=default.target
```

```
systemctl --user enable --now kebnekaise-agent
```
