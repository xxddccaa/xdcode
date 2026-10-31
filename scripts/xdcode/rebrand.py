#!/usr/bin/env python3
"""Mechanical upstream-to-xdcode name translation; review changes before committing.

External URLs, model identifiers, third-party code and legal notices retain
their original spelling. This is a source migration tool, never a runtime task.
"""

import argparse
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BRAND = re.compile("codex", re.IGNORECASE)
EXTERNAL = re.compile(
    r"https?://[^\s\"'<>`\\]+"
    r"|\b(?:gpt[-\w.:]*codex[-\w.:]*|codex-(?:auto-review|mini[-\w.:]*|1))\b"
    r"|x-openai-internal-codex-residency|codex/(?:encryptedContent|imageDetail)"
    r'|"codex_rollout_budget_units"'
    r"|^.*(?:Copyright|copyright|SPDX-License-Identifier|PORTABLE_METADATA).*$",
    re.MULTILINE,
)


def rename(text: str) -> str:
    def replacement(match):
        value = match.group()
        return "XDCODE" if value.isupper() else "Xdcode" if value[0].isupper() else "xdcode"

    return BRAND.sub(replacement, text)


def translate(text: str) -> str:
    parts = []
    end = 0
    for match in EXTERNAL.finditer(text):
        parts.extend((rename(text[end:match.start()]), match.group()))
        end = match.end()
    parts.append(rename(text[end:]))
    return "".join(parts).replace("@openai/xdcode", "@xxddccaa/xdcode")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--repair-wire", action="store_true", help="restore externally defined wire literals after a rename")
    args = parser.parse_args()
    tracked = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT).decode().split("\0")
    changed = []
    renamed = []
    for relative in filter(None, tracked):
        path = ROOT / relative
        if path.is_symlink() or not path.is_file():
            continue
        if relative.startswith("third_party/") or path.name.upper().startswith(("LICENSE", "NOTICE", "COPYING")):
            continue
        if path.resolve() == Path(__file__).resolve():
            continue
        raw = path.read_bytes()
        if b"\0" in raw:
            continue
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            continue
        if args.repair_wire:
            updated = text
            for before, after in (
                ("x-openai-internal-xdcode-residency", "x-openai-internal-codex-residency"),
                ("xdcode/encryptedContent", "codex/encryptedContent"),
                ("xdcode/imageDetail", "codex/imageDetail"),
                ('"xdcode_rollout_budget_units"', '"codex_rollout_budget_units"'),
            ):
                updated = updated.replace(before, after)
        else:
            updated = translate(text)
        if text != updated:
            changed.append(relative)
            if args.apply:
                path.write_text(updated, encoding="utf-8", newline="")

    # Rename leaf paths before directories, including symlinks as paths.
    if args.repair_wire:
        print(json.dumps({"modified_files": len(changed), "applied": args.apply}))
        return
    paths = set()
    for relative in filter(None, tracked):
        path = ROOT / relative
        paths.add(path)
        paths.update(parent for parent in path.parents if parent != ROOT and ROOT in parent.parents)
    for path in sorted(paths, key=lambda p: len(p.parts), reverse=True):
        if not path.exists() and not path.is_symlink():
            continue
        if path.relative_to(ROOT).parts[0] == "third_party":
            continue
        new_name = rename(path.name)
        if path.name == new_name:
            continue
        target = path.with_name(new_name)
        if target.exists() or target.is_symlink():
            raise RuntimeError(f"Rename collision: {path} -> {target}")
        renamed.append(str(path.relative_to(ROOT)))
        if args.apply:
            path.rename(target)
    print(json.dumps({"modified_files": len(changed), "renamed_paths": len(renamed), "applied": args.apply}))


if __name__ == "__main__":
    main()
