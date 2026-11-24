use crate::gsi_types::{GsiSnapshot, PlayerBlock};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Last-seen state, kept in memory for the life of one listener session, used to
/// detect changes in only the exact (non-inferred) fields we track.
#[derive(Default)]
pub struct StateTracker {
    last_map_name: Option<String>,
    last_round_phase: Option<String>,
    last_win_team: Option<String>,
    last_bomb: Option<String>,
    last_player_stats: HashMap<String, PlayerBlock>,
    /// Teammate steamids already reported since the current map started. Cleared on
    /// every map change so each match gets its own roster rather than one
    /// session-wide union -- a session routinely spans several matches with
    /// different teammates.
    observed_teammates: HashSet<String>,
}

fn player_changed(prev: Option<&PlayerBlock>, current: &PlayerBlock) -> bool {
    match prev {
        None => true,
        Some(p) => {
            p.state.as_ref().and_then(|s| s.health) != current.state.as_ref().and_then(|s| s.health)
                || p.state.as_ref().and_then(|s| s.armor) != current.state.as_ref().and_then(|s| s.armor)
                || p.state.as_ref().and_then(|s| s.money) != current.state.as_ref().and_then(|s| s.money)
                || p.match_stats.as_ref().map(|m| m.kills) != current.match_stats.as_ref().map(|m| m.kills)
                || p.match_stats.as_ref().map(|m| m.assists) != current.match_stats.as_ref().map(|m| m.assists)
                || p.match_stats.as_ref().map(|m| m.deaths) != current.match_stats.as_ref().map(|m| m.deaths)
                || p.match_stats.as_ref().map(|m| m.mvps) != current.match_stats.as_ref().map(|m| m.mvps)
                || p.match_stats.as_ref().map(|m| m.score) != current.match_stats.as_ref().map(|m| m.score)
        }
    }
}

fn player_event(now: &str, steamid: &str, player: &PlayerBlock) -> Value {
    json!({
        "type": "player_stat_update",
        "at": now,
        "steamid": steamid,
        "name": player.name,
        "health": player.state.as_ref().and_then(|s| s.health),
        "armor": player.state.as_ref().and_then(|s| s.armor),
        "money": player.state.as_ref().and_then(|s| s.money),
        "kills": player.match_stats.as_ref().and_then(|m| m.kills),
        "assists": player.match_stats.as_ref().and_then(|m| m.assists),
        "deaths": player.match_stats.as_ref().and_then(|m| m.deaths),
        "mvps": player.match_stats.as_ref().and_then(|m| m.mvps),
        "score": player.match_stats.as_ref().and_then(|m| m.score),
    })
}

/// Pulls the round number and each team's score off `snapshot`'s `map` block
/// (`None`-safe, since `map` and its sub-fields are all optional in GSI). These
/// are cumulative team-score counters straight off the wire, not per-kill data.
fn round_and_score(snapshot: &GsiSnapshot) -> (Option<i64>, Option<i64>, Option<i64>) {
    let round = snapshot.map.as_ref().and_then(|m| m.round);
    let ct_score = snapshot
        .map
        .as_ref()
        .and_then(|m| m.team_ct.as_ref())
        .and_then(|t| t.score);
    let t_score = snapshot
        .map
        .as_ref()
        .and_then(|m| m.team_t.as_ref())
        .and_then(|t| t.score);
    (round, ct_score, t_score)
}

/// Compares `snapshot` against `tracker`'s last-seen state and returns zero or
/// more compact event records for whatever exact fields changed. Never attempts
/// to infer kill attribution -- see the design spec's Non-goals.
pub fn derive_events(snapshot: &GsiSnapshot, tracker: &mut StateTracker) -> Vec<Value> {
    let mut out = Vec::new();
    let now = Utc::now().to_rfc3339();
    let (round, ct_score, t_score) = round_and_score(snapshot);

    let map_name = snapshot.map.as_ref().and_then(|m| m.name.clone());
    if map_name.is_some() && map_name != tracker.last_map_name {
        out.push(json!({
            "type": "map_change",
            "at": now,
            "map": map_name,
        }));
        tracker.last_map_name = map_name;
        tracker.observed_teammates.clear();
    }

    if let Some(round_block) = &snapshot.round {
        if round_block.phase != tracker.last_round_phase {
            out.push(json!({
                "type": "round_phase_change",
                "at": now,
                "phase": round_block.phase,
                "round": round,
                "ct_score": ct_score,
                "t_score": t_score,
            }));
            tracker.last_round_phase = round_block.phase.clone();
        }
        if round_block.win_team.is_some() && round_block.win_team != tracker.last_win_team {
            out.push(json!({
                "type": "round_win",
                "at": now,
                "win_team": round_block.win_team,
                "round": round,
                "ct_score": ct_score,
                "t_score": t_score,
            }));
            tracker.last_win_team = round_block.win_team.clone();
        }
        if round_block.bomb != tracker.last_bomb {
            if let Some(bomb_state) = &round_block.bomb {
                out.push(json!({
                    "type": "bomb_state_change",
                    "at": now,
                    "bomb": bomb_state,
                }));
            }
            tracker.last_bomb = round_block.bomb.clone();
        }
    }

    // GSI's `allplayers` block is only ever populated in spectator contexts --
    // confirmed empirically against a real competitive match, where it never
    // appeared once across 772 updates (Valve withholds it during live play,
    // most likely to prevent a local listener leaking real-time enemy
    // state). `player` (the local user's own POV) is sent regardless -- but
    // it isn't always actually the local user: CS2 briefly points `player` at
    // a teammate's data during deathcam/spectate (confirmed: ~24% of updates
    // in one real match). `provider.steamid` is always the true local game
    // client's own id regardless of who's being observed, so it's used here to
    // separate the two cases. Tracked through the same steamid-keyed map as
    // `allplayers`: if a steamid ever appears in both `player` and `allplayers`
    // in the same update, processing `player` first means the `allplayers` pass
    // below sees no further change and no-ops, rather than emitting a duplicate.
    //
    // When `player` is NOT the local user it is a teammate being spectated, and
    // that identity is recorded as a `teammate_observed` event. This is the only
    // roster signal available: Leetify's `/v3/profile/matches` returns exactly
    // ONE stat block (the requested player's own) and there is no match-detail or
    // roster endpoint, so nothing downstream can recover who else was in a match.
    // An earlier revision of this file dropped these frames outright, on the
    // stated grounds that Leetify "already gives complete, accurate stats for all
    // 10 players" -- that is false, and it silently discarded the only teammate
    // data the setup ever had. Their *stats* are still not tracked: a deathcam
    // snapshot is a fragment mid-match and would corrupt the diffing in
    // `last_player_stats`. Only identity is kept, once per teammate per map.
    if let Some(player) = &snapshot.player {
        let is_self = snapshot
            .provider
            .as_ref()
            .and_then(|p| p.steamid.as_deref())
            .is_some_and(|provider_id| player.steamid.as_deref() == Some(provider_id));
        if let Some(steamid) = &player.steamid {
            if is_self {
                let prev = tracker.last_player_stats.get(steamid);
                if player_changed(prev, player) {
                    out.push(player_event(&now, steamid, player));
                    tracker.last_player_stats.insert(steamid.clone(), player.clone());
                }
            } else if tracker.observed_teammates.insert(steamid.clone()) {
                out.push(json!({
                    "type": "teammate_observed",
                    "at": now,
                    "steamid": steamid,
                    "name": player.name,
                    "map": tracker.last_map_name,
                    "round": round,
                }));
            }
        }
    }

    for (steamid, player) in &snapshot.allplayers {
        let prev = tracker.last_player_stats.get(steamid);
        if player_changed(prev, player) {
            out.push(player_event(&now, steamid, player));
            tracker.last_player_stats.insert(steamid.clone(), player.clone());
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gsi_types::parse_snapshot;
    use serde_json::json;

    const ME: &str = "76561198000000001";
    const MATE: &str = "76561198000000002";

    fn snapshot(map: &str, player_steamid: &str) -> GsiSnapshot {
        parse_snapshot(&json!({
            "provider": { "steamid": ME },
            "map": { "name": map, "round": 5 },
            "player": {
                "steamid": player_steamid,
                "name": "someone",
                "state": { "health": 100, "armor": 0, "money": 800 },
            },
        }))
    }

    fn types(events: &[Value]) -> Vec<String> {
        events
            .iter()
            .map(|e| e["type"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn teammate_seen_during_deathcam_is_recorded() {
        let mut tracker = StateTracker::default();
        let events = derive_events(&snapshot("de_mirage", MATE), &mut tracker);

        let observed: Vec<&Value> = events
            .iter()
            .filter(|e| e["type"] == "teammate_observed")
            .collect();
        assert_eq!(observed.len(), 1, "expected one teammate_observed: {events:?}");
        assert_eq!(observed[0]["steamid"], MATE);
        assert_eq!(observed[0]["map"], "de_mirage");
        assert_eq!(observed[0]["round"], 5);
    }

    #[test]
    fn teammate_is_reported_once_per_map_then_again_after_a_map_change() {
        let mut tracker = StateTracker::default();

        derive_events(&snapshot("de_mirage", MATE), &mut tracker);
        let repeat = derive_events(&snapshot("de_mirage", MATE), &mut tracker);
        assert!(
            !types(&repeat).contains(&"teammate_observed".to_string()),
            "same teammate on the same map must not re-emit: {repeat:?}"
        );

        // A new match starts: the roster must not carry over from the previous map.
        let next_map = derive_events(&snapshot("de_dust2", MATE), &mut tracker);
        assert!(
            types(&next_map).contains(&"teammate_observed".to_string()),
            "map change should clear the roster and re-report: {next_map:?}"
        );
    }

    #[test]
    fn teammate_stats_are_not_tracked_as_self_stats() {
        let mut tracker = StateTracker::default();
        let events = derive_events(&snapshot("de_mirage", MATE), &mut tracker);

        // A deathcam frame is a mid-match fragment; letting it into last_player_stats
        // would corrupt the diffing that produces player_stat_update.
        assert!(
            !types(&events).contains(&"player_stat_update".to_string()),
            "teammate frames must not emit stat updates: {events:?}"
        );
        assert!(tracker.last_player_stats.is_empty());
    }

    #[test]
    fn local_player_still_emits_stat_updates() {
        let mut tracker = StateTracker::default();
        let events = derive_events(&snapshot("de_mirage", ME), &mut tracker);

        assert!(types(&events).contains(&"player_stat_update".to_string()));
        assert!(!types(&events).contains(&"teammate_observed".to_string()));
        assert!(tracker.last_player_stats.contains_key(ME));
    }
}
