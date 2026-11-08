# xdcode

xdcode is an independently maintained personal coding agent for Linux x86_64 and
macOS Apple Silicon.
Upstream attribution and licensing are
preserved in [LICENSE](LICENSE) and [NOTICE](NOTICE); both travel with each package,
along with the derivative change notices in [MODIFICATIONS.md](MODIFICATIONS.md).

The product version is **xdcode 0.167.0**. The actual upstream baseline recorded in
[upstream.json](upstream.json) remains **0.165.0**
(`rust-v0.165.0`), with upstream source commit
`360319d8dee62d87e2d59fc0c21805d79519cfed`. A separate build ID identifies personal
revisions. Release tags can live on upstream release branches, so `git describe`
on the source snapshot is not the product version authority.

This is a selective maintenance release, not a full synchronization with Codex
0.167.0. `version` identifies the product; `upstream_version` identifies the baseline.
See [the 0.167.0 maintenance record](UPSTREAM_0.167.0.md),
[the baseline migration record](UPSTREAM_0.165.0.md) and
[the selective backports](MODIFICATIONS.md#2026-10-31-selective-backports) for
adopted changes, preserved behavior and validation scope. The current personal
revision adds provider-grouped model selection, bounded model delegation, richer
transcript and image interaction, queue resumption and focused network diagnostics.
It also preserves pending agent messages during compaction, improves paginated
history and SQLite reliability, and fixes input and file-search edge cases.
`/new` inherits the current confirmed provider, exact catalog entry, model,
reasoning effort and mode only after the new session acknowledges them. `/model`
opens at the first selectable entry. Plan-mode transitions and temporary rate-limit
recovery remain intact.
The fixed-main development pass selectively adopts applicable repairs and opt-in
local features from Codex `c9b4267c370a755da12ec1c8648c2451163212b7`.
See [its source ledger, experimental entry points and verification limits](UPSTREAM_MAIN_C9B4267C.md).
Source upgrades take effect after building and installing a new package.

xdcode uses its own `xdcode` command, `XDCODE_*` environment variables and
`~/.xdcode` runtime directory. Configure a user-owned Responses provider using
`XDCODE_API_KEY`. Runtime does not depend on another CLI installation,
hosted account login, or a hosted binary download service.
Local tools, approvals, sandboxing, MCP, skills, threads and the local app-server
are part of the derivative's delivery contract.

## Build and package

Use Python 3.11 or newer. Build prerequisites and the V8 dependency setup
are described in [docs/xdcode.md](docs/xdcode.md).

```bash
xdcode_upstream_commit="$(python3.11 -c 'import json; print(json.load(open("upstream.json"))["upstream_commit"])')"
if ! git cat-file -e "$xdcode_upstream_commit^{commit}" 2>/dev/null; then
  git fetch --no-tags --depth=1 https://github.com/openai/codex.git "$xdcode_upstream_commit"
fi
python3.11 scripts/xdcode/build.py --build-id personal1
python3.11 scripts/xdcode/package.py --build-dir target/xdcode-builds/personal1
```

This builds `x86_64-unknown-linux-gnu`, uses `target/` and `.build-cache/`, and
produces `target/xdcode-dist/xdcode-0.167.0-personal1-linux-x86_64.tar.gz`, its
`.sha256` sidecar, and a matching `.install.py` offline installer. A development
snapshot requires the explicit `--allow-dirty` option and records its content
digest. No product version override is accepted.

## Install from GitHub (Linux x86_64)

After release `v0.167.0-ge8036a95a605` is published, install it with one command.
Requires Bash, `curl`, Python 3.11 or newer available as `python3`, and a local
proxy listening at `http://127.0.0.1:10826`:

```bash
bash -c 'set -e; package=xdcode-0.167.0-ge8036a95a605-linux-x86_64; cd "$(mktemp -d)"; for suffix in tar.gz tar.gz.sha256 install.py; do curl -fL -x http://127.0.0.1:10826 -O "https://github.com/xxddccaa/xdcode/releases/download/v0.167.0-ge8036a95a605/$package.$suffix"; done; python3 "$package.install.py" --force "$package.tar.gz"'
```

This command is not for macOS. `--force` skips only active-process checks;
integrity verification and install locking remain enabled. Old builds and runtime
data are retained. Existing sessions keep using their old build; restart the
local daemon safely after active tasks finish to use the new build.

## Offline installation

Transfer the archive, checksum and installer together from a trusted build:

```bash
python3.11 xdcode-0.167.0-personal1-linux-x86_64.install.py \
  xdcode-0.167.0-personal1-linux-x86_64.tar.gz
```

The installer checks integrity and installs into
`~/.local/lib/xdcode/0.167.0-personal1`, then points `~/.local/bin/xdcode` at that
build. Put `~/.local/bin` on your PATH. It does not download anything, replace
other CLI installations, or write your provider configuration. Finish active xdcode tasks
and stop its local daemon before installing; keep xdcode stopped until the
installer exits. Old builds and runtime data are retained. Keeping an older
binary does not guarantee that it can read data written by a newer build.

## Configure your provider

Use [config.example.toml](scripts/xdcode/config.example.toml) as the starting point
for your own `~/.xdcode/config.toml`. Its `https://example.invalid/v1` URL is a
placeholder; replace it with your Responses provider's base URL. Provide
`XDCODE_API_KEY` through your shell or secret manager. The example model is `gpt-5.6-sol`; change it to a model your
provider serves. Never put a real key into this repository or package.

[The delivery guide](docs/xdcode.md) covers explicit test paths, checksums,
build provenance, GNU libc requirements, local daemon upgrades, and the manual
upstream transplant workflow. Use that guide and the [local configuration
guide](docs/config.md) for supported xdcode setup and operation.

## Instructions for future AI-assisted upgrades

Read [UPSTREAM_UPGRADE.md](UPSTREAM_UPGRADE.md) before comparing upstream releases
or transplanting features. It defines required inputs, application invariants,
change selection, dependency analysis, identity checks, tests, and delivery.
[AGENTS.md](AGENTS.md) makes this workflow discoverable to future coding agents.
