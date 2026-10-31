//! Detecting other programs that talk to the keyboard.
//!
//! The keyboard broadcasts replies and has a single command buffer, so two
//! programs driving it at once break each other. Base Camp in particular
//! must be closed before Kebnekaise sends anything.

use serde::Serialize;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// Another program that is, or may be, using the keyboard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub pid: u32,
    pub name: String,
    pub reason: ConflictReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    /// A Mountain Base Camp process or service.
    BaseCamp,
    /// Another known keyboard tool (e.g. BaseCamp-Linux).
    KnownTool,
    /// A process with the command interface's device node open (Linux).
    HoldsDevice,
}

/// Process names (lower case, without `.exe`) that belong to Base Camp.
const BASE_CAMP_PROCESSES: &[&str] = &[
    "base camp",
    "basecamp.service",
    "basecampservice",
    "mountaindisplaypadworker",
];

/// Process names of other community tools that drive the keyboard.
const KNOWN_TOOLS: &[&str] = &[
    "basecamp-linux",
    "basecamp-controller",
    "basecamp-tray",
    "emax_controller",
    "k2",
];

/// Classifies a process name, or `None` if it is unrelated.
pub(crate) fn classify_process(name: &str) -> Option<ConflictReason> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    if BASE_CAMP_PROCESSES.contains(&stem) {
        Some(ConflictReason::BaseCamp)
    } else if KNOWN_TOOLS.contains(&stem) {
        Some(ConflictReason::KnownTool)
    } else {
        None
    }
}

/// Programs that may be using the keyboard, excluding this process.
///
/// `device_path` is the command interface we are about to open; on Linux any
/// other process holding it open is reported too. Processes owned by other
/// users may be invisible without elevated rights.
pub fn find_conflicts(device_path: Option<&str>) -> Vec<Conflict> {
    #[cfg(target_os = "linux")]
    {
        linux::find_conflicts(device_path)
    }
    #[cfg(windows)]
    {
        let _ = device_path;
        windows::find_conflicts()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = device_path;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_names() {
        assert_eq!(
            classify_process("Base Camp.exe"),
            Some(ConflictReason::BaseCamp)
        );
        assert_eq!(
            classify_process("BaseCamp.Service.exe"),
            Some(ConflictReason::BaseCamp)
        );
        assert_eq!(
            classify_process("MountainDisplayPadWorker.exe"),
            Some(ConflictReason::BaseCamp)
        );
        assert_eq!(classify_process("K2.exe"), Some(ConflictReason::KnownTool));
        assert_eq!(classify_process("kebnekaise"), None);
        assert_eq!(classify_process("basecampfire.exe"), None);
        assert_eq!(classify_process("explorer.exe"), None);
    }
}
