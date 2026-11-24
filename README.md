# cs2-coach

An AI coaching setup for Counter-Strike 2. Ask an AI agent things like "how's my aim trending this month?", "am I
better on CT or T on Mirage?" or "do I win more when I queue with this friend?" and it answers from your real match
data. There's no fixed report: the agent pulls fresh numbers and does whatever analysis the question needs.

It has three parts:

- **Python data clients** (stdlib + `requests`):
  - `leetify_client.py`: your last ~100 matches from the Leetify public API, flattened into one row per match,
    with corrected ADR and a teammate split (`split_by_teammate`) that joins two players' histories
  - `csstats_data.py`: your full history from a csstats.gg CSV export, for long-range trends
  - `steam_client.py`: Steam friend list and name resolution
  - `sharecode.py`: decode and encode CS2 match sharecodes
- **`gsi-listener/`**: a small Rust program that receives CS2's Game State Integration feed while you play and writes
  each session to JSONL (a raw stream plus derived round events)
- **An agent skill** (`.claude/skills/cs2-coach/SKILL.md`): it teaches the agent how to use the clients, which
  numbers to trust, and the data pitfalls found along the way

## Setup

1. Python 3.10+ and [Poetry](https://python-poetry.org/): `poetry install`
2. `cp .env.example .env`, then fill in:
   - `LEETIFY_API_KEY` from https://leetify.com/app/developer
   - `CS2_STEAM_ID`: your 17-digit SteamID64
   - `STEAM_API_KEY` (optional, only for friend-name lookups) from https://steamcommunity.com/dev/apikey
   - `CS2_TIMEZONE` (optional): the IANA zone your csstats export was made in, e.g. `America/Chicago`
3. Check it works:
   ```bash
   poetry run python3 -c "from leetify_client import *; s=my_steam_id(); print(len(flatten_matches(s, get_match_history(s))))"
   ```

### Long-range history (optional)

csstats.gg has no API and blocks scripted access, so the long-range history is a manual CSV export made in your
browser. The skill has the exact export prompt and column list. Save the file as `data/csstats_match_history.csv`
(gitignored) and run the checks in `docs/notes/2026-07-16-data-integrity-findings.md` before trusting it.

### GSI listener (optional, Windows)

The listener is a native Windows binary, cross-compiled from WSL/Linux with the `x86_64-pc-windows-gnu` target
(`mingw-w64` provides the linker).

1. `gsi-listener/scripts/deploy_to_windows.sh /mnt/d/cs2-coach/bin` builds it and copies it, with its launch wrapper,
   to a local Windows folder.
2. Copy `gsi-listener/cfg/gamestate_integration_cs2coach.cfg` into CS2's `game/csgo/cfg/` folder.
3. Set CS2's Steam launch options to `"C:\Windows\System32\wscript.exe" "D:\cs2-coach\bin\launch_with_listener.vbs" %command%`
   (use your deploy folder). The listener starts hidden with the game.
4. Sessions are written to `GSI_OUTPUT_DIR` if set (e.g. `setx GSI_OUTPUT_DIR "D:\cs2-coach\gsi-sessions"`, then
   restart Steam), otherwise to `gsi-sessions\` next to the exe.

Tests: `cd gsi-listener && cargo test`

## Using it with an AI agent

Open the repo in your agent and ask about your stats.
- **Claude Code** picks up the skill automatically.
- **Other agents** read `AGENTS.md`, which points them at the same skill file.

To use it from any folder in Claude Code, symlink the skill:
`ln -s "$PWD/.claude/skills/cs2-coach" ~/.claude/skills/cs2-coach`.

The design notes in `docs/` explain why things work the way they do. Two examples: Leetify's ADR can be understated,
so ADR comes from csstats when available; and csstats' relative dates are unresolvable, so the loader uses exact
timestamps.

## License

MIT
