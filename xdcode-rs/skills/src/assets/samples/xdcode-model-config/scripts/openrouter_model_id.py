#!/usr/bin/env python3
"""Extract an exact OpenRouter model ID locally; this does not verify capabilities."""

import argparse
import json
from urllib.parse import unquote, urlsplit


def extract_model_id(url: str) -> str:
    parsed = urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.hostname != "openrouter.ai"
        or parsed.username is not None
        or parsed.password is not None
        or parsed.port is not None
    ):
        raise ValueError("expected an HTTPS model-page URL on openrouter.ai")
    parts = unquote(parsed.path).strip("/").split("/")
    if len(parts) != 2 or any(
        not part or any(character.isspace() or ord(character) < 32 for character in part)
        for part in parts
    ):
        raise ValueError("expected a model-page path /provider/exact-model-id")
    if any(part in {".", ".."} for part in parts):
        raise ValueError("invalid model-page path")
    return "/".join(parts)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url")
    args = parser.parse_args(argv)
    try:
        model = extract_model_id(args.url)
    except ValueError as error:
        parser.error(str(error))
    print(json.dumps({"model": model, "verification": "unverified"}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
