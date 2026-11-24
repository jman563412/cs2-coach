# Agent instructions

This repo answers questions about one player's Counter-Strike 2 performance.

- For any question about the user's CS2 stats, trends, maps, aim/utility ratings, or teammates, follow
  `.claude/skills/cs2-coach/SKILL.md`. It explains how to fetch the data, which numbers to trust, and the pitfalls.
- Run Python through Poetry from the repo root: `poetry run python3 ...`.
- The player is `CS2_STEAM_ID` in `.env` (`leetify_client.my_steam_id()`). Never hardcode a SteamID.
- Never print, log or echo API keys or other `.env` values. Saying whether one is set is fine.
- The Rust GSI listener is in `gsi-listener/` (`cargo test` there).
