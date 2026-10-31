---
name: skill-installer
description: Install xdcode skills from a user-selected local directory or GitHub repository, or list skills from that source. Use when the user asks to install or discover skills; no remote catalog is selected by default.
metadata:
  short-description: Install skills from a source you choose
---

# Skill Installer

Use the source selected by the user. If the user supplies only a skill name, ask for its local directory or repository before any network request. If they ask what is available without specifying a source, list already installed local skills and explain how to supply a source.

## Local skills

- List installed skills without network access: `scripts/list-skills.py` (add `--format json` for structured output).
- List a selected local collection: `scripts/list-skills.py --local /path/to/skills`.
- Install a selected local skill: `scripts/install-skill-from-github.py --local /path/to/skill`.

## Selected repositories

- List a selected GitHub directory: `scripts/list-skills.py --repo owner/repository --path path/to/skills [--ref revision]`.
- Install one or more selected paths: `scripts/install-skill-from-github.py --repo owner/repository --path path/to/skill [path/to/another-skill] [--ref revision]`.
- Install from a selected URL: `scripts/install-skill-from-github.py --url https://github.com/owner/repository/tree/revision/path/to/skill`.

These repository operations access the network. Use the current environment's permissions; the skill does not require escalation for ordinary permitted requests. Existing Git credentials or optional `GITHUB_TOKEN`/`GH_TOKEN` can authenticate a private source. Never print credentials.

## Installation behavior

The destination is `$XDCODE_HOME/skills/<skill-name>`, defaulting to `~/.xdcode/skills/<skill-name>`. Use `--dest` to select another skills directory and `--name` to override a single skill's directory name. An existing destination is an error; do not overwrite it implicitly. Preserve the selected skill's license notices.

GitHub downloads are the default for selected repositories; authentication failures can fall back to Git. `--method auto|download|git` selects the transport. Local installation copies a validated skill directory without a remote request.

Report the source and installed path. Tell the user that the skill becomes available on their next turn. Bundled skills are managed by xdcode in `skills/.system`; install user-selected skills alongside that directory.
