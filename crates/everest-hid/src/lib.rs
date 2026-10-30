//! HID access to the Mountain Everest Max.
//!
//! - [`discovery`]: list the keyboard's HID interfaces and pick the vendor
//!   command collection (interface 3, usage page `0xFF00`).
//! - [`transport`]: the [`Transport`] trait, the hidapi implementation and
//!   the report-ID handling that differs between operating systems.
//! - [`session`]: the only way to talk to the keyboard. It enforces the
//!   safety-class allowlist and matches replies to queries.
//! - [`coexist`]: detect Base Camp and other programs using the keyboard.
//! - [`mock`]: a scripted transport for tests.

pub mod coexist;
pub mod discovery;
pub mod mock;
pub mod session;
pub mod transport;

pub use discovery::{HidInterface, InterfaceRole};
/// Re-exported so callers use the same hidapi build (and backend) as this crate.
pub use hidapi;
pub use session::{Policy, Session};
pub use transport::{HidTransport, Transport};

use everest_proto::Classification;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HID error: {0}")]
    Hid(String),
    #[error("no Mountain Everest Max found (USB 3282:0001, interface 3)")]
    NotFound,
    #[error("refused to send {:?} ({}): the current policy does not allow {:?} commands", .packet, .classification.name, .classification.class)]
    Refused {
        packet: everest_proto::Packet,
        classification: Classification,
    },
    #[error("the keyboard did not answer {:02x} {:02x}", .0[0], .0[1])]
    NoReply([u8; 2]),
}

pub type Result<T> = std::result::Result<T, Error>;
