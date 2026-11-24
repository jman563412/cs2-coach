# CS2 GSI Listener — Design Spec

## Purpose

A companion to the existing cs2-coach tool that captures near-real-time match state *while a CS2 match is being played*, using Valve's Game State Integration (GSI) protocol, so that coaching questions can draw on round-by-round and economy/health trajectory data that post-match sources (Leetify, csstats.gg) don't preserve at that resolution.

This is purely a data-capture component. There is no live overlay, no in-match alerts, and — consistent with the rest of this project — no fixed analysis baked in. Captured data is read ad hoc by Claude, the same way `flatten_matches()` output and csstats history already are.

## Non-goals

- **No kill attribution / kill-feed reconstruction.** Confirmed by inspecting the actual GSI schema (Valve's own wiki is blocked by an anti-scraper challenge; verified instead against a maintained third-party TypeScript type-definitions mirror): GSI exposes only cumulative per-player counters (`match_stats.kills/assists/deaths/mvps/score`), not a discrete per-kill record with attacker/victim/weapon. Reconstructing "who killed whom" would require inferring it from counters incrementing across players within the same update — and CS2's `throttle` setting means multiple deaths in a trade or multi-kill commonly land in the *same* GSI update with no ordering information between them. This makes inference structurally unreliable precisely in the situations (trades, multi-kills) most worth coaching on. Rejected as not worth building; a Leetify demo review is the correct source for exact kill detail.
- **No spatial or aim data.** GSI does not expose player position or view angle at all. That level of detail only ever exists via demo file parsing, which is what Leetify's aim/positioning/preaim ratings already come from. This listener cannot and should not try to compete with that.
- **No in-game overlay/HUD, no live in-match coaching alerts.**
- **No automated trend-detection module.** Trend analysis stays conversational and ad hoc, per question — consistent with the rest of this project's "no fixed report format" design.
- **No historical backfill.** GSI only ever sees data from whenever it's installed forward, same shape as the deferred Leetify-cache Future Work item in the main coach spec.
- **No installer automation.** Setup (placing the GSI cfg file, configuring Steam Launch Options) is a manual one-time step, consistent with how the csstats.gg CSV export is also a manual one-time step.

## Environment

CS2 runs natively on Windows, on the same physical machine as this WSL2 dev environment. This shapes several decisions below: the listener must be a native Windows binary (Windows and WSL2 have separate process namespaces — a WSL-side process cannot see `cs2.exe` in its own process list), and data handoff crosses the WSL/Windows filesystem boundary in the low-friction direction (WSL reading a Windows path via `/mnt/d/...`, not the reverse).

## Architecture

Two fully independent processes, connected only by files on disk — no sockets between them, no shared process, no language bindings:

- **Existing Python coach** (`leetify_client.py`, `csstats_data.py`) — unchanged.
- **New Rust listener** — a native Windows `.exe`, cross-compiled from this WSL repo (`x86_64-pc-windows-gnu` target), launched automatically alongside CS2 via Steam Launch Options, and self-terminating when `cs2.exe` exits.

Rejected alternative: embedding a Python interpreter in a Rust host process so one binary could serve as both the listener and the interactive coach entry point (via PyO3). Ruled out because the two components have fundamentally different lifecycles — the listener is a long-running daemon that should start once per gaming session and never touch anything unrelated; the coach is bursty, on-demand, invoked per question through a Claude conversation. Coupling them into one process would mean a hiccup in ad hoc Q&A (a bad snippet, a stalled network call) could jeopardize the one process whose entire job is not dropping live match data. The listener also never needs Python at all — its job (parse JSON, write to a file) is plain Rust with zero interop required.

## Components

### GSI config

`gamestate_integration_cs2coach.cfg`, placed manually once in CS2's `game/csgo/cfg/` folder on Windows. Tells the game to POST to `http://127.0.0.1:3909` (arbitrary local port, unlikely to collide with anything else running) on a short interval, subscribed to:

```
provider, map, round, player_id, player_state, player_weapons, player_match_stats,
allplayers_id, allplayers_state, allplayers_match_stats, phase_countdowns, bomb
```

Subscription is broad on purpose (capture what's available, decide what's useful per question later) — but only the fields confirmed exact (see Non-goals) are used to derive `events.jsonl`.

**Revised, post-implementation:** the original subscription list omitted `player_match_stats`/`allplayers_match_stats` — confirmed against a real competitive match's `raw.jsonl` (772 updates), where `match_stats` never appeared once, so kills/assists/deaths/mvps/score were silently unavailable to the diffing layer the whole time. Added above. Separately, `allplayers` itself (state *and* match_stats) was confirmed never populated at all during live competitive play across that same match — Valve appears to withhold it outside spectator contexts, most plausibly to prevent a local GSI listener from leaking real-time enemy state. `player` (the local user's own POV) comes through reliably; `events.rs`'s diffing was updated to also track `snapshot.player` (not just `snapshot.allplayers`) so the user's own economy/health/kill trajectory is captured even when `allplayers` is empty, which — outside of scrimmage/spectator setups — it always will be in practice.

**Second follow-up:** `player` isn't strictly *always* the local user's own POV — CS2 briefly points it at whoever's being spectated during deathcam (confirmed: ~24% of one real match's updates showed a teammate's steamid there instead). `events.rs` cross-checks `snapshot.player.steamid` against `snapshot.provider.steamid` (the local game client's own id, unaffected by spectate) to tell the two cases apart.

**Third follow-up (2026-08-05) — corrects the paragraph above.** That revision dropped the teammate frames entirely, on the stated grounds that "Leetify's post-match API already provides complete, accurate stats for all 10 players with no GSI involvement needed." **That claim is false.** `/v3/profile/matches` returns exactly ONE stat block — the requested player's own. Verified against the raw HTTP body: across 100 matches the response contains 100 `steam64_id` keys and exactly one distinct SteamID64 (the user's), and every `stats` array has length 1. There is also no match-detail or roster endpoint (`/v3/matches/{id}`, `/v3/match/{id}`, and three variants all 404). So nothing downstream could recover who else was in a match, and the listener was discarding the only roster signal the setup had.

Deathcam frames are now recorded as a `teammate_observed` event (`steamid`, `name`, `map`, `round`), emitted once per teammate per map — `observed_teammates` is cleared on every `map_change`, so a session spanning several matches yields per-match rosters rather than one union. Their *stats* are still deliberately not tracked: a deathcam snapshot is a mid-match fragment and would corrupt the diffing in `last_player_stats`. Identity only.

Recovering this retroactively is possible for sessions already captured, since `raw.jsonl` stores every payload verbatim: scanning the 50 archived sessions surfaced 269 distinct teammate SteamID64s, 8 of them on the user's Steam friends list — where the Leetify join could only ever see 4. **Coverage is a lower bound**: a teammate only appears if the user died and happened to spectate that specific player, so absence is not evidence of absence, and any "played solo" bucket derived from this is contaminated by under-counting. Cross-reference with the Leetify join (`split_by_teammate`) rather than relying on either source alone.

### Rust listener (`gsi-listener/`, new Cargo project in this repo)

Dependencies:
- `tiny_http` — synchronous local HTTP server; no async runtime needed for a single local client posting periodically
- `serde` / `serde_json` — hand-written structs for just the subscribed GSI blocks (no third-party GSI crate — the two on crates.io that appeared relevant during research don't hold up: `gsi-cs2` doesn't actually exist despite search results suggesting otherwise, confirmed via a direct `crates.io` API call returning `{"errors":[{"detail":"crate `gsi-cs2` does not exist"}]}`; `csgo-gsi2` is real but targets CS:GO specifically, is tokio/async-coupled, and only does typed deserialization + HTTP server wiring — no more than we get from hand-written structs + `tiny_http`)
- `sysinfo` — polls the Windows process list every few seconds for `cs2.exe`; the listener exits on its own once it's no longer found

Behavior:
1. On startup, opens two files for the session in the output folder (`GSI_OUTPUT_DIR`, default `gsi-sessions\` next to the exe; e.g. `D:\cs2-coach\gsi-sessions\`), named by start timestamp (e.g. `session_2026-07-09T193045Z_raw.jsonl` / `..._events.jsonl`).
2. Every GSI POST received is parsed and appended verbatim to `raw.jsonl`, plus a `_received_at` timestamp.
3. The listener tracks `round.phase`, `round.win_team`, `round.bomb`, and each player's `match_stats`/`state` (health, armor, money) across updates. When one of these exact fields changes, a compact derived record is appended to `events.jsonl`: round start/end (with score), bomb plant/defuse/phase change, and per-player stat/economy snapshots at round boundaries. No kill attribution, no inference — every field written to `events.jsonl` is a direct, unambiguous GSI value.
4. Every 5 seconds, checks the Windows process list for `cs2.exe`. If absent, flushes both files and exits.

### Coach-side read access

No new Python module. The storage convention (two JSONL files per session, in the output folder, e.g. `D:\cs2-coach\gsi-sessions\`, readable from WSL at `/mnt/d/cs2-coach/gsi-sessions/`) is documented for Claude to read directly with an ad hoc snippet per question — same pattern used for `flatten_matches()` output and csstats history. `events.jsonl` is the default read (small, cheap); `raw.jsonl` is read selectively — using timestamps/round numbers found in `events.jsonl` to locate the relevant slice — only when a question genuinely needs tick-level health/money/weapon detail beyond round-boundary snapshots. A generic loader function is deferred (see Future Work) since GSI's per-update shape isn't a single flat schema the way Leetify matches are, so a hand-rolled helper wouldn't earn its keep yet.

## Retention / Storage Cap

Without a cap, `raw.jsonl` files would accumulate indefinitely. Rough estimate (not yet measured against a real capture): a single GSI update with `allplayers` populated is ~4-6 KB of JSON, and updates only fire on data changes, so a full match plausibly lands around 5-15 MB of `raw.jsonl`. `events.jsonl` is far smaller — only round boundaries, bomb events, and per-round econ snapshots, likely under 200 KB per match.

**Revised, post-implementation:** the estimate above assumed `events.jsonl` would log player snapshots only at round boundaries. The implementation instead logs a `player_stat_update` on every exact-field change (health/armor/money/kills/assists/deaths/mvps/score), for every player, continuously through the match — not just at round transitions. This is a deliberate choice (richer data, still zero inference — see Non-goals) but it means the "under 200 KB per match" figure is stale: with `throttle 0.5s` in the GSI cfg and the amount of health/money churn during firefights and buy rounds, real per-match `events.jsonl` size is likely low-single-digit MB rather than under 200 KB. The 200 MB total cap below is unchanged — it's still a real ceiling, it will just start pruning sooner and more often than the original estimate assumed. See the retuning item in Future Work, which is now more urgent than its original wording implies (this isn't a "someday" cleanup — the size estimate the caps were picked against is already known to be wrong).

Two independent caps, enforced the same way, checked once at every listener startup (i.e. each time a new session begins):
- **`raw.jsonl` total: 2 GB** across all sessions in the output folder
- **`events.jsonl` total: 200 MB** across all sessions

For each file type independently: sum current total size; if over its cap, delete oldest-timestamped files (sorted by the timestamp already encoded in the filename) until back under it. The two caps are independent — `raw.jsonl` growth never evicts `events.jsonl` history, since the latter is cheap and high-value (most questions read it by default; see Coach-side read access above).

**These numbers are estimates and will likely need retuning once real capture data exists** — see Future Work.

## Deployment / Lifecycle

Steam Launch Options for CS2 set to a wrapper batch script: starts the listener binary in the background (non-blocking), then chains to the real game launch (`%command%`). The listener's own process-polling loop (see above) ties its lifetime to `cs2.exe` without any separate always-on watcher process — nothing runs when CS2 isn't open.

## Error Handling

Matches the rest of this project's minimal style: no retries, no backoff. If the listener can't bind its port or can't write to a session file, it logs to stderr and exits rather than looping or attempting recovery.

## Testing

No automated test suite, consistent with the main coach spec. Verification is a one-time smoke check during implementation:
1. POST a synthetic GSI payload via `curl` to the running listener and confirm both `raw.jsonl` and `events.jsonl` are written correctly.
2. A real session alongside CS2: confirm the listener starts via the Steam launch options wrapper, captures real round/bomb/economy events during play, and self-terminates within a few seconds of the game closing.

## Future Work (explicitly deferred, not part of this implementation)

- **Retune the `raw.jsonl` (2 GB) / `events.jsonl` (200 MB) storage caps** once a few real sessions' worth of capture data exist and actual per-match file sizes are known, rather than the rough estimates used to pick these defaults. This is more urgent than it might sound: the `events.jsonl` estimate below the caps were originally picked against is already known to be stale (see the "Revised, post-implementation" note under Retention / Storage Cap above) — this isn't a hypothetical future drift, it's a known-wrong number sitting behind a real cap today.
- **Richer `events.jsonl` content**, if round-boundary-only granularity proves insufficient in practice — e.g. logging exact-field health-drop events mid-round (still no kill attribution, just "this player's health changed from X to Y at this timestamp").
- **A dedicated `gsi_data.py` read/parse helper**, if ad hoc raw-JSONL reads in Claude's Bash snippets prove unwieldy once real session data exists.
- **Correlating a GSI session file with its corresponding Leetify `match_id`** after the match finishes and syncs, so live-captured data and post-match Leetify stats can be cross-referenced for the same match.
