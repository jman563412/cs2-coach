# CS2 Coach — Design Spec

## Purpose

An on-demand tool for pulling the user's own CS2 match data from the Leetify Public API, so that Claude can act as a coach — answering ad hoc questions about performance trends (core Leetify rating dimensions, win rate/rank, map-specific performance, raw combat stats) by analyzing that data live, in conversation.

There is no scheduled job, no dashboard, and no fixed report format. The user asks a question in a session; Claude fetches fresh data and answers it.

## Non-goals

- ~~No teammate/duo-specific stats (the feature that originally motivated this — "how do I do playing with player X" — is already covered by Leetify's own UI and is out of scope here).~~ **Reversed 2026-07-16:** the user asked exactly this question, so the UI evidently doesn't cover it in practice. Implemented as `split_by_teammate()` — see "Teammate analysis" below.
- No local persistence of *Leetify* match history across sessions. The API's match-history endpoint always returns a fixed window of the ~100 most recent matches with no working pagination or date-filter parameters (confirmed empirically — `limit`, `page`, `offset`, `before`, `startDate`/`endDate` were all tried and had no effect on the response). This caps analyzable trend depth at whatever that window currently covers (~6 weeks for this user as of 2026-07-07). Building a local cache to accumulate *Leetify* history beyond that window was considered and explicitly deferred — see Future Work. (This is distinct from the one-time csstats.gg CSV seed described below, which is a manually-triggered snapshot, not an automated accumulating cache.)
- No fixed set of report types (e.g. "weekly report," "map report"). Analysis is done ad hoc per question against the flattened match data, not pre-built into the client.
- No test suite. This is a small personal script against a single third-party API; correctness is established via a smoke check against real data during implementation, not automated tests. **Reaffirmed 2026-07-16 by explicit preference, with a known cost:** two silent data-corruption bugs shipped and went unnoticed for 8 days, and a handful of tests would have caught both cheaply. Manual verification recipes stand in for them — see `docs/notes/2026-07-16-data-integrity-findings.md`, and run them after any csstats re-export.

## Architecture

A single Python module, no CLI wrapper, no server. Claude invokes its functions directly (via Bash, calling `python3 -c "..."` or a short script) when answering a user question in-session.

## API Reference (Leetify Public API)

- Base URL: `https://api-public.cs-prod.leetify.com`
- Auth: header `_leetify_key: <LEETIFY_API_KEY>`
- Key validation: `GET /api-key/validate` → 200 valid / 401 invalid or missing / 500 server error
- `GET /v3/profile?id={steam64_id}` → profile summary: `privacy_mode`, `winrate`, `total_matches`, `first_match_date`, `name`, `bans`, `steam64_id`, `id`, `ranks`, `rating`, `stats`, `recent_matches`, `recent_teammates`
- `GET /v3/profile/matches?id={steam64_id}` → array of the ~100 most recent matches. No working pagination/filter params. Each match has: `id`, `finished_at`, `data_source`, `data_source_match_id`, `map_name`, `has_banned_player`, `team_scores` (array of `{team_number, score}`), `stats` (array of per-player stat blocks for all 10 players in the match, each keyed by `steam64_id`, `initial_team_number`, and ~30 raw stat fields: accuracy, preaim, reaction_time, utility usage, counter-strafing, score, etc.)
- `GET /v2/matches/{gameId}` and `GET /v2/matches/{dataSource}/{dataSourceId}` exist for full single-match detail but are not needed for this tool — `/v3/profile/matches` already embeds per-player `stats` inline.

The SteamID64 to analyze comes from `CS2_STEAM_ID` in `.env` (see `.env.example`); `leetify_client.my_steam_id()` reads it.

## Components

Dependency management: a Poetry project scoped to `cs2/` (`pyproject.toml`), with `requests` as its only dependency. Invoked via `poetry run` rather than relying on any ambient/system-wide package install.

`cs2/leetify_client.py`:

- `load_api_key() -> str` — reads `LEETIFY_API_KEY` from `cs2/.env`. Raises if missing.
- `get_profile(steam_id: str) -> dict` — calls `GET /v3/profile`, returns parsed JSON.
- `get_match_history(steam_id: str) -> list[dict]` — calls `GET /v3/profile/matches`, returns parsed JSON array.
- `flatten_matches(steam_id: str, matches: list[dict]) -> list[dict]` — for each match, extracts the entry in `stats` whose `steam64_id` matches the user, merges it with match-level fields (`finished_at`, `map_name`, `data_source`, `match_id`), and computes `won: bool | None` by comparing the user's `initial_team_number`'s score against the other team's score in `team_scores` (`True` if strictly higher, `False` if strictly lower, `None` if equal/tied). Returns one flat dict per match, suitable for direct filtering/aggregation with plain Python (sort by date, group by map, slice by index, etc.) — no pandas dependency.

## Data Flow

1. User asks a coaching question in a Claude Code session.
2. Claude calls `get_match_history` + `flatten_matches` (and `get_profile` if aggregate/rank fields are needed) via Bash.
3. Claude filters/aggregates the flattened data to whatever the specific question requires (e.g. last N matches, matches on a given map, T-side vs CT-side split).
4. Claude interprets the result and answers conversationally, as a coach — the module only prepares clean data, it does not generate prose.

## Error Handling

Minimal, matching the tool's scope: an interactive script invoked by Claude, not a long-running service. If the API key is invalid/missing (401) or a request otherwise fails, the client should raise with the response body surfaced — no retries, no backoff.

## Testing

No automated test suite. Verification is a one-time smoke check during implementation: call `get_profile` and `get_match_history` against the real Steam ID above, confirm expected top-level keys are present, and confirm `flatten_matches` produces one dict per match with a correctly computed `won` field (spot-checked against `team_scores` from the raw response).

## Supplementary Data Source: csstats.gg (added 2026-07-08)

Leetify's ~6-week window turned out to be a real limitation in practice (e.g. answering "am I still adapting to a sensitivity/mouse change from a few weeks ago" needs a pre-change baseline the API can't reach). csstats.gg tracks a much longer history (this user: 889 matches back to 2023-11-10 as of the 2026-07-16 export, vs. Leetify's ~100-match API window) but has **no official public API** and is Cloudflare-protected against scripted scraping — confirmed by a direct fetch attempt returning HTTP 403, and by research finding only an unofficial third-party paid scraper wrapper (Parse.bot), which was rejected as an integration path (cost, fragility, no sanction from csstats.gg).

Instead, the data was obtained via a **one-time manual export**: the user browsed their own csstats.gg match history in Chrome (a normal authenticated session, not a bot) and used the Claude for Chrome extension to extract the visible table and save it as a CSV download. This sidesteps the anti-bot protection entirely since it's genuine human browsing, not scripted access.

`cs2/csstats_data.py`:
- `load_csstats_history() -> list[dict]` — pure function, no network call. Parses `cs2/data/csstats_match_history.csv` (gitignored — personal data snapshot, not source code) into one dict per match: `timestamp` (exact, ISO-8601 UTC), `date` (local calendar date, DST-aware `America/Chicago`, matching what csstats displays), `map`, `won: bool | None`, `score`, `own_score`, `other_score`, `kills`, `deaths`, `assists`, `adr`, `hs_percent`, `rating`. Sorted oldest-first.
  - **Everything derives from the `timestamp` column** (added to the export 2026-07-16). The CSV's `date` column also carries relative labels ("3 hours ago", "yesterday") for its newest rows; these are deliberately never parsed. csstats' "yesterday" is a rolling 24-48h bucket rather than a calendar day and is not resolvable to a single date. An earlier version resolved these against `datetime.now()` and silently drifted a day per day. See `docs/notes/2026-07-16-data-integrity-findings.md`.
  - Raises on a missing `timestamp`/`adr`/`hs_percent` column, and on any `hs_percent > 100` — which catches the column transposition that corrupted the 2026-07-08 export.

This is **not merged** into Leetify's `flatten_matches` schema — the two sources have different fields (csstats has no aim/positioning/utility breakdown; Leetify only covers ~6 weeks) and forcing a union would mean inventing data neither source actually provides. They're kept as two independently queryable sources: csstats.gg for long-range win rate/K-D/ADR/rating trend, Leetify for recent detailed mechanics.

**Staleness:** the CSV is a snapshot as of whenever it was exported (currently 2026-07-16, 889 matches through 2026-07-15) — it will not update itself. Getting fresher long-range data means repeating the manual Chrome-extension export. **The export must include the exact `timestamp` column**; without it the newest rows are only relative labels and are not reliably datable. The re-export procedure, including a ready-to-paste prompt, is in the skill (`cs2-coach/SKILL.md`).

## Teammate analysis (added 2026-07-16)

`split_by_teammate(flat_matches, teammate_id, teammate_matches) -> dict` in `leetify_client.py`. Pure, no network call; the caller fetches both histories. It takes `flatten_matches()` output rather than raw matches so it composes with `attach_csstats_adr()` — flattening internally would have silently discarded the caller's ADR overlay.

A match response's `stats` array contains **only the requesting player's own block** — there is no roster, and no match-detail endpoint exists (`/v3/matches/{id}`, `/v3/match/{id}`, `/v3/matches?id=`, `/v3/match?id=`, `/v3/profile/matches/{id}` all 404 as of 2026-07-16). So the teammate's history is fetched separately and joined on `data_source_match_id`, which is shared between both players' responses — the UUID `id` is not. `get_profile()` accepts any SteamID64 and returns that account's `name`, which is how a name is resolved to an ID (`recent_teammates` gives IDs only); a private or unregistered profile 404s.

Two correctness details worth preserving: sharing a match does not imply sharing a team, so `initial_team_number` is compared rather than assumed (opponents bucket into `against`); and `covers_window` reports whether the teammate's ~100-match cap reaches back far enough for `without` to be trustworthy, since otherwise shared matches silently misfile as `without`.

## Future Work (explicitly deferred, not part of this implementation)

- **Local accumulating cache of *Leetify* match history** (ready to implement next session): confirmed no other source can fill this gap — Leetify's own website UI caps at 30 matches (less than our 100-match API window), and the aim/positioning/utility-level detail only ever exists if computed from the match demo at the time it was played; once Valve's demo retention expires, that detail is permanently unrecoverable from any tool, including Leetify itself beyond what it already exposes. So this cache cannot backfill the past — it can only stop the ~6-week window from being a hard ceiling going forward.
  - New file `cs2/leetify_cache.py`: `load_cache() -> list[dict]` and `append_new_matches(flattened: list[dict])`, reading/writing `cs2/data/leetify_cache.jsonl` (JSONL — one flattened match per line, same shape as `flatten_matches`'s output, so no schema translation needed).
  - Dedupe by `match_id`; append-only, skip anything already cached.
  - No scheduled job: each time the coach answers a real question, it opportunistically appends any new matches from the live 100-match window into the cache first. Growth is a side effect of normal use, matching the project's "no scheduled job" philosophy.
  - Starts empty (no backfill possible, per above) — depth grows only from this point forward, but that permanently closes the gap between "6 weeks" and "however long this tool keeps being used."
- Migrate the csstats.gg CSV (and/or a future Leetify cache) to SQLite if the flat-file approach becomes unwieldy (e.g. needing indexed queries, incremental updates without re-parsing the whole file, or merging multiple sources more richly). Explicitly deferred for now — a plain CSV is more than sufficient for ~860 rows queried with plain Python.
