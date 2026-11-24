use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct RoundBlock {
    pub phase: Option<String>,
    pub win_team: Option<String>,
    pub bomb: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct PlayerStateBlock {
    pub health: Option<i64>,
    pub armor: Option<i64>,
    pub money: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct MatchStatsBlock {
    pub kills: Option<i64>,
    pub assists: Option<i64>,
    pub deaths: Option<i64>,
    pub mvps: Option<i64>,
    pub score: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PlayerBlock {
    pub steamid: Option<String>,
    pub name: Option<String>,
    pub state: Option<PlayerStateBlock>,
    pub match_stats: Option<MatchStatsBlock>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct TeamScoreBlock {
    pub score: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct MapBlock {
    pub name: Option<String>,
    pub mode: Option<String>,
    pub round: Option<i64>,
    pub team_ct: Option<TeamScoreBlock>,
    pub team_t: Option<TeamScoreBlock>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct ProviderBlock {
    pub steamid: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GsiSnapshot {
    pub provider: Option<ProviderBlock>,
    pub map: Option<MapBlock>,
    pub round: Option<RoundBlock>,
    pub player: Option<PlayerBlock>,
    #[serde(default)]
    pub allplayers: HashMap<String, PlayerBlock>,
}

/// Extracts only the fields we track from a raw GSI payload. Unknown/unsubscribed
/// fields (phase_countdowns, weapons, ...) are silently ignored by serde's default
/// behavior -- this struct is intentionally not the full schema. `provider.steamid`
/// is the local game client's own steamid, always accurate regardless of who
/// `player` currently reflects (see events.rs -- CS2's single `player` slot can
/// briefly show a teammate's data during deathcam/spectate).
pub fn parse_snapshot(value: &serde_json::Value) -> GsiSnapshot {
    serde_json::from_value(value.clone()).unwrap_or_default()
}
