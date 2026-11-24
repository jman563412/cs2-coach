"""Read settings from the repo's .env file (see .env.example), falling back to the process environment."""
import os

ENV_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".env")


def _parse(path: str) -> dict:
    values = {}
    if not os.path.exists(path):
        return values
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, value = line.split("=", 1)
            value = value.strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            values[key.strip()] = value
    return values


def read_env(name: str, default: str | None = None) -> str | None:
    value = _parse(ENV_PATH).get(name) or os.environ.get(name)
    return value if value else default


def require_env(name: str) -> str:
    value = read_env(name)
    if value is None:
        raise RuntimeError(f"{name} not set; add it to .env (see .env.example)")
    return value
