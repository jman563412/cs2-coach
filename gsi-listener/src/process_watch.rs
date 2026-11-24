use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// Refreshes `sys` and checks whether a process named "cs2.exe" is currently
/// running. Case-insensitive match, since Windows process names can vary in case.
/// Only refreshes the process list itself (name/pid), not CPU/memory/disk/exe/
/// tasks -- this runs every 5s and doesn't need anything beyond process names.
pub fn is_cs2_running(sys: &mut System) -> bool {
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes()
        .values()
        .any(|p| p.name().to_string_lossy().eq_ignore_ascii_case("cs2.exe"))
}
