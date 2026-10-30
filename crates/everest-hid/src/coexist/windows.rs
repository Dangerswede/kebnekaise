use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::{classify_process, Conflict};

pub(super) fn find_conflicts() -> Vec<Conflict> {
    let own = std::process::id();
    // Detection is best-effort: if the process list is unavailable we report
    // nothing rather than block the user.
    let Ok(mut system) = System::new() else {
        return Vec::new();
    };
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());

    let mut found: Vec<Conflict> = system
        .processes()
        .iter()
        .filter(|(pid, _)| pid.as_u32() != own)
        .filter_map(|(pid, process)| {
            let name = process.name().to_string_lossy().into_owned();
            classify_process(&name).map(|reason| Conflict {
                pid: pid.as_u32(),
                name,
                reason,
            })
        })
        .collect();
    found.sort_by_key(|c| c.pid);
    found
}
