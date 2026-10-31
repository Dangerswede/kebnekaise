use everest_proto::{
    OTHER_MOUNTAIN_PRODUCTS, PRODUCT_ID_EVEREST_MAX, VENDOR_ID, VENDOR_INTERFACE, VENDOR_USAGE,
    VENDOR_USAGE_PAGE,
};
use serde::Serialize;

use crate::{Error, Result};

/// One HID interface (one top-level collection on Windows) of a Mountain device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HidInterface {
    /// OS path to open: `/dev/hidrawN` on Linux, a `\\?\HID#...` path on Windows.
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    /// USB interface number, `-1` if the OS does not report it.
    pub interface_number: i32,
    /// `0` when the backend could not read the report descriptor.
    pub usage_page: u16,
    pub usage: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub role: InterfaceRole,
}

/// What an interface is for, as far as we know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceRole {
    /// Interface 3, usage page 0xFF00: the command channel.
    VendorCommands,
    /// Interface 3 without usage information (some Linux kernels).
    VendorCommandsProbable,
    /// Typing, media keys, mouse: owned by the OS, never opened by us.
    Input,
    /// Another Mountain product (different protocol).
    OtherProduct(&'static str),
}

impl HidInterface {
    fn classify_role(
        product_id: u16,
        interface_number: i32,
        usage_page: u16,
        usage: u16,
    ) -> InterfaceRole {
        if product_id != PRODUCT_ID_EVEREST_MAX {
            let name = OTHER_MOUNTAIN_PRODUCTS
                .iter()
                .find(|(pid, _)| *pid == product_id)
                .map_or("unknown Mountain product", |(_, n)| n);
            return InterfaceRole::OtherProduct(name);
        }
        match (interface_number, usage_page, usage) {
            (VENDOR_INTERFACE, VENDOR_USAGE_PAGE, VENDOR_USAGE) => InterfaceRole::VendorCommands,
            (VENDOR_INTERFACE, 0, _) => InterfaceRole::VendorCommandsProbable,
            _ => InterfaceRole::Input,
        }
    }

    pub fn is_command_channel(&self) -> bool {
        matches!(
            self.role,
            InterfaceRole::VendorCommands | InterfaceRole::VendorCommandsProbable
        )
    }
}

/// Every HID interface with Mountain's vendor ID.
pub fn list_mountain_interfaces(api: &hidapi::HidApi) -> Vec<HidInterface> {
    let mut list: Vec<HidInterface> = api
        .device_list()
        .filter(|d| d.vendor_id() == VENDOR_ID)
        .map(|d| HidInterface {
            path: d.path().to_string_lossy().into_owned(),
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            interface_number: d.interface_number(),
            usage_page: d.usage_page(),
            usage: d.usage(),
            manufacturer: d.manufacturer_string().map(str::to_owned),
            product: d.product_string().map(str::to_owned),
            role: HidInterface::classify_role(
                d.product_id(),
                d.interface_number(),
                d.usage_page(),
                d.usage(),
            ),
        })
        .collect();
    list.sort_by(|a, b| {
        (
            a.product_id,
            a.interface_number,
            a.usage_page,
            a.usage,
            &a.path,
        )
            .cmp(&(
                b.product_id,
                b.interface_number,
                b.usage_page,
                b.usage,
                &b.path,
            ))
    });
    list
}

/// The command channels among `interfaces`, best match first.
///
/// An exact match (interface 3, usage page 0xFF00, usage 1) is preferred over
/// an interface 3 without usage information. Matching is never done by
/// enumeration order: Windows lists each collection as its own device.
pub fn command_channels(interfaces: &[HidInterface]) -> Vec<&HidInterface> {
    let mut found: Vec<&HidInterface> = interfaces
        .iter()
        .filter(|i| i.is_command_channel())
        .collect();
    found.sort_by_key(|i| i.role != InterfaceRole::VendorCommands);
    found
}

/// Picks the command channel to open: the one at `path` if given, otherwise
/// the only keyboard present.
pub fn select_command_channel<'a>(
    interfaces: &'a [HidInterface],
    path: Option<&str>,
) -> Result<&'a HidInterface> {
    let channels = command_channels(interfaces);
    match path {
        Some(p) => channels
            .into_iter()
            .find(|i| i.path == p)
            .ok_or(Error::NotFound),
        None => {
            // With usage information missing, a single keyboard can show up
            // once per probable channel; prefer the exact match.
            channels.first().copied().ok_or(Error::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(pid: u16, n: i32, page: u16, usage: u16, path: &str) -> HidInterface {
        HidInterface {
            path: path.into(),
            vendor_id: VENDOR_ID,
            product_id: pid,
            interface_number: n,
            usage_page: page,
            usage,
            manufacturer: None,
            product: None,
            role: HidInterface::classify_role(pid, n, page, usage),
        }
    }

    /// Roughly what Windows reports: one entry per top-level collection.
    fn windows_like() -> Vec<HidInterface> {
        vec![
            iface(1, 0, 0x01, 0x06, "mi00"),
            iface(1, 1, 0x0C, 0x01, "mi01-col01"),
            iface(1, 1, 0x01, 0x80, "mi01-col02"),
            iface(1, 1, 0x01, 0x02, "mi01-col03"),
            iface(1, 2, 0x01, 0x06, "mi02"),
            iface(1, 3, 0xFF00, 0x01, "mi03"),
            iface(9, 3, 0xFF00, 0x01, "displaypad"),
        ]
    }

    #[test]
    fn picks_vendor_collection_only() {
        let list = windows_like();
        let chosen = select_command_channel(&list, None).unwrap();
        assert_eq!(chosen.path, "mi03");
        assert_eq!(command_channels(&list).len(), 1);
    }

    #[test]
    fn other_products_are_not_the_keyboard() {
        let list = windows_like();
        let pad = list.iter().find(|i| i.path == "displaypad").unwrap();
        assert_eq!(pad.role, InterfaceRole::OtherProduct("DisplayPad"));
        assert!(!pad.is_command_channel());
    }

    #[test]
    fn falls_back_to_interface_3_without_usage() {
        let list = vec![
            iface(1, 0, 0, 0, "/dev/hidraw0"),
            iface(1, 3, 0, 0, "/dev/hidraw3"),
        ];
        assert_eq!(
            select_command_channel(&list, None).unwrap().path,
            "/dev/hidraw3"
        );
    }

    #[test]
    fn exact_match_wins_over_probable() {
        let list = vec![iface(1, 3, 0, 0, "a"), iface(1, 3, 0xFF00, 1, "b")];
        assert_eq!(select_command_channel(&list, None).unwrap().path, "b");
    }

    #[test]
    fn explicit_path_must_be_a_command_channel() {
        let list = windows_like();
        assert!(select_command_channel(&list, Some("mi00")).is_err());
        assert_eq!(
            select_command_channel(&list, Some("mi03")).unwrap().path,
            "mi03"
        );
    }

    #[test]
    fn nothing_found() {
        assert!(matches!(
            select_command_channel(&[], None),
            Err(Error::NotFound)
        ));
    }
}
