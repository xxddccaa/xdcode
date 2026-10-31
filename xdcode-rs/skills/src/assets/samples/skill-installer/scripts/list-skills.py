#!/usr/bin/env python3
"""List installed/local skills, or a user-selected GitHub repository path."""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error

from github_utils import github_api_contents_url, github_request

DEFAULT_REF = "main"


class ListError(Exception):
    pass


class Args(argparse.Namespace):
    repo: str | None
    path: str | None
    local: str | None
    ref: str
    format: str


def _request(url: str) -> bytes:
    return github_request(url, "xdcode-skill-list")


def _xdcode_home() -> str:
    return os.environ.get("XDCODE_HOME", os.path.expanduser("~/.xdcode"))


def _installed_skills() -> set[str]:
    return set(_local_skills(os.path.join(_xdcode_home(), "skills")))


def _local_skills(root: str) -> list[str]:
    if not os.path.isdir(root):
        return []
    entries = set()
    for name in os.listdir(root):
        path = os.path.join(root, name)
        if os.path.isfile(os.path.join(path, "SKILL.md")):
            entries.add(name)
        elif name == ".system":
            entries.update(_local_skills(path))
    return sorted(entries)


def _list_skills(repo: str, path: str, ref: str) -> list[str]:
    api_url = github_api_contents_url(repo, path, ref)
    try:
        payload = _request(api_url)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            raise ListError(
                f"Skills path not found: https://github.com/{repo}/tree/{ref}/{path}"
            ) from exc
        raise ListError(f"Failed to fetch skills: HTTP {exc.code}") from exc
    data = json.loads(payload.decode("utf-8"))
    if not isinstance(data, list):
        raise ListError("Unexpected skills listing response.")
    skills = [item["name"] for item in data if item.get("type") == "dir"]
    return sorted(skills)


def _parse_args(argv: list[str]) -> Args:
    parser = argparse.ArgumentParser(description="List local skills or an explicitly selected repository.")
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--repo", help="User-selected GitHub owner/repo; requires --path")
    source.add_argument("--local", help="Local skills directory (default: $XDCODE_HOME/skills)")
    parser.add_argument(
        "--path",
        help="Explicit repository directory to list; requires --repo",
    )
    parser.add_argument("--ref", default=DEFAULT_REF)
    parser.add_argument(
        "--format",
        choices=["text", "json"],
        default="text",
        help="Output format",
    )
    args = parser.parse_args(argv, namespace=Args())
    if bool(args.repo) != bool(args.path):
        parser.error("Remote listing requires both --repo and --path.")
    return args


def main(argv: list[str]) -> int:
    args = _parse_args(argv)
    try:
        skills = (
            _list_skills(args.repo, args.path, args.ref)
            if args.repo
            else _local_skills(args.local or os.path.join(_xdcode_home(), "skills"))
        )
        installed = _installed_skills()
        if args.format == "json":
            payload = [
                {"name": name, "installed": name in installed} for name in skills
            ]
            print(json.dumps(payload))
        else:
            for idx, name in enumerate(skills, start=1):
                suffix = " (already installed)" if name in installed else ""
                print(f"{idx}. {name}{suffix}")
        return 0
    except ListError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
