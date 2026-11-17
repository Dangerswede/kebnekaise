//! A virtual keyboard through Linux `/dev/uinput`.
//!
//! The device exists while this value lives; closing the file removes it,
//! and the kernel releases any key still held. Uses only the documented
//! uinput ioctls (`UI_SET_EVBIT`, `UI_SET_KEYBIT`, `UI_DEV_SETUP`,
//! `UI_DEV_CREATE`, `UI_DEV_DESTROY`).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

use super::hostkeys::Event;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0;
const BUS_VIRTUAL: u16 = 0x06;

// _IOW('U', 100, int), _IOW('U', 101, int), _IO('U', 1), _IO('U', 2),
// _IOW('U', 3, struct uinput_setup) with sizeof(uinput_setup) = 92.
const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;

/// Pause after each key event, so applications see separate presses.
const EVENT_PAUSE: Duration = Duration::from_millis(4);

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct InputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

pub struct VirtualKeyboard {
    file: File,
}

fn ioctl(fd: i32, request: libc::c_ulong, arg: libc::c_ulong) -> std::io::Result<()> {
    // SAFETY: fd is an open uinput file; every request used here takes an
    // int or a pointer to a live, correctly sized struct.
    let r = unsafe { libc::ioctl(fd, request as _, arg) };
    if r < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl VirtualKeyboard {
    /// Creates the device with every code in `codes`.
    pub fn create(codes: &[u16]) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/dev/uinput")?;
        let fd = file.as_raw_fd();
        ioctl(fd, UI_SET_EVBIT, EV_KEY as _)?;
        for c in codes {
            ioctl(fd, UI_SET_KEYBIT, *c as _)?;
        }
        let mut setup = UinputSetup {
            id: InputId {
                bustype: BUS_VIRTUAL,
                vendor: 0,
                product: 0,
                version: 1,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name = b"Kebnekaise agent virtual keyboard";
        setup.name[..name.len()].copy_from_slice(name);
        ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup as _)?;
        ioctl(fd, UI_DEV_CREATE, 0)?;
        // The desktop needs a moment to pick up a new input device; events
        // sent before that are lost.
        std::thread::sleep(Duration::from_millis(500));
        Ok(VirtualKeyboard { file })
    }

    fn emit(&mut self, kind: u16, code: u16, value: i32) -> std::io::Result<()> {
        let ev = InputEvent {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            kind,
            code,
            value,
        };
        // SAFETY: InputEvent is repr(C) plain data; we view it as bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &ev as *const InputEvent as *const u8,
                std::mem::size_of::<InputEvent>(),
            )
        };
        self.file.write_all(bytes)
    }

    /// Plays events in order.
    pub fn play(&mut self, events: &[Event]) -> std::io::Result<()> {
        for e in events {
            match *e {
                Event::Down(code) | Event::Up(code) => {
                    let value = i32::from(matches!(e, Event::Down(_)));
                    self.emit(EV_KEY, code, value)?;
                    self.emit(EV_SYN, SYN_REPORT, 0)?;
                    std::thread::sleep(EVENT_PAUSE);
                }
                Event::SleepMs(ms) => std::thread::sleep(Duration::from_millis(ms.into())),
            }
        }
        Ok(())
    }
}

impl Drop for VirtualKeyboard {
    fn drop(&mut self) {
        let _ = ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_the_kernel() {
        // The ioctl number encodes sizeof(struct uinput_setup) = 92.
        assert_eq!(std::mem::size_of::<UinputSetup>(), 92);
        assert_eq!(
            std::mem::size_of::<InputEvent>(),
            std::mem::size_of::<libc::timeval>() + 8
        );
    }
}
