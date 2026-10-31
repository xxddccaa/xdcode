#!/usr/bin/env python3
"""Translate implementation names without rewriting third-party wire contracts.

This developer migration is never invoked by the installed application.
License notices, URLs, service hosts, model IDs, HTTP headers and MCP metadata
retain their meaning. Every other change is reviewed and compiled normally.
"""

import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SPECIAL = {
    "openai_models": "model_types",
    "requires_openai_auth": "requires_hosted_auth",
    "requiresOpenaiAuth": "requiresHostedAuth",
    "requiresOpenAIAuth": "requiresHostedAuth",
    "create_openai_provider": "create_custom_provider",
    "is_openai": "is_hosted_provider",
    "openai.yaml": "xdcode.yaml",
    "openai_yaml": "xdcode_yaml",
    "com.openai.xdcode": "io.xdcode",
}
PROTECTED = re.compile(
    r"(?:https?|ssh|openai)://[^\s\"'<>`\\]+"
    r"|(?:[\w-]+\.)*openai\.(?:com|org)\b"
    r"|openai\.gpt[-\w.:]*"
    r"|openai\.azure\.|windows\.net/openai"
    r"|(?:x-[\w-]*openai[\w-]*|OpenAI-(?:Beta|Model|Project|Organization|Processing-Ms|Version))"
    r'|openai/[\w-]+|(?-i:"openaiForm")'
    r"|^.*(?:Copyright|SPDX-License-Identifier|PORTABLE_METADATA).*$",
    re.IGNORECASE | re.MULTILINE,
)
NAME = re.compile("openai", re.IGNORECASE)


def rename(text):
    for old, new in SPECIAL.items():
        text = text.replace(old, new)
    return NAME.sub(lambda m: "PROVIDER" if m[0].isupper() else "Provider" if m[0][0].isupper() else "provider", text).replace("Provider Xdcode", "xdcode")


def translate(text):
    result = []
    offset = 0
    for match in PROTECTED.finditer(text):
        result.extend((rename(text[offset:match.start()]), match[0]))
        offset = match.end()
    result.append(rename(text[offset:]))
    return "".join(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    files = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT).decode().split("\0")
    files = [p for p in files if p.startswith("xdcode-rs/") and not p.startswith("xdcode-rs/vendor/")]
    modified = 0
    for relative in files:
        path = ROOT / relative
        if path.is_symlink() or not path.is_file() or path.name.upper().startswith(("LICENSE", "NOTICE", "COPYING")):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        updated = translate(text)
        if updated != text:
            modified += 1
            if args.apply:
                path.write_text(updated, encoding="utf-8", newline="")
    # Derive candidate paths from tracked inputs, never descend build caches.
    candidates = set()
    for relative in files:
        path = ROOT / relative
        candidates.add(path)
        candidates.update(parent for parent in path.parents if parent != ROOT and ROOT in parent.parents)
    renamed = []
    for path in sorted(candidates, key=lambda p: len(p.parts), reverse=True):
        target = path.with_name(rename(path.name))
        if target == path or not path.exists():
            continue
        if target.exists():
            raise RuntimeError(f"Rename collision: {path} -> {target}")
        renamed.append(str(path.relative_to(ROOT)))
        if args.apply:
            path.rename(target)
    print(json.dumps({"modified_files": modified, "renamed_paths": renamed, "applied": args.apply}))


if __name__ == "__main__":
    main()
