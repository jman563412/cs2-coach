use chrono::Utc;
use serde_json::Value;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

pub const RAW_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB
pub const EVENTS_CAP_BYTES: u64 = 200 * 1024 * 1024; // 200 MB

/// Parses `body` as JSON, adds a `_received_at` timestamp, and appends it as one
/// line to `file`. Writes the object as received -- no field filtering -- since
/// this file exists specifically to preserve full fidelity for later drill-down.
pub fn append_raw(file: &mut File, body: &str) -> io::Result<()> {
    let mut value: Value =
        serde_json::from_str(body).unwrap_or_else(|_| serde_json::json!({ "_unparsed": body }));
    if let Value::Object(ref mut map) = value {
        map.insert(
            "_received_at".to_string(),
            Value::String(Utc::now().to_rfc3339()),
        );
    }
    writeln!(file, "{}", value)?;
    file.flush()
}

pub fn append_events(file: &mut File, events: &[Value]) -> io::Result<()> {
    for event in events {
        writeln!(file, "{}", event)?;
    }
    file.flush()
}

/// Deletes the oldest files (by filename, which sorts chronologically since
/// session files are named by start timestamp) whose name ends with `suffix`,
/// in `dir`, until their combined size is at or under `cap_bytes`.
pub fn prune_oldest(dir: &Path, suffix: &str, cap_bytes: u64) -> io::Result<()> {
    let mut entries: Vec<(std::path::PathBuf, u64)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(suffix))
        .filter_map(|e| {
            let size = e.metadata().ok()?.len();
            Some((e.path(), size))
        })
        .collect();

    entries.sort_by(|a, b| a.0.cmp(&b.0)); // filenames are timestamp-prefixed, so this is oldest-first

    let mut total: u64 = entries.iter().map(|(_, size)| size).sum();
    let mut remaining = entries.len();
    for (path, size) in entries {
        // Always keep at least one file, even if it alone exceeds the cap -- this
        // only ever means an old session's file is oversized, never the one
        // currently being written (pruning runs once at startup, before the new
        // session file exists).
        if total <= cap_bytes || remaining <= 1 {
            break;
        }
        std::fs::remove_file(&path)?;
        total -= size;
        remaining -= 1;
    }

    Ok(())
}
