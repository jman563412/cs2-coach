"""Decode CS2 match sharing codes into the identifiers the Game Coordinator wants.

A sharecode (e.g. "CSGO-caMKV-r664Q-ubqq5-XOEhk-OijDA", which is match 1, outcome 2,
token 3) is a base-57 encoding of an 18-byte payload: match_id, outcome_id, and a
16-bit token. Leetify's
`data_source_match_id` is exactly this string, so the codes already in a match
history can be decoded without asking Steam for anything.
"""

DICTIONARY = "ABCDEFGHJKLMNOPQRSTUVWXYZabcdefhijkmnopqrstuvwxyz23456789"
_BASE = len(DICTIONARY)

# The decoded payload is match_id (u64) + outcome_id (u64) + token (u16).
_PAYLOAD_BYTES = 18
_CODE_CHARS = 25


def _strip(code: str) -> str:
    cleaned = code.strip().upper()
    if cleaned.startswith("CSGO-"):
        code = code.strip()[len("CSGO-") :]
    return code.replace("-", "").strip()


def decode_sharecode(code: str) -> dict:
    """Return {"match_id", "outcome_id", "token"} for a sharecode.

    Raises ValueError on anything that is not a well-formed code, rather than
    returning nonsense identifiers that would fail confusingly later.

    Verification status (2026-08-05): `match_id` is confirmed. Decoding all 100
    matches in the Leetify window yields a strictly increasing sequence, and the
    three demos on disk land in the correct chronological slots relative to it
    (the two June files fall below the window's first match, the July file lands
    on 2026-07-13). `outcome_id` and `token` are NOT yet confirmed - the second
    field of a demo filename does not equal the decoded `outcome_id`, which may
    mean the filename carries a different identifier or that the 8/8/2 split is
    wrong. Confirm with a live `request_full_match_info` before trusting them;
    a wrong split fails loudly there rather than silently.
    """
    body = _strip(code)
    if len(body) != _CODE_CHARS:
        raise ValueError(
            f"sharecode must hold {_CODE_CHARS} characters, got {len(body)}: {code!r}"
        )
    bad = sorted(set(body) - set(DICTIONARY))
    if bad:
        raise ValueError(f"sharecode has characters outside the alphabet: {bad}")

    # The encoder consumes the payload least-significant-digit first, so decoding
    # walks the characters in reverse.
    value = 0
    for char in reversed(body):
        value = value * _BASE + DICTIONARY.index(char)

    payload = value.to_bytes(_PAYLOAD_BYTES, "big")
    return {
        "match_id": int.from_bytes(payload[0:8], "little"),
        "outcome_id": int.from_bytes(payload[8:16], "little"),
        "token": int.from_bytes(payload[16:18], "little"),
    }


def encode_sharecode(match_id: int, outcome_id: int, token: int) -> str:
    """Inverse of decode_sharecode. Exists so the decoder can be round-trip tested."""
    payload = (
        match_id.to_bytes(8, "little")
        + outcome_id.to_bytes(8, "little")
        + token.to_bytes(2, "little")
    )
    value = int.from_bytes(payload, "big")

    chars = []
    for _ in range(_CODE_CHARS):
        value, remainder = divmod(value, _BASE)
        chars.append(DICTIONARY[remainder])

    body = "".join(chars)
    groups = [body[i : i + 5] for i in range(0, _CODE_CHARS, 5)]
    return "CSGO-" + "-".join(groups)
