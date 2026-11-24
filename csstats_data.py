import csv
import os
from datetime import datetime
from zoneinfo import ZoneInfo

from env import read_env

CSV_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data", "csstats_match_history.csv")

# csstats.gg renders its match dates in the exporting user's local timezone, while
# the exported `timestamp` column is exact ISO-8601 UTC. `date` is derived from the
# timestamp in this zone so it matches what the site displays. Set CS2_TIMEZONE in
# .env to the IANA zone your browser was in when you exported (default UTC).
#
# This MUST be a DST-aware IANA zone (e.g. America/Chicago), not a fixed offset: a
# multi-year history crosses DST boundaries. Checked 2026-07-16 against every date
# csstats displayed: the DST-aware zone matched all 887, while a fixed UTC-5 missed
# 49, every one of them in November-March. (A fixed offset looks correct if you only
# sample summer matches, which is how it slipped through earlier.)
DISPLAY_TZ = ZoneInfo(read_env("CS2_TIMEZONE", "UTC"))

# The CSV's `date` column also carries relative labels ("3 hours ago", "yesterday")
# for the newest rows. They are deliberately NOT parsed: csstats' "yesterday" is a
# rolling 24-48h bucket, not a calendar day, so it cannot be resolved to a single
# date (verified 2026-07-16 -- two matches 79 minutes apart on the same local day
# were labeled "yesterday" and "Mon, Jul 6th, 26" respectively). The `timestamp`
# column is exact and unambiguous; always prefer it. An earlier version of this
# module resolved the relative labels against datetime.now(), which silently walked
# those rows forward one day per day until they were 8 days off.


# Every column this module reads. Guarded up front so a partial export fails with
# an actionable message naming the missing column, rather than a bare KeyError from
# somewhere in the row loop.
REQUIRED_COLUMNS = frozenset({
    "timestamp", "map", "result", "score",
    "kills", "deaths", "assists", "adr", "hs_percent", "rating",
})


def _require_columns(fieldnames: list) -> None:
    missing = REQUIRED_COLUMNS - set(fieldnames or [])
    if missing:
        raise RuntimeError(
            f"{CSV_PATH} is missing required column(s): {sorted(missing)}. "
            "Re-export from csstats.gg including the exact `timestamp` column "
            "and the full stat set; see the re-export prompt in the cs2-coach skill."
        )


def load_csstats_history() -> list:
    """Parse the csstats.gg snapshot. Pure -- reads a local CSV, no network call.

    Returns one dict per match sorted oldest-first, with `timestamp` (exact, ISO
    UTC) and `date` (the local calendar date csstats displays).
    """
    matches = []
    with open(CSV_PATH, newline="") as f:
        reader = csv.DictReader(f)
        _require_columns(reader.fieldnames)
        for row in reader:
            ts = datetime.fromisoformat(row["timestamp"].replace("Z", "+00:00"))
            own_score, other_score = (int(x) for x in row["score"].split(":"))
            adr = float(row["adr"])
            hs_percent = float(row["hs_percent"])

            # The 2026-07-08 export had `adr` and `hs_percent` transposed, which went
            # unnoticed for 8 days and made every csstats ADR/HS number wrong (it
            # reported a ~80% mean headshot rate). ADR routinely exceeds 100 and a
            # headshot percentage cannot, so a transposed export is detectable here
            # rather than silently believed.
            if hs_percent > 100:
                raise RuntimeError(
                    f"{CSV_PATH}: hs_percent={hs_percent} exceeds 100 for the match at "
                    f"{row['timestamp']}, which is impossible. The `adr` and `hs_percent` "
                    "columns are most likely transposed -- verify the export against "
                    "Leetify's dpr before using it."
                )

            matches.append({
                "timestamp": ts.isoformat(),
                "date": ts.astimezone(DISPLAY_TZ).date().isoformat(),
                "map": row["map"],
                "won": {"win": True, "loss": False, "tie": None}[row["result"]],
                "score": row["score"],
                "own_score": own_score,
                "other_score": other_score,
                "kills": int(row["kills"]),
                "deaths": int(row["deaths"]),
                "assists": int(row["assists"]),
                "adr": adr,
                "hs_percent": hs_percent,
                "rating": float(row["rating"]),
            })
    matches.sort(key=lambda m: m["timestamp"])
    return matches
