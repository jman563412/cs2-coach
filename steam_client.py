import requests

from env import require_env

BASE_URL = "https://api.steampowered.com"

# GetPlayerSummaries accepts at most 100 steamids per request.
_SUMMARY_BATCH = 100

# communityvisibilitystate values returned by GetPlayerSummaries.
VISIBILITY_PRIVATE = 1
VISIBILITY_PUBLIC = 3


def load_api_key() -> str:
    return require_env("STEAM_API_KEY")


def _get(path: str, params: dict):
    api_key = load_api_key()
    response = requests.get(f"{BASE_URL}{path}", params={**params, "key": api_key})
    if response.status_code == 401:
        raise RuntimeError(
            "Steam API 401. The key does not grant access to this profile's data - "
            "for GetFriendList this almost always means the friends list is set to "
            "private. An API key does not bypass privacy settings, even on your own "
            "account."
        )
    if not response.ok:
        raise RuntimeError(f"Steam API error {response.status_code}: {response.text}")
    return response.json()


def get_friend_list(steam_id: str) -> list:
    """Return the account's friends as [{"steam64_id", "friend_since"}], newest first.

    Requires the profile's friends list to be public. Raises on 401 otherwise.
    """
    payload = _get(
        "/ISteamUser/GetFriendList/v1/",
        {"steamid": steam_id, "relationship": "friend"},
    )
    friends = payload.get("friendslist", {}).get("friends", [])
    return sorted(
        (
            {"steam64_id": f["steamid"], "friend_since": f.get("friend_since")}
            for f in friends
        ),
        key=lambda f: f["friend_since"] or 0,
        reverse=True,
    )


def get_player_summaries(steam_ids: list) -> dict:
    """Resolve SteamID64s to current Steam profile info.

    Returns {steam64_id: {"name", "profile_url", "visibility", "is_public"}}. IDs Steam
    declines to return (deleted accounts) are simply absent from the result.
    """
    summaries = {}
    for start in range(0, len(steam_ids), _SUMMARY_BATCH):
        batch = steam_ids[start : start + _SUMMARY_BATCH]
        payload = _get(
            "/ISteamUser/GetPlayerSummaries/v2/", {"steamids": ",".join(batch)}
        )
        for player in payload.get("response", {}).get("players", []):
            visibility = player.get("communityvisibilitystate")
            summaries[player["steamid"]] = {
                "name": player.get("personaname"),
                "profile_url": player.get("profileurl"),
                "visibility": visibility,
                "is_public": visibility == VISIBILITY_PUBLIC,
            }
    return summaries


def get_friends_with_names(steam_id: str) -> list:
    """get_friend_list + get_player_summaries joined into one list.

    Each entry has "steam64_id", "friend_since", "name", "profile_url", "is_public".
    Friends Steam returns no summary for keep a name of None rather than being dropped,
    since they can still be joined against Leetify by ID.
    """
    friends = get_friend_list(steam_id)
    summaries = get_player_summaries([f["steam64_id"] for f in friends])
    for friend in friends:
        summary = summaries.get(friend["steam64_id"], {})
        friend["name"] = summary.get("name")
        friend["profile_url"] = summary.get("profile_url")
        friend["is_public"] = summary.get("is_public")
    return friends
