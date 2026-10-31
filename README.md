# xdcode

xdcode is an independently maintained personal coding agent for Linux x86_64 and
macOS Apple Silicon.
Upstream attribution and licensing are
preserved in [LICENSE](LICENSE) and [NOTICE](NOTICE); both travel with each package,
along with the derivative change notices in [MODIFICATIONS.md](MODIFICATIONS.md).

The product version follows the baseline upstream stable release recorded in
[upstream.json](upstream.json). The current baseline is **0.165.0**
(`rust-v0.165.0`), with upstream source commit
`360319d8dee62d87e2d59fc0c21805d79519cfed`. A separate build ID identifies personal
revisions. Release tags can live on upstream release branches, so `git describe`
on the source snapshot is not the product version authority.

See [the baseline migration record](UPSTREAM_0.165.0.md) and
[the selective backports](MODIFICATIONS.md#2026-10-31-selective-backports) for
adopted changes, preserved behavior and validation scope. The current personal
revision adds provider-grouped model selection, bounded model delegation, richer
transcript and image interaction, queue resumption and focused network diagnostics.
It retains reliable Plan-mode transitions and temporary rate-limit recovery.
Source upgrades take effect after building and installing a new package.

xdcode uses its own `xdcode` command, `XDCODE_*` environment variables and
`~/.xdcode` runtime directory. Configure a user-owned Responses provider using
`XDCODE_API_KEY`. Runtime does not depend on another CLI installation,
hosted account login, or a hosted binary download service.
Local tools, approvals, sandboxing, MCP, skills, threads and the local app-server
are part of the derivative's delivery contract.

## Build and package

Use Python 3.11 or newer (on the development host:
`/root/.local/bin/python3.11`). Build prerequisites and the V8 dependency setup
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
produces `target/xdcode-dist/xdcode-0.165.0-personal1-linux-x86_64.tar.gz`, its
`.sha256` sidecar, and a matching `.install.py` offline installer. A development
snapshot requires the explicit `--allow-dirty` option and records its content
digest. No product version override is accepted.

## Offline installation

Transfer the archive, checksum and installer together from a trusted build:

```bash
python3.11 xdcode-0.165.0-personal1-linux-x86_64.install.py \
  xdcode-0.165.0-personal1-linux-x86_64.tar.gz
```

The installer checks integrity and installs into
`~/.local/lib/xdcode/0.165.0-personal1`, then points `~/.local/bin/xdcode` at that
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
