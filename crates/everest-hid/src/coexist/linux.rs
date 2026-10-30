use std::fs;
use std::path::Path;

use super::{classify_process, Conflict, ConflictReason};

pub(super) fn find_conflicts(device_path: Option<&str>) -> Vec<Conflict> {
    let own = std::process::id();
    let device = device_path.and_then(|p| fs::canonicalize(p).ok());
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == own {
            continue;
        }
        let name = process_name(&entry.path());
        if let Some(reason) = classify_process(&name) {
            found.push(Conflict { pid, name, reason });
        } else if let Some(dev) = &device {
            if holds_file(&entry.path(), dev) {
                found.push(Conflict {
                    pid,
                    name,
                    reason: ConflictReason::HoldsDevice,
                });
            }
        }
    }
    found.sort_by_key(|c| c.pid);
    found
}

/// The executable name: `comm` is cut at 15 characters, so prefer the
/// file name of `argv[0]` (Python tools show up as `python3` in `comm`).
fn process_name(proc_dir: &Path) -> String {
    let cmdline = fs::read(proc_dir.join("cmdline")).unwrap_or_default();
    let args: Vec<&[u8]> = cmdline
        .split(|&b| b == 0)
        .filter(|a| !a.is_empty())
        .collect();
    let base = |arg: &[u8]| {
        let s = String::from_utf8_lossy(arg).into_owned();
        Path::new(&s)
            .file_stem()
            .map_or(s.clone(), |f| f.to_string_lossy().into_owned())
    };
    if let Some(first) = args.first() {
        let exe = base(first);
        // `python3 emax_controller.py`: the script is the interesting part.
        if exe.starts_with("python") {
            if let Some(script) = args.get(1) {
                return base(script);
            }
        }
        return exe;
    }
    fs::read_to_string(proc_dir.join("comm"))
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn holds_file(proc_dir: &Path, target: &Path) -> bool {
    let Ok(fds) = fs::read_dir(proc_dir.join("fd")) else {
        return false;
    };
    fds.flatten()
        .any(|fd| fs::read_link(fd.path()).is_ok_and(|link| link == target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanning_proc_does_not_panic_and_skips_self() {
        let conflicts = find_conflicts(Some("/dev/null"));
        assert!(conflicts.iter().all(|c| c.pid != std::process::id()));
    }
}
