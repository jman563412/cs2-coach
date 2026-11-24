mod events;
mod gsi_types;
mod process_watch;
mod storage;

use chrono::Utc;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use sysinfo::System;
use tiny_http::{Response, Server};

const PORT: u16 = 3909;
const PROCESS_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Where session files go: `GSI_OUTPUT_DIR` if set and non-empty, else `gsi-sessions` next to the executable.
fn output_dir() -> PathBuf {
    match std::env::var_os("GSI_OUTPUT_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => std::env::current_exe()
            .expect("failed to locate the listener executable")
            .parent()
            .expect("executable has no parent directory")
            .join("gsi-sessions"),
    }
}

fn main() {
    let output_dir = output_dir();
    let output_dir = output_dir.as_path();
    std::fs::create_dir_all(output_dir).expect("failed to create output directory");

    if let Err(e) = storage::prune_oldest(output_dir, "_raw.jsonl", storage::RAW_CAP_BYTES) {
        eprintln!("warning: raw.jsonl pruning failed: {e}");
    }
    if let Err(e) = storage::prune_oldest(output_dir, "_events.jsonl", storage::EVENTS_CAP_BYTES) {
        eprintln!("warning: events.jsonl pruning failed: {e}");
    }

    let session_ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let raw_path = output_dir.join(format!("session_{session_ts}_raw.jsonl"));
    let events_path = output_dir.join(format!("session_{session_ts}_events.jsonl"));

    let mut raw_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&raw_path)
        .expect("failed to open raw session file");
    let mut events_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)
        .expect("failed to open events session file");

    let server = match Server::http(format!("127.0.0.1:{PORT}")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to bind 127.0.0.1:{PORT}: {e}");
            std::process::exit(1);
        }
    };
    println!("gsi-listener listening on 127.0.0.1:{PORT}, writing to {}", output_dir.display());

    let mut sys = System::new_all();
    let mut tracker = events::StateTracker::default();
    let mut last_process_check = Instant::now();
    // Latches true the first time cs2.exe is observed running. Only meaningful
    // once true does "cs2.exe not found" mean "the game exited" -- before that,
    // it just means the game hasn't finished starting yet (the Steam launch
    // wrapper starts this listener before CS2 itself, and CS2 cold-start can
    // easily take longer than one PROCESS_CHECK_INTERVAL).
    let mut seen_cs2_running = false;

    loop {
        match server.recv_timeout(Duration::from_secs(1)) {
            Ok(Some(mut request)) => {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_ok() {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
                        let snapshot = gsi_types::parse_snapshot(&value);
                        // Community/workshop servers ("custom") and deathmatch have no
                        // round structure, unlike real matchmaking (competitive/casual)
                        // -- GSI streams constant player_stat_update spam instead of
                        // sparse round-boundary updates (a 30min custom-server session
                        // recorded here once outsized a full 21-round competitive
                        // match), and neither is useful for coaching on ranked/casual
                        // play. Skip persisting entirely while in one of these modes;
                        // menu/pre-connect updates (mode absent) still get written same
                        // as before.
                        let skip_mode = matches!(
                            snapshot.map.as_ref().and_then(|m| m.mode.as_deref()),
                            Some("custom") | Some("deathmatch")
                        );

                        if !skip_mode {
                            if let Err(e) = storage::append_raw(&mut raw_file, &body) {
                                eprintln!("failed to write raw.jsonl: {e}");
                                break;
                            }

                            let derived = events::derive_events(&snapshot, &mut tracker);
                            if !derived.is_empty() {
                                if let Err(e) = storage::append_events(&mut events_file, &derived) {
                                    eprintln!("failed to write events.jsonl: {e}");
                                    break;
                                }
                            }
                        }
                    }
                }
                let _ = request.respond(Response::empty(200));
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("server error: {e}");
                break;
            }
        }

        if last_process_check.elapsed() >= PROCESS_CHECK_INTERVAL {
            if process_watch::is_cs2_running(&mut sys) {
                seen_cs2_running = true;
            } else if seen_cs2_running {
                println!("cs2.exe no longer running, exiting");
                break;
            }
            last_process_check = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test fn: both cases mutate the process-wide GSI_OUTPUT_DIR, and parallel tests would race on it.
    #[test]
    fn output_dir_prefers_env_var_then_defaults_next_to_exe() {
        let custom = std::env::temp_dir().join("gsi-output-dir-test");
        std::env::set_var("GSI_OUTPUT_DIR", &custom);
        assert_eq!(output_dir(), custom);

        std::env::set_var("GSI_OUTPUT_DIR", "");
        let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(output_dir(), exe_dir.join("gsi-sessions"));

        std::env::remove_var("GSI_OUTPUT_DIR");
        assert_eq!(output_dir(), exe_dir.join("gsi-sessions"));
    }
}
