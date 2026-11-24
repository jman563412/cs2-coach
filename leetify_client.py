from datetime import datetime

import requests

from env import require_env

BASE_URL = "https://api-public.cs-prod.leetify.com"


def load_api_key() -> str:
    return require_env("LEETIFY_API_KEY")


def my_steam_id() -> str:
    """The SteamID64 whose stats to analyze, from CS2_STEAM_ID in .env."""
    return require_env("CS2_STEAM_ID")


def _get(path: str, params: dict):
    api_key = load_api_key()
    response = requests.get(
        f"{BASE_URL}{path}",
        params=params,
        headers={"_leetify_key": api_key},
    )
    if not response.ok:
        raise RuntimeError(f"Leetify API error {response.status_code}: {response.text}")
    return response.json()


def get_profile(steam_id: str) -> dict:
    return _get("/v3/profile", {"id": steam_id})


def get_match_history(steam_id: str) -> list:
    return _get("/v3/profile/matches", {"id": steam_id})


def flatten_matches(steam_id: str, matches: list) -> list:
    flattened = []
    for match in matches:
        player_stats = next(
            (s for s in match["stats"] if s["steam64_id"] == steam_id), None
        )
        if player_stats is None:
            continue

        team_number = player_stats["initial_team_number"]
        own_score = next(
            (t["score"] for t in match["team_scores"] if t["team_number"] == team_number),
            None,
        )
        other_score = next(
            (t["score"] for t in match["team_scores"] if t["team_number"] != team_number),
            None,
        )
        if own_score is None or other_score is None:
            won = None
        elif own_score > other_score:
            won = True
        elif own_score < other_score:
            won = False
        else:
            won = None

        flat = dict(player_stats)
        flat["finished_at"] = match["finished_at"]
        flat["map_name"] = match["map_name"]
        flat["data_source"] = match["data_source"]
        flat["match_id"] = match["id"]
        flat["data_source_match_id"] = match.get("data_source_match_id")
        flat["won"] = won

        # Prefer a self-computed ADR over the API's `dpr`. Leetify's `dpr` divides by
        # its own `rounds_count`, which disagreed with the match's `team_scores` on 1
        # of 100 matches sampled 2026-07-16 (2026-06-29 de_inferno: rounds_count=12,
        # but team_scores were 1 and 12 = 13 rounds), making `dpr` wrong by 6.9 there.
        # `team_scores` is the trustworthy round count -- csstats independently agrees
        # with it on that match, and its ADR matches total_damage/13 exactly. `dpr` is
        # left untouched for comparison; prefer `adr`.
        total_rounds = own_score + other_score if None not in (own_score, other_score) else None
        flat["rounds_total"] = total_rounds
        flat["adr"] = (
            player_stats["total_damage"] / total_rounds
            if total_rounds else None
        )
        flattened.append(flat)
    return flattened


def attach_csstats_adr(flat_matches: list, csstats_history: list) -> list:
    """Overlay csstats' ADR onto flattened Leetify matches, joined on exact timestamp.

    `flat_matches` is flatten_matches() output; `csstats_history` is
    csstats_data.load_csstats_history() output. Pure -- returns new dicts, no
    network call. Takes the history as an argument rather than importing it, so
    the two sources stay independent.

    Prefer this over Leetify's ADR whenever the csstats export covers the window.
    csstats' ADR tracks Valve's own figure to within integer rounding, while
    Leetify's `total_damage` is occasionally understated (verified 2026-07-13
    de_dust2: Valve 97, csstats 96, Leetify 90.57 -- ~90 damage missing). Leetify
    agrees on 96 of 98 matches, but its errors are rare-and-large where csstats'
    are bounded-and-small, so csstats is the safer default.

    Sets `adr` to the best available value and `adr_source` to "csstats" or
    "leetify". Matches outside the csstats snapshot (played after the export)
    keep Leetify's computed `adr` and are marked "leetify".
    """
    by_timestamp = {m["timestamp"]: m for m in csstats_history}

    enriched = []
    for flat in flat_matches:
        out = dict(flat)
        key = (
            datetime.fromisoformat(flat["finished_at"].replace("Z", "+00:00"))
            .isoformat()
        )
        match = by_timestamp.get(key)
        if match is None:
            out["adr_source"] = "leetify"
            enriched.append(out)
            continue

        # The join is on timestamp alone, so verify the row really is this match
        # before believing its ADR. Both sources carry map/kills/deaths, and all 98
        # overlapping matches agree on them, so a disagreement means the join is
        # wrong -- not something to paper over. Matching on a fingerprint instead of
        # an identity has already produced false pairings here (a Feb-2026 csstats
        # row "matching" a Jul-2026 Leetify match by map+K/D+score).
        mismatched = [
            f"{name}: leetify={lhs!r} csstats={rhs!r}"
            for name, lhs, rhs in (
                ("map", flat["map_name"], match["map"]),
                ("kills", flat["total_kills"], match["kills"]),
                ("deaths", flat["total_deaths"], match["deaths"]),
            )
            if lhs != rhs
        ]
        if mismatched:
            raise RuntimeError(
                f"csstats row at {key} does not describe the same match as Leetify's "
                f"({', '.join(mismatched)}). The timestamp join is unsound -- do not "
                "trust the ADR overlay until this is understood."
            )

        out["adr"] = match["adr"]
        out["adr_source"] = "csstats"
        enriched.append(out)
    return enriched


def split_by_teammate(
    flat_matches: list, teammate_id: str, teammate_matches: list
) -> dict:
    """Split your matches by whether a given teammate played alongside you.

    `flat_matches` is flatten_matches() output -- pass it through
    attach_csstats_adr() first if you intend to look at ADR, otherwise the buckets
    carry Leetify's less accurate value. `teammate_matches` is raw
    get_match_history output for the teammate (their own stat blocks are needed).

    A match's own `id` differs between the two players' histories, so the join is
    on `data_source_match_id` (the "CSGO-XXXXX-..." code), which is shared.

    Returns a dict of:
      "with"    - flattened matches where they were on your team
      "against" - flattened matches where they were on the opposing team
      "without" - flattened matches where they were absent
      "covers_window" - whether "without" can be trusted (see below)

    Being in the same match does NOT mean being on the same team, so team
    membership is checked via initial_team_number rather than assumed.

    "without" is only meaningful when covers_window is True. Each history is
    capped at ~100 matches; if the teammate's oldest match is newer than yours,
    their earlier games simply aren't in the response, and games you really did
    play together get misfiled as "without".
    """
    teammate_by_key = {
        m["data_source_match_id"]: m
        for m in teammate_matches
        if m.get("data_source_match_id")
    }

    result = {"with": [], "against": [], "without": [], "covers_window": False}

    for flat in flat_matches:
        their_match = teammate_by_key.get(flat["data_source_match_id"])
        if their_match is None:
            result["without"].append(flat)
            continue

        their_stats = next(
            (s for s in their_match["stats"] if s["steam64_id"] == teammate_id), None
        )
        if their_stats is None:
            # Unreachable when teammate_id is right: a match response carries exactly
            # one stat block, its owner's, and we only got here because that match
            # came out of *their* history. So this means teammate_id is wrong (a
            # common cause: passing an int, or an ID from a different source). Filing
            # these as "without" would silently return with=0/without=everything --
            # indistinguishable from "you never played together", which reads as a
            # real finding rather than a bug.
            raise RuntimeError(
                f"teammate_id {teammate_id!r} matches no stat block in the teammate's "
                f"own match {their_match.get('data_source_match_id')!r} (that block "
                f"belongs to {[s['steam64_id'] for s in their_match['stats']]}). "
                "Pass the SteamID64 as a string, exactly as Leetify reports it."
            )

        if their_stats["initial_team_number"] == flat["initial_team_number"]:
            result["with"].append(flat)
        else:
            result["against"].append(flat)

    if flat_matches and teammate_matches:
        my_oldest = min(m["finished_at"] for m in flat_matches)
        their_oldest = min(m["finished_at"] for m in teammate_matches)
        result["covers_window"] = their_oldest <= my_oldest

    return result
