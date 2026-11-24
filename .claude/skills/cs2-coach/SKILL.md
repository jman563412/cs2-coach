---
name: cs2-coach
description: Use when the user asks about their own CS2 (Counter-Strike 2) performance, stats, trends, win rate, rank, aim/positioning/utility ratings, or map-specific results. Fetches live data from the user's Leetify account and answers ad hoc, without a fixed report format.
---

# CS2 Coach

## Purpose

Answer ad hoc questions about the user's own CS2 performance by pulling fresh data from their Leetify account and analyzing whatever the specific question calls for — no fixed report types, no scheduled jobs, no local persistence. Every invocation fetches live.

## Where the code lives

This skill lives at `.claude/skills/cs2-coach/` inside the repo, so the repo root is three levels up from this
skill's base directory. The skill may be loaded through a symlink (e.g. `~/.claude/skills/cs2-coach`), so resolve it
physically: `cd "$(realpath '<this skill's base directory>/../../..')"`. Run every command below from the repo root
(`cd <repo root>` in the snippets means that).

Client module: `leetify_client.py`
Design spec (background/rationale): `docs/superpowers/specs/2026-07-07-cs2-coach-design.md`

The SteamID64 to analyze is `CS2_STEAM_ID` in the repo's `.env`; `leetify_client.my_steam_id()` reads it. If it's
missing, ask the user for their SteamID64 and have them add it to `.env` (see `.env.example`). Never hardcode it.

## How to fetch data

Always invoke via Poetry from the project directory — do not rely on any ambient/system Python:

```bash
cd <repo root>
poetry run python3 -c "
from leetify_client import get_match_history, flatten_matches, get_profile, my_steam_id
ME = my_steam_id()
matches = get_match_history(ME)
flat = flatten_matches(ME, matches)
# ... filter/aggregate flat for whatever the question asks, then print just what's needed
"
```

Available functions (all in `leetify_client.py`):

- `load_api_key() -> str` — internal; reads `LEETIFY_API_KEY` from the repo's `.env`. Never call this directly for output, and never print its return value.
- `get_profile(steam_id: str) -> dict` — profile summary: `privacy_mode`, `name`, `winrate`, `total_matches`, `first_match_date`, `ranks` (leetify/premier/faceit/wingman/competitive-per-map), `rating` (aim/positioning/utility/clutch/opening/ct_leetify/t_leetify), `recent_matches`, `recent_teammates`. Works for **any** SteamID64, not just the user's — see "Analyzing performance with a specific teammate" below.
- `get_match_history(steam_id: str) -> list[dict]` — the ~100 most recent matches, raw. Each has `id`, `data_source_match_id`, `finished_at`, `map_name`, `data_source`, `has_banned_player`, `team_scores`, and a `stats` array.
  - **The `stats` array contains exactly ONE block: the requested player's own.** It does *not* contain the other nine players — there is no roster data in this response, and no other endpoint exposes one (see Constraints). An earlier version of this doc claimed all 10 players were present; that was wrong and cost a session real time. To learn anything about another player, you must fetch *their* history separately by their SteamID64.
- `flatten_matches(steam_id: str, matches: list[dict]) -> list[dict]` — pure, no network call. Takes `get_match_history`'s output and returns one flat dict per match: the user's own stat block merged with `finished_at`, `map_name`, `data_source`, `match_id`, `data_source_match_id`, `rounds_total`, a computed `adr`, and a computed `won: bool | None` (`True`/`False`/`None` for tie). This is almost always what you want to filter/sort/aggregate against — prefer it over walking the raw match list yourself.
  - **Use `adr`, not the API's `dpr`.** `dpr` divides by Leetify's `rounds_count`, which contradicted the match's own `team_scores` on 1 of 100 matches sampled (making `dpr` wrong by 6.9 there); `adr` divides `total_damage` by `sum(team_scores)`.
  - **But for ADR, use `attach_csstats_adr()` (below) rather than Leetify's own number.** Verified against Valve's own figure (2026-07-13 de_dust2: Valve 97, csstats 96, Leetify 90.57) — **csstats tracks Valve; Leetify's `total_damage` can simply be understated**, and no computation over it can fix that. Leetify agrees with csstats on 96 of 98 matches, so it's fine in aggregate, but don't trust it for a single match's ADR. Leetify remains the only source for mechanical stats (preaim, counter-strafing, accuracy) — those aren't in question.
  - **Ground truth for any one match:** download its demo in CS2's match history and play it back. The end-of-match screen shows ADR / K-D-A / HSP directly. No parsing needed. CS2's match *history* list doesn't show ADR — only the demo playback screen does.
- `attach_csstats_adr(flat_matches: list[dict], csstats_history: list[dict]) -> list[dict]` — pure, no network call. Overlays csstats' (Valve-accurate) ADR onto `flatten_matches()` output, joined on exact timestamp. Sets `adr` to the best available value and `adr_source` to `"csstats"` or `"leetify"`. **Use this for any ADR question.** Matches played after the last csstats export fall back to Leetify's computed `adr` and are marked accordingly — check `adr_source` if precision matters. Takes the history as an argument rather than importing it, keeping the two sources independent.

  ```bash
  cd <repo root>
  poetry run python3 -c "
  from leetify_client import get_match_history, flatten_matches, attach_csstats_adr, my_steam_id
  from csstats_data import load_csstats_history
  ME = my_steam_id()
  flat = attach_csstats_adr(flatten_matches(ME, get_match_history(ME)), load_csstats_history())
  "
  ```
- `split_by_teammate(flat_matches: list[dict], teammate_id: str, teammate_matches: list[dict]) -> dict` — pure, no network call. Splits the user's matches by whether a teammate played alongside them. Takes `flatten_matches()` output (run it through `attach_csstats_adr()` first if ADR matters — the buckets keep whatever you pass in), plus the teammate's **raw** `get_match_history()` output. Returns `{"with": [...], "against": [...], "without": [...], "covers_window": bool}`. Read the docstring before using — the `covers_window` flag matters. Raises if `teammate_id` matches no stat block (usually: passed as an int, or an ID from another source) rather than silently reporting `with=0`.

## Analyzing performance with a specific teammate (added 2026-07-16)

"How do I play with X?" is answerable, but not the obvious way — match responses carry no roster. The technique:

1. **Resolve the name to a SteamID64.** `get_profile(steam_id)` accepts any SteamID64 and returns that account's `name` and `privacy_mode`. `get_profile(user)["recent_teammates"]` gives `steam64_id` + `recent_matches_count` for the user's regulars, but **no names** — so call `get_profile` on each ID to see who they are. A private/unregistered profile raises `RuntimeError: Leetify API error 404`; catch it, since some teammates simply won't be queryable.
2. **Fetch the teammate's own match history** with `get_match_history(their_id)`.
3. **Join on `data_source_match_id`** (the `CSGO-XXXXX-...` code). A match's `id` (UUID) differs between the two players' histories, but `data_source_match_id` is identical — that's the join key.
4. **Use `split_by_teammate`**, which does steps 3–4 including the same-team check and coverage guard:

```bash
cd <repo root>
poetry run python3 -c "
from leetify_client import get_match_history, flatten_matches, attach_csstats_adr, split_by_teammate, my_steam_id
from csstats_data import load_csstats_history
ME, THEM = my_steam_id(), '<teammate SteamID64>'
flat = attach_csstats_adr(flatten_matches(ME, get_match_history(ME)), load_csstats_history())
r = split_by_teammate(flat, THEM, get_match_history(THEM))
print(len(r['with']), len(r['without']), r['covers_window'])
"
```

(Drop the `attach_csstats_adr` line if the question doesn't involve ADR — but it costs nothing, and leaving it out silently gives the buckets Leetify's less accurate ADR.)

Two traps this encodes — don't undo them:

- **Same match ≠ same team.** They may have been an opponent. `split_by_teammate` compares `initial_team_number` and buckets those into `against` rather than silently counting them as teammates.
- **`covers_window` is a correctness guard, not a nicety.** Both histories cap at ~100 matches. If the teammate's history starts *later* than the user's, their older games are missing and shared matches get misfiled as `without`, biasing the comparison. If `covers_window` is False, restrict the analysis to the overlapping date range or say so.

**Sanity check available for free:** the size of `with` should equal that teammate's `recent_matches_count` in `recent_teammates`. Two independent code paths agreeing is a strong signal the join worked.

**Identity is a snapshot, not a fact.** Leetify's `name` is whatever it last cached; Steam names change constantly. If the user names someone from their friends list, the account you find may not be who they meant — confirm the SteamID64 with them before presenting conclusions. Steam's Web API (`GetFriendList` + `GetPlayerSummaries`) resolves current names properly, but has **no CS2 match-roster endpoint**, so it can identify people and nothing more; the analysis itself still has to come from the Leetify join above.

**Watch the sample size.** These splits are small (tens of matches). Run a permutation test before calling any gap real — a 4pp win-rate difference over 37 matches is indistinguishable from noise, and reporting it as a finding would be actively misleading. Check map mix across the two groups too; it's a live confound.

## Longer-range history: csstats.gg (separate source, added 2026-07-08)

Leetify's API only ever covers the last ~100 matches (how far back that reaches depends on how often the user plays). For anything further back, there's a second, separate module: `csstats_data.py`, with one function:

- `load_csstats_history() -> list[dict]` — pure, no network call (reads a local CSV, not an API). Returns one dict per match, sorted oldest-first: `timestamp` (exact, ISO-8601 UTC), `date` (ISO local date, matching what csstats displays), `map`, `won: bool | None`, `score`, `own_score`, `other_score`, `kills`, `deaths`, `assists`, `adr`, `hs_percent`, `rating`. Covers whatever range the user's last csstats.gg export reached (see Staleness below).

Fetch it the same way, via Poetry from the project directory:

```bash
cd <repo root>
poetry run python3 -c "
from csstats_data import load_csstats_history
history = load_csstats_history()
# ... filter/aggregate for whatever the question asks
"
```

**Do not merge this with `flatten_matches`'s output** — the two have genuinely different fields (csstats has no aim/positioning/utility/preaim breakdown; Leetify only covers ~6 weeks). Use csstats for long-range win rate / K-D / ADR / rating trend questions, and Leetify for anything needing the detailed mechanical stats (aim, positioning, utility, counter-strafing) or when the question is scoped within the last ~6 weeks.

**Staleness:** this is a manual snapshot (exported by the user browsing csstats.gg in Chrome with the Claude for Chrome extension — csstats.gg has no official API and is Cloudflare-protected against scripted access; a scripted fetch returns 403). It will not update itself. If the user asks about very recent matches and the csstats data looks stale relative to Leetify's, say so — don't present it as current. Getting a fresher snapshot means repeating that manual export; that's a "help me do this again" conversation with the user, not something to script automatically. **When re-exporting, always request the `timestamp` column** — it's the column that makes this data trustworthy (see below).

**Dates:** the CSV's `date` column carries *relative* labels ("3 hours ago", "yesterday") for its newest rows. **Ignore them — use `timestamp`.** csstats' "yesterday" is a rolling 24-48h bucket rather than a calendar day, so it is genuinely unresolvable to a single date (two matches 79 minutes apart on the same local day were labeled "yesterday" and "Mon, Jul 6th, 26"). The loader derives everything from the exact `timestamp` column and never parses the relative labels. The prior version resolved them against `datetime.now()`, so they silently walked forward a day per day and were 8 days off before anyone noticed.

The `date` field is a local calendar date (in **`CS2_TIMEZONE`** from `.env`, default UTC; it must be a DST-aware IANA zone — a fixed UTC-5 offset is wrong for Nov–Mar and misses 49 of 887 dates; it only *looks* right if you sample summer matches). Leetify's `finished_at` is **UTC**. Since both sources now carry exact timestamps, cross-referencing them is a clean equality join on the instant — no fingerprinting by map/K/D, which produces false pairings across a multi-year history.

**Re-export procedure** (when the user wants fresher long-range data). csstats has no API and 403s scripted fetches, so this is a manual step the user drives in Chrome with the Claude for Chrome extension. Hand them this prompt — it worked on 2026-07-16:

```
Go to https://csstats.gg/player/<CS2_STEAM_ID> and open the match history.
Scroll/paginate until the ENTIRE history is loaded (check the oldest match date csstats
shows for this account). Confirm you've hit the oldest row before exporting;
a partial export is worse than none.

Save as a CSV download with these columns, in this exact order and naming:
  date,map,result,score,kills,deaths,assists,adr,hs_percent,rating,timestamp

- timestamp: the exact underlying value behind the relative dates (ISO-8601 UTC)
- date: exactly as csstats displays it (e.g. "Mon, Jul 6th, 26")
- result: one of win / loss / tie
- score: "13:10" format, own score first
- adr, hs_percent, rating: numbers only, no units or % sign

Double-check that `adr` and `hs_percent` are not swapped -- ADR is typically
75-85 and can exceed 100; hs_percent is typically 45-55 and cannot exceed 100.

Do not script or automate the page fetch -- csstats is Cloudflare-protected and
only normal in-browser reading works.
```

The download lands in the user's browser download folder (on WSL, Windows' Downloads is `/mnt/c/Users/<windows user>/Downloads/`). Copy it to `data/csstats_match_history.csv` (gitignored). Then run the verification recipes in `docs/notes/2026-07-16-data-integrity-findings.md` before trusting it.

**Trust but verify a new export.** The 2026-07-08 export silently had `adr` and `hs_percent` **transposed**, which made every csstats ADR/HS number wrong for 8 days (it reported a ~80% mean headshot rate and ~49 mean ADR — the two are simply swapped). `load_csstats_history()` now raises if any `hs_percent` exceeds 100, which catches that specific transposition, but it is not a general guarantee. After any re-export, spot-check `adr`/`hs_percent` against Leetify's `adr` and headshot rate on the overlapping window before trusting a trend (recipe 1 in `docs/notes/2026-07-16-data-integrity-findings.md`). Expect near-total agreement (measured on the original 2026-07 export: 96 of 98 within 1.0 ADR, mean difference +0.145. Sub-1.0 spread is just csstats reporting ADR as an integer. That export had exactly one outlier, 5.4 apart, where the two sources disagreed on total damage; cause unknown). Widespread disagreement, or any HS% mismatch, means a bad export.

## Constraints that took real effort to discover — don't relitigate these

- Auth header is `_leetify_key: <key>` — not `Authorization: Bearer`, despite that being how the separate `/api-key/validate` endpoint accepts credentials.
- `/v3/profile/matches` has **no working pagination or date filtering**. `limit`, `page`, `offset`, `before`, `startDate`/`endDate` were all tried and silently ignored — it always returns exactly the same ~100 most recent matches. If a question needs data older than that window covers, say so explicitly rather than assuming you can page further back; there is currently no way to retrieve older matches through this API.
- **There is no match-detail / roster endpoint.** Probed 2026-07-16, all 404: `/v3/matches/{id}`, `/v3/match/{id}`, `/v3/matches?id=`, `/v3/match?id=`, `/v3/profile/matches/{id}`. The only two working endpoints are `/v3/profile` and `/v3/profile/matches`. To find out who else was in a match, fetch the other player's history and join on `data_source_match_id` — see the teammate section above.
- **Never print, log, or echo the raw API key value** in any command output, in code you write, or in your response to the user. Only its presence/absence or length is ever safe to reference.

## How to answer questions

There is no fixed report format by design. When the user asks something:

1. Fetch `get_match_history` + `flatten_matches` (and `get_profile` if aggregate/rank fields are needed).
2. Filter/aggregate the flattened data to exactly what was asked — last N matches, a specific map, T-side vs CT-side, a date range within the available window, etc. Do this with plain Python (no pandas dependency in the project).
3. Interpret the result and answer conversationally, as a coach would — the module only prepares clean data; you supply the analysis and framing each time.

If the question needs a trend longer than the ~100-match Leetify window can show, check `load_csstats_history()` first (see above) before telling the user it's unavailable — it very likely covers the range needed.
