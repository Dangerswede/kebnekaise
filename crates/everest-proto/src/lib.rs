//! Packet builders and parsers for the Mountain Everest Max HID protocol.
//!
//! This crate does no I/O. It is the single source of truth for which bytes
//! may be sent to the keyboard: every outgoing [`Packet`] is built here and
//! carries a [`SafetyClass`], and [`classify`] tells the transport (and the
//! capture decoder) what an arbitrary packet is. See `docs/PROTOCOL.md`.

pub mod actions;
mod classify;
mod info;
pub mod keymap;
mod layout;
pub mod ledmap;
pub mod lighting;
mod packet;
pub mod pictures;

pub use actions::{ActionError, ActionText, DKey, StoredKind};
pub use classify::{classify, Classification, Group, SafetyClass};
pub use info::{
    parse_layout, DialMode, DisplayBrightness, ExtendedInfo, FirmwareInfo, FirmwareVersion,
    LightingSlot, ModuleSet, ParseError,
};
pub use keymap::{KeyPos, KeymapError};
pub use layout::{Layout, LayoutShape};
pub use lighting::{ColorMode, CustomFrame, Direction, Effect, EffectSettings, LightingError, Rgb};
pub use packet::{is_reply_to, Packet, PACKET_SIZE};
pub use pictures::{
    ImageChunk, KeyResetMask, Picture, PictureError, PictureTarget, TransferReply, TransferStatus,
};

/// USB vendor ID used by Mountain.
pub const VENDOR_ID: u16 = 0x3282;
/// USB product ID of the Everest Max (and the Everest Core).
pub const PRODUCT_ID_EVEREST_MAX: u16 = 0x0001;
/// USB interface number of the vendor command collection.
pub const VENDOR_INTERFACE: i32 = 3;
/// HID usage page of the vendor command collection.
pub const VENDOR_USAGE_PAGE: u16 = 0xFF00;
/// HID usage of the vendor command collection.
pub const VENDOR_USAGE: u16 = 0x0001;

/// Firmware versions on which writing commands have been tested.
///
/// Writes on any other version need an explicit `--force` from the user.
pub const TESTED_FIRMWARE: &[u8] = &[0x57];

/// Other Mountain products sharing the vendor ID. They speak different
/// protocols and must never be mistaken for the keyboard.
pub const OTHER_MOUNTAIN_PRODUCTS: &[(u16, &str)] = &[
    (0x0002, "Makalu Max"),
    (0x0003, "Makalu 67"),
    (0x0005, "Everest 60 (ANSI)"),
    (0x0006, "Everest 60 (ISO)"),
    (0x0008, "MacroPad"),
    (0x0009, "DisplayPad"),
];
