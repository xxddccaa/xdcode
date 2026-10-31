# xdcode for Apple Silicon

This package is built natively for Apple Silicon (`aarch64-apple-darwin`).
The release is tested on macOS 15. Intel Macs are not supported by this archive.
It includes xdcode, the code-mode helper and ripgrep. Keep the complete directory
together: moving only `bin/xdcode` loses its manifest and helper programs.

Download the `macos-arm64.tar.gz` archive and its adjacent `.sha256` from your
xdcode GitHub release. In the download directory, verify the named checksum:

```sh
shasum -a 256 -c xdcode-<version>-<build-id>-macos-arm64.tar.gz.sha256
```

Replace the placeholders below with the version and build ID in the filename.
Use a new directory for each build. No Rust, Node.js or Homebrew installation is
needed on the destination Mac.

```sh
mkdir -p "$HOME/.local/lib/xdcode" "$HOME/.local/bin"
mkdir "$HOME/.local/lib/xdcode/<version>-<build-id>"
tar -xzf xdcode-<version>-<build-id>-macos-arm64.tar.gz \
  -C "$HOME/.local/lib/xdcode/<version>-<build-id>"
cd "$HOME/.local/lib/xdcode/<version>-<build-id>"
shasum -a 256 -c SHA256SUMS
./bin/xdcode --version
ln -s "$PWD/bin/xdcode" "$HOME/.local/bin/xdcode"
export PATH="$HOME/.local/bin:$PATH"
```

The `ln` command intentionally refuses an existing entry. For upgrades, finish
active xdcode tasks and stop its local daemon first (`xdcode app-server daemon
stop`). Inspect the existing link before replacing it; retain the old build and
back up runtime data separately. Add the PATH export to `~/.zshrc` if needed.

Executables have local ad-hoc signatures; this package is **not Apple Developer
ID signed or notarized**. A browser download may be quarantined by macOS. If
macOS blocks it, first verify the release source and checksums, then use macOS
Privacy & Security's Open Anyway approval. Do not disable Gatekeeper globally.

## Configuration

The default configuration and data directory is `~/.xdcode`, or an explicit
`XDCODE_HOME`. Existing Codex configuration is not imported. Create
`~/.xdcode/config.toml` from `config.example.toml` and configure your own Responses
provider and model. Supply `XDCODE_API_KEY` securely in your shell; do not put a
real key into the package, source repository, or a shared command transcript.

SQLite databases and sessions remain under the runtime home unless their paths
are explicitly configured. Installing or unpacking this archive does not alter
those files. macOS sandbox operations use the existing Seatbelt implementation;
the Linux-only bwrap helper is not part of this package.

## Rebuilding

On an Apple Silicon Mac with Xcode command-line tools, Python 3.11 and rustup:

```sh
python3.11 scripts/xdcode/macos.py
python3.11 scripts/verify_xdcode_runtime.py target/xdcode-macos-package/bin/xdcode
```

The builder requires a clean checkout, uses the repository's pinned Rust
toolchain and Cargo lockfile, and fetches checksum-pinned V8 and ripgrep build
inputs. It records source identity and binary hashes, checks ARM64 Mach-O files
and system-only dynamic dependencies, and probes each executable. The runtime
verification uses a local mock model server and no production credentials.
