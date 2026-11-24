# Data Integrity Findings

Date: 2026-07-16

Two silent data-corruption bugs surfaced while answering an unrelated question ("how do I perform when playing with a specific teammate"). Both had been live for 8 days and neither announced itself — they produced plausible-looking numbers that fed a published conclusion. This note records what broke, how it was caught, and how to re-check it, since there is no automated test suite (see "On testing" below).

## Bug 1: transposed `adr` / `hs_percent` (severity: high)

The 2026-07-08 csstats.gg export had its `adr` and `hs_percent` columns swapped, and `load_csstats_history()` passed the error through unexamined. Every csstats ADR/HS number between 2026-07-08 and 2026-07-16 was wrong.

- **How it looked:** mean HS% of 79.7% and mean ADR of 48.8 across 864 matches. The two are simply each other. An ~80% headshot rate is impossible and should have been caught on sight.
- **How it was caught:** a new export had the columns in the opposite order. Cross-checking both against Leetify's `dpr` settled which was right — 71 of 72 unambiguous pairs matched the transposed reading of the old file.
- **Blast radius:** `docs/notes/2026-07-08-mouse-sens-adaptation.md` built conclusions on it, including a claim about "worst HS%" that was actually about ADR. That note has been retracted and redone.
- **Now guarded:** `load_csstats_history()` raises if any `hs_percent > 100`. ADR routinely exceeds 100 and a headshot percentage cannot, so this specific transposition is now detectable. It is *not* a general guarantee — a swap where every ADR happened to be under 100 would still pass.

## Bug 2: relative dates resolved against `now()` (severity: medium)

csstats renders its newest rows as relative labels ("3 hours ago", "yesterday"). The old loader resolved them against `datetime.now()`, so those rows walked forward one day for every day the file sat on disk — 8 days off by the time it was noticed.

- **Root cause:** the CSV is a frozen snapshot; anything resolved against "now" drifts.
- **Compounding discovery:** csstats' "yesterday" is a rolling 24-48h bucket, not a calendar day, so it was never resolvable to a single date. Proof: two matches 79 minutes apart on the same local day (2026-07-06 05:26Z and 06:45Z) were labeled `"Mon, Jul 6th, 26"` and `"yesterday"` respectively. A brute force over all 1,681 possible day-boundary offsets at one-minute resolution found **zero** that explain the labels. The label tracks elapsed time (the flip is at ~48h), not the calendar.
- **Fixed properly:** the export now includes an exact ISO-8601 UTC `timestamp` column. All relative-date parsing is gone, along with the `EXPORT_TIME` constant it needed. There is no longer a moving part to go stale.

## Near-miss: fixed UTC offset vs DST

While fixing Bug 2, a fixed `UTC-5` offset was validated against a sample that was entirely July — all CDT — where it scored 67/68 and looked correct. Across the full 2023-2026 history it misses 49 dates, **every one in November-March**. Now uses DST-aware `ZoneInfo("America/Chicago")`, which agrees on all 887 displayed dates.

The lesson generalizes to both bugs above: all three defects survived because they were checked against samples too narrow to expose them.

## Verification recipes

Run these after any re-export, or when a csstats-derived number looks surprising. These are the checks that would otherwise be unit tests.

**1. Columns are not transposed** (catches Bug 1). Joins on exact timestamp and compares against Leetify:

```bash
cd <repo root>
poetry run python3 -c "
import datetime
from csstats_data import load_csstats_history
from leetify_client import get_match_history, flatten_matches, my_steam_id
h={x['timestamp']:x for x in load_csstats_history()}
f=flatten_matches(my_steam_id(), get_match_history(my_steam_id()))
ok=bad=miss=0
for m in f:
    ts=datetime.datetime.fromisoformat(m['finished_at'].replace('Z','+00:00')).isoformat()
    r=h.get(ts)
    if not r: miss+=1; continue
    lt_hs=100*m['total_hs_kills']/m['total_kills'] if m['total_kills'] else 0
    if abs(r['adr']-m['adr'])<=5 and abs(r['hs_percent']-lt_hs)<=5: ok+=1
    else: bad+=1
print('agree=%d disagree=%d not-in-csstats=%d' % (ok,bad,miss))
"
```

Expect nearly all agreeing. As of 2026-07-16 this reported **97 agree, 1 disagree, 2 missing** — the 1 disagreement is the known 07-13 outlier below, and "missing" counts matches played after the export, so it grows until the CSV is re-exported. Compare against Leetify's computed `adr`, **not** its raw `dpr` (an earlier version of this recipe used `dpr` and reported a phantom disagreement on the 06-29 match that `flatten_matches` already corrects). Widespread disagreement, or any HS% mismatch, means a transposed export.

**How closely the two sources actually agree on ADR** (98 overlapping matches, measured 2026-07-16, *before* the `adr` fix described below):

| | |
|---|---|
| Mean difference (csstats − Leetify) | **+0.07** |
| Median difference | +0.10 |
| Within 1.0 ADR | 95 / 98 |
| Beyond 5.0 ADR | 2 / 98 |
| Largest difference | 6.8 |

There is no systematic bias — the mean difference is 0.09% of a typical ~80 ADR, so this does not distort trends. The sub-1.0 spread is mostly rounding: **csstats reports ADR as an integer**, Leetify as a decimal.

The 2 outliers are not fully explained, and it is worth not inventing a story for them (an earlier version of this note asserted "the two sites count rounds differently," which fits one case and is wrong for the other). Working backwards from each ADR to the round count it implies:

- **2026-06-29 inferno**, scoreline `1:12` (= 13 rounds): csstats' ADR implies exactly 13.0 rounds, but Leetify's own `rounds_count` reports 12 — contradicting its own scoreline. Here csstats appears correct and Leetify looks to have dropped a round.
- **2026-07-13 dust2**, scoreline `1:13` (= 14 rounds): Leetify's `rounds_count` agrees at 14, but csstats' ADR implies 13.2 — matching neither 13 nor 14. Round counting cannot explain this one; the two sources must disagree on total damage. Cause unknown.

**One of the two was fixable and has been fixed.** The 06-29 case was a defect in Leetify's `dpr`, not in csstats. `flatten_matches()` now computes `adr = total_damage / sum(team_scores)` instead of trusting `dpr`, which divides by Leetify's `rounds_count`. That match now computes 82.92 against csstats' 83 — agreement restored. Leetify's `rounds_count`/`rounds_won`/`rounds_lost` are internally consistent on all 100 matches but disagree with `team_scores` on that one, and csstats independently corroborates `team_scores`. Use `adr`; `dpr` is retained only for comparison.

The 07-13 dust2 case is a genuine disagreement on total damage (csstats implies 1344, Leetify reports 1268) and is **not** fixable from Leetify's data. It was later settled against Valve's own figure — csstats is right, Leetify understates the damage — see "Resolved: Valve's ADR is obtainable" below.

Post-fix agreement: mean difference +0.145 ADR, 96 of 98 within 1.0, 1 outlier beyond 5.0 (down from 2).

The reason to record this: if a future check shows *many* matches disagreeing by >5, that is a different problem than this one and should not be waved off as "the known ADR outlier."

### Resolved: Valve's ADR is obtainable, and csstats matches it — Leetify does not

**Where to find Valve's ADR:** download the match demo from CS2's match history and play it back. The end-of-match screen shows each player's **ADR, K-D-A, and HSP** directly. That is Valve's own figure and is authoritative. It requires no demo-parsing tooling — just watch the replay and read the number.

Two wrong turns are worth recording so they are not repeated. CS2's *match history* scoreboard shows only `K`/`A`/`D`/`★`/`Score`, which led to a premature conclusion that CS2 never surfaces ADR and that "match Valve's number" had no target. It does — on the demo playback screen, not the history list. It was then assumed that reaching the demo's figure would need a parsing dependency and was disproportionate. It needs neither.

**Ground truth check, 2026-07-13 de_dust2** (the disputed match):

| Source | ADR | K-D-A | HS% | vs Valve |
|---|---|---|---|---|
| **Valve (demo playback)** | **97** | 15-13-1 | 53% | — |
| csstats | 96 | 15-13-1 | 53% | −1 (integer rounding) |
| Leetify (`adr`, and raw `dpr`) | 90.57 | 15-13-1 | 53.3% | **−6.43 (wrong)** |

Implied total damage over 14 rounds: Valve ~1358, csstats ~1344, **Leetify 1268 — undercounting by ~90 damage**.

**Conclusions:**

- **csstats' ADR tracks Valve** to within integer rounding. For ADR, treat csstats as the accurate source.
- **Leetify's damage can simply be wrong**, and this is not a round-counting artifact — rounds agree at 14 here. `total_damage` itself is understated, so the computed `adr` cannot rescue it; it divides the same bad number. Leetify's ADR agrees with csstats on 96 of 98 matches, but is not trustworthy for an individual match.
- K-D-A and HS% agree across all three sources, so only the damage-derived figure is affected.
- The earlier "short matches where the two sites count rounds differently" explanation was wrong in both cases: 06-29 was Leetify's round count, 07-13 is Leetify's damage. Two unrelated Leetify defects.

**Spot-checking a match against Valve is cheap.** Download the demo, play it back, read the end screen. Do this whenever a specific match's ADR looks suspect.

#### Falsified: "the data just hadn't finished loading when we fetched it"

A natural hypothesis, and worth recording because it is wrong in a way that is easy to re-derive. It explains one thing and not the other:

- **True for the `adr_source: "leetify"` fallbacks.** Those matches postdate the csstats export, so no csstats row exists to join. Pure snapshot staleness, not a defect — a fresh export resolves them.
- **False for the two ADR defects.** Three independent reasons:

1. **Age at fetch.** Re-fetched from Leetify 2026-07-16T09:11Z, the 06-29 match was **17.2 days old** and the 07-13 match **3.2 days old** — and both were still wrong. Leetify processes demos within minutes to hours, so these fetches *are* the post-ingest measurement. A 17-day-old match is final.

2. **The failure signatures point the wrong way.** A partially-ingested match would lose rounds *and* damage together. Instead each defect has the other half fully intact:
   - **06-29**: damage **complete** (1078 vs csstats' implied ~1079 — they agree), but `rounds_count=12` for a 13-round match. Every damage event present; round count wrong.
   - **07-13**: rounds **complete and internally coherent** (`rounds_count=14`, `rounds_won+rounds_lost=14`, `sum(team_scores)=14` all agree), but damage ~76-90 short.

   A half-loaded match cannot hold all the damage while missing a round, nor know all 14 rounds while losing damage from one. These are event-level parsing gaps inside otherwise fully-ingested matches — two unrelated bugs that merely look alike from outside.

3. **External corroboration.** csstats parsed the *same demos* and got both right, matching Valve on 07-13. Incomplete demos at ingest would have affected it too.

Conclusion: staleness explains the fallbacks; the ADR discrepancies are genuine Leetify parser defects and will not resolve themselves with time. Re-fetching does not fix them.

Ruled out for *bulk* verification (checked 2026-07-16): **GSI cannot supply damage** — `round_totaldmg` appears in **0 of 9,446** captured payloads, and this is not a config gap (`player_state "1"` enables the whole block and every other field arrives), so CS2 does not emit it over GSI. **No Valve API** exposes match scoreboards. So automated verification is not possible; per-match demo playback is the only route, and it is manual.

`flatten_matches()`'s computed `adr` still stands: it fixes a real self-contradiction in Leetify's data (`rounds_count` vs `team_scores`) and corrected the 06-29 match to agree with csstats. But it does not make Leetify's ADR authoritative.

**Resolution:** `attach_csstats_adr(flat_matches, csstats_history)` in `leetify_client.py` overlays csstats' ADR onto Leetify matches, joined on exact timestamp, and records which source won in `adr_source`. 98 of 100 Leetify matches resolve to csstats; the rest were played after the export and fall back to Leetify's computed value. Use it for any ADR question.

The reasoning is about the *shape* of each source's error, not just its size. csstats' error is bounded and small (±0.5 from integer rounding, averaging toward zero across many matches). Leetify's is rare but large (~6 when it drops data, on ~2% of matches). Bounded-and-small is the safer default even though csstats is nominally less precise per match.

This does not change any conclusion drawn so far — the teammate ADR split moves from 72.54/80.43 to 72.77/80.53 — but it makes individual matches correct, which is what a per-match question would surface.

### Why the residual is not worth chasing

csstats exports ADR as an **integer**. That is a hard source-level limit — the decimals do not exist in the export and cannot be recovered, so "exact" csstats ADR is unobtainable regardless of what the loader does. It also does not matter, because rounding is negligible next to the sampling noise already present in any average:

| Year | n | ADR | Rounding error | Sampling error | Ratio |
|---|---|---|---|---|---|
| 2023 | 39 | 72.5 | ±0.046 | ±3.45 | 75× |
| 2024 | 205 | 77.7 | ±0.020 | ±1.65 | 82× |
| 2025 | 154 | 77.1 | ±0.023 | ±1.88 | 81× |
| 2026 | 491 | 81.7 | ±0.013 | ±1.07 | 82× |

Integer rounding averages out at ±0.29/√n, so it perturbs a yearly ADR by ~0.01 — roughly **80× smaller** than the ±1 of ordinary sampling noise in the same figure. Eliminating it entirely would not change a single conclusion. Note also that the ~0.09% figure quoted during this investigation is the *disagreement between two independent sources*, not an error rate in our data; the loader reports exactly what csstats exported.

**2. Derived dates match what csstats displays** (catches the DST near-miss). Compare `date` against the CSV's absolute labels; expect **0 disagreements**, and be suspicious of a check that only covers summer months:

```bash
poetry run python3 -c "
import csv, re, datetime
from csstats_data import DISPLAY_TZ
raw=list(csv.DictReader(open('data/csstats_match_history.csv', newline='')))
MON={m:i+1 for i,m in enumerate(['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'])}
ok=bad=0
for r in raw:
    m=re.match(r'^\w+,\s*(\w+)\s+(\d+)(?:st|nd|rd|th),\s*(\d+)$', r['date'].strip())
    if not m: continue
    shown=datetime.date(2000+int(m.group(3)), MON[m.group(1)], int(m.group(2))).isoformat()
    ts=datetime.datetime.fromisoformat(r['timestamp'].replace('Z','+00:00'))
    ok,bad=(ok+1,bad) if ts.astimezone(DISPLAY_TZ).date().isoformat()==shown else (ok,bad+1)
print('agree=%d disagree=%d' % (ok,bad))
"
```

**3. Plausibility smoke test.** Mean ADR should land ~75-85 and mean HS% ~45-55. If they look swapped, they are.

**4. Teammate split sanity.** `len(split_by_teammate(flat_matches, teammate_id, teammate_matches)['with'])` should equal that teammate's `recent_matches_count` in `get_profile()['recent_teammates']` — two independent code paths agreeing that the `data_source_match_id` join worked.

## On testing

The design spec records "No test suite" as a deliberate choice for a small personal script. That decision stands by explicit preference — but it is worth being clear-eyed that both bugs above are exactly what a handful of tests would have caught cheaply:

- a transposed-columns fixture asserting `RuntimeError`
- a winter-dated row (e.g. `2026-01-15T05:30:00Z` → `2026-01-14` in Chicago, but `2026-01-15` under a fixed UTC-5) pinning DST handling
- a row with a `"yesterday"` label and a valid timestamp, asserting the timestamp wins

The verification recipes above cover the same ground manually. They only help if someone actually runs them after a re-export — which is the tradeoff being accepted here.

## Statistical caution (unrelated to the bugs, same root cause)

The retracted mouse/sens note also failed for a second, independent reason: it read 25-32 match buckets at face value with no significance testing. Under permutation testing (20k shuffles) not one of its claims cleared p<0.05, and across 110 post-change matches vs a 135-match baseline the rating difference was −0.000 (p=0.994).

Match-level CS2 data is extremely noisy. A 4-6pp win-rate gap over 25-40 matches is indistinguishable from randomness. **Always run a permutation test before reporting a difference as a finding**, and check that the map mix is comparable across the groups being compared — it is a live confound.
