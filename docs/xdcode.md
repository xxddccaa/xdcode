# xdcode delivery guide

xdcode is an independently maintained personal coding agent. This guide covers
Linux x86_64 with GNU libc, built from the selected source snapshot. Native
Apple Silicon packages use the [macOS delivery guide](xdcode-macos.md).
The command, interface, default instructions, realtime instructions and bundled
self-description must consistently identify the application as xdcode. Changing
the configured model or provider does not change that application identity.

xdcode uses `XDCODE_*` environment variables, `~/.xdcode` for user state and
`.xdcode` for project configuration. Use an explicit `XDCODE_HOME` to isolate a
runtime. Model requests go to the provider you configure; setup uses your
provider's credentials. Source provenance and attribution are recorded in
[NOTICE](../NOTICE), [MODIFICATIONS.md](../MODIFICATIONS.md) and the source
repository's `upstream.json`. The [LICENSE](../LICENSE) and notices travel with
each package.

## Version and source identity

The source repository's `upstream.json` is the authority for the product version
and baseline. Installed packages record that metadata in `build-info.json` under
`upstream`:

- Product version: `0.167.0`; actual upstream baseline: `0.165.0`, tag `rust-v0.165.0`.
- Selected upstream source: `360319d8dee62d87e2d59fc0c21805d79519cfed`.
- Release branch base: `b559e6a3988489b636e1e79672eff330bf0679dc`.

The 0.167.0 maintenance revision preserves the [unified model catalog](model-configuration.md),
provider switching, project preferences and the bundled configuration skill.
It fixes Plan-to-implementation transitions and retries temporary rate limits
until recovery or cancellation. The build ID distinguishes each release. See
[the selective maintenance record](../UPSTREAM_0.167.0.md) and
[the baseline upgrade record](../UPSTREAM_0.165.0.md) for scope and validation, and each
build's receipt for its actual source and artifact identity.

Stable tags can be on a release branch; a nearest-ancestor `git describe` on
main can return an unrelated old version. Update the metadata deliberately when
transplanting an upstream baseline. `version` identifies the xdcode product and
must agree with Cargo and CLI manifests. `upstream_version` identifies the real
upstream baseline and must agree with `upstream_tag`; old metadata without this
field falls back to `version`. Selective backports do not imply a full upstream
upgrade, even when the product version advances.

A build ID distinguishes personal revisions, for example `personal1` or
`g<12-character-source-commit>`. IDs accept letters, digits, dots, underscores
and hyphens, start with a letter or digit, and are at most 80 characters. Use a
fresh ID for changed binaries. By default the build ID comes from the personal
source commit, with a source-content suffix for explicitly allowed dirty builds.
`build-info.json` records the full personal source commit, a digest of the source
files, the dirty flag, baseline metadata, target, Rust toolchain, GNU libc
requirement, and hashes of all four executable files and MODIFICATIONS.md. The source digest stores
no source contents, environment variables, configuration, or keys.

The product version is stored in `xdcode-package.json`. Runtime `BuildInfo`
reads it from the installed package; an unbundled source binary is a development
build. Keep the complete package layout together. The packager executes the
staged CLI's `--version` with a temporary `XDCODE_HOME` and rejects an output
unless stripped stdout is exactly `xdcode <version>`; prerelease suffixes, build
metadata and extra output are rejected. It does not substitute the build ID
into the product version. The package's personal build ID is in the manifest's
`buildId` and in `build-info.json`; the runtime may expose its own commit-derived
opaque build identity separately.

## Prerequisites and dependency downloads

Use Python **3.11+**, Git, Rust as pinned by `xdcode-rs/rust-toolchain.toml`, a
native C/C++ build toolchain, CMake, clang/libclang, pkg-config, GNU binutils and
the libcap and OpenSSL development packages. For Debian/Ubuntu, prepare a build
machine with:

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential clang libclang-dev cmake pkg-config binutils libcap-dev libssl-dev
rustup target add --toolchain 1.99.1 x86_64-unknown-linux-gnu
python3.11 --version
```

The script runs Cargo inside `xdcode-rs` and explicitly selects the pinned
`RUSTUP_TOOLCHAIN`, including when the caller's shell has a `stable` override.
It builds with `--locked --release`. The first build may download Cargo source
dependencies, the checksum-pinned upstream V8 archive/bindings, and checksum-
pinned ripgrep. These are build dependencies; the delivered installer does not
fetch an application binary. V8 setup reuses
`scripts/xdcode_package/v8.py` and `third_party/v8` checksum pins. To supply your
own verified V8 dependencies, set both `RUSTY_V8_ARCHIVE` and
`RUSTY_V8_SRC_BINDING_PATH` together. `V8_FROM_SOURCE=1` opts into the upstream
source-build route and its additional prerequisites. An explicit V8 pair is
trusted as a build input, so verify those files before setting overrides.

The default target directory is repository-root `target/`; V8 and ripgrep caches
are under `.build-cache/`. Both are gitignored. `--target-dir` and `--cache-dir`
can point to another filesystem with sufficient capacity. On the development
host use the cpfs mount: the system `/tmp` mount is too small for V8 and Cargo.
The ripgrep subprocess receives a task-specific `TMPDIR` under the cache. No
scratch path is assigned to a user's home or application-home variable.

GNU builds depend on the build machine's runtime libraries. `requires_glibc`
conservatively records the builder's GNU libc version, and the installer refuses
an older GNU libc. Build on the oldest supported distribution. The installer
checks platform, ELF class/architecture and glibc, then loads all four staged
executables before activation. The target needs `libcap.so.2` and `libgcc_s.so.1`
as well as GNU libc and the ELF loader (Debian/Ubuntu packages `libcap2` and
`libgcc-s1`). The minimum host is Linux x86_64 with GNU libc at least the
receipt's `requires_glibc`; the default Ubuntu 22.04 CI build requires glibc
2.35 or newer. Check the release receipt when using a package from another builder.
These startup probes do not exercise sandbox operations or optional libraries
loaded later. Check `ldd` and run the real package on intended deployment
distributions before distributing it.

## Build and package commands

From a stable checkout with resolved `upstream.json`, fetch the pinned upstream
commit for build verification. This also supports source snapshots whose Git
history starts with the personal project; the fetch creates no branch or tag.

```bash
xdcode_upstream_commit="$(python3.11 -c 'import json; print(json.load(open("upstream.json"))["upstream_commit"])')"
if ! git cat-file -e "$xdcode_upstream_commit^{commit}" 2>/dev/null; then
  git fetch --no-tags --depth=1 https://github.com/openai/codex.git "$xdcode_upstream_commit"
fi
python3.11 scripts/xdcode/build.py --build-id personal1
python3.11 scripts/xdcode/package.py \
  --build-dir target/xdcode-builds/personal1 \
  --output-dir target/xdcode-dist
```

Release delivery must use a clean, reviewed source commit. For a working-tree
integration build, add `--allow-dirty`; the build records that fact and a content
digest, and the result remains a development artifact. Avoid editing source
while building: the script compares source identity before and after compilation
and refuses a mixed snapshot. Build outputs/caches must stay in ignored
directories or outside the source tree. `--output-dir` on the build command
selects a fresh receipt/binary
directory. `--rg-bin /absolute/path/to/rg` uses an existing local ripgrep. The
build and packager never overwrite an existing output.

The build sequence is deliberate:

1. Build `bwrap` for `x86_64-unknown-linux-gnu`, strip its final bytes, and require
   a successful bounded `--version` probe identifying the vendored build.
2. Hash those bytes and set `XDCODE_BWRAP_SHA256` for the subsequent build.
3. Build `xdcode` and `xdcode-code-mode-host` with the digest and
   `STABLE_GIT_COMMIT`. Do not rebuild or strip bwrap after this step.
4. Copy the artifacts, strip only the copies of the two first-party binaries,
   and write the build receipt with all hashes.
5. Package only matching source/receipt/binary inputs using the existing canonical
   `scripts/xdcode_package` layout helpers. No Cargo command or download occurs
   during packaging. Check the staged CLI version, then write checksums and a
   deterministic tar/gzip serialization. Packaging must use the same source
   snapshot as the build; changing installer/docs/source after compilation
   requires returning to that snapshot or rebuilding.

Release builds reject `XDCODE_SKIP_BWRAP_BUILD` whenever it is present, even
empty or `0`, and reject external `XDCODE_BWRAP_SOURCE_DIR` overrides. They pin
the vendored directory explicitly; its path is in the receipt and its contents
are covered by the source digest. The bwrap build script tracks the full resolved
source directory recursively, including `utils.h`, `network.h`, `bind-mount.h`
and new headers, so incremental builds cannot retain stale header code.

The archive is `xdcode-<version>-<buildid>-linux-x86_64.tar.gz`, with this layout:

```text
xdcode-package.json
build-info.json
SHA256SUMS
LICENSE
NOTICE
MODIFICATIONS.md
install.py
config.example.toml
docs/xdcode.md
bin/xdcode
bin/xdcode-code-mode-host
xdcode-resources/bwrap
xdcode-path/rg
```

The archive has an adjacent `.tar.gz.sha256` checksum and a standalone installer
named `xdcode-<version>-<buildid>-linux-x86_64.install.py`. Neither the build
receipt nor checksums are signatures. Obtain the installer and checksum through
a trusted channel; a checksum alone does not authenticate the publisher.

## Install offline and upgrade manually

By default, finish active tasks, stop xdcode's local daemon using its supported local daemon
control, and close any other xdcode process. Keep it stopped during installation.
The installer refuses any same-user native `xdcode`, `xdcode-app-server`, or
`xdcode-code-mode-host` process, plus live daemon PID records in the selected
`XDCODE_HOME`. It checks again before activation and does not send signals.
Without `--force`, a malformed PID record is a refusal, not a reason to
erase daemon state automatically. Both `daemon.pid`/`daemon-updater.pid` and
legacy `app-server.pid`/`app-server-updater.pid` are checked. Native Linux
`processIdentity` (or legacy `linuxProcessIdentity`) records are matched against
the boot ID and `/proc` start ticks. Previous boots, reused PIDs, missing
processes and zombies are provably stale and do not block; records are retained.
Unreadable/ambiguous identities fail closed. A legacy wall-clock timestamp alone
cannot prove an extant non-zombie PID stale. The check is an observation of local Linux
processes; it cannot prevent an unrelated launcher from starting a process
concurrently. It does not coordinate other machines sharing a filesystem.

Explicitly pass `--force` to install while existing xdcode processes keep running.
This skips only active-process and daemon PID checks. Archive and payload checksums,
path safety, the installation lock, runtime probes and refusal to overwrite an
existing build all remain enforced. The new build is installed separately and the
entrypoint is switched atomically; old builds, configuration and runtime data are
retained. Running processes keep using their current builds and are not restarted
or upgraded in place. Finish their tasks and restart them safely to use the new
build. A new CLI and an old daemon may have incompatible protocols; `--force`
does not resolve protocol or runtime-data compatibility.

Transfer the three output files to the target machine and run:

```bash
python3.11 xdcode-0.167.0-personal1-linux-x86_64.install.py \
  xdcode-0.167.0-personal1-linux-x86_64.tar.gz
```

To allow existing sessions to continue running during installation:

```bash
python3.11 xdcode-0.167.0-personal1-linux-x86_64.install.py --force \
  xdcode-0.167.0-personal1-linux-x86_64.tar.gz
```

Alternatively use the trusted repository's `scripts/xdcode/install.py`. Supply
`--sha256 <trusted-64-character-digest>` if you do not use the adjacent sidecar.
No network access or source checkout is needed by the standalone installer.

Defaults are `~/.local/lib/xdcode/<version>-<buildid>` and the symlink
`~/.local/bin/xdcode`. Only a recognized xdcode symlink within that same install
root can be updated. An existing ordinary file, a symlink into a different
installation, a duplicate build ID, system command directories, reserved paths
for other applications, symlinked parent paths and overlaps with runtime data
are refused. Hidden application directories are also rejected; `.local` is the
allowed hidden installation prefix. Custom install roots must end in `xdcode`,
custom entry directories in `bin`; both must be owned
by the current user and not writable by other users/groups. The installer holds
a local install lock, validates archive names, file types, permissions and all
checksums, and stages in the chosen install filesystem. Before activation it
executes `xdcode --version`, `xdcode-code-mode-host --help`, `rg --version` and
`bwrap --version`. Each probe has a ten-second timeout and a 64 KiB stdout
limit, discards stderr, and uses a temporary runtime home and a minimal
environment without keys, proxy settings or loader overrides. No user's
configuration/data or credential logs are written. A missing runtime library,
bad exit status, timeout or wrong version preserves the current entrypoint.
Activation uses an
atomic symlink replacement within the bin filesystem, including when bin and
builds are on different filesystems.

Old builds are kept, as is all runtime data. A late activation failure may leave
the newly verified build directory in place while preserving the old entrypoint.
There is no automatic pruning, uninstall/data removal, or rollback guarantee.
An older binary may not understand newer database/config formats. Handle data
backups and any manual version change separately after checking compatibility.
There is no background updater; rebuild, transfer, and install each revision
deliberately. Start the local daemon from the installed `xdcode` entrypoint.

Explicit paths allow isolated installation tests without changing home variables:

```bash
xdcode_test_root="$(mktemp -d)"
python3.11 scripts/xdcode/install.py \
  target/xdcode-dist/xdcode-0.167.0-personal1-linux-x86_64.tar.gz \
  --install-root "$xdcode_test_root/lib/xdcode" \
  --bin-dir "$xdcode_test_root/bin" \
  --xdcode-home "$xdcode_test_root/runtime"
"$xdcode_test_root/bin/xdcode" --version
```

The install paths do not set the CLI's runtime home. For an actual isolated CLI
run, pass that runtime directory as `XDCODE_HOME` to the CLI process. Do not
reuse another application's runtime home for testing. The installer does not
create or modify configuration, credentials, tasks, or data in the runtime
directory.

## Transcript, images, queueing and diagnostics

The model picker opened with `/model` groups models by provider display name and
keeps their configured order within each provider. Search matches model and
provider names. The provider and profile can also be added to the status line.

With the default keymap, press `Ctrl+X`, then `[` to select transcript text.
Arrow keys and Vim motions move the cursor; character and line selections can be
copied with `y`. `/` opens Find within selection mode; `F3` opens transcript Find
from the main view. `Esc` returns to the preserved composer draft. These bindings
are configurable, and the default selection chord yields to an explicit binding.
`Ctrl+L` clears the visible terminal while detailed history remains available via
`Ctrl+T`.

Asynchronous question choices and answers support Markdown and links. Local image
attachments retain their files when text is restored or moved through Vim
registers. Supported image terminals show a thumbnail strip above the draft;
unsupported terminals retain textual attachment markers. Image paths shown by
image-view tools are clickable. Pending patch approvals allow expanding the diff
inline before deciding.

Queue a message and ask the selected server to resume an unloaded thread:

```bash
xdcode queue --thread SESSION_ID --message "Continue the task" --resume
```

An already loaded thread continues as it is. If queueing succeeds but resumption
fails, the command preserves the queued message and prints a warning. Queueing
uses the thread's model; unsupported model and interactive flags are rejected.
Use `--remote` to select another configured app-server endpoint.

Run focused network checks with:

```bash
xdcode doctor --network-only
xdcode doctor --network-only --json
```

The report covers effective local configuration, network settings, provider HTTP
reachability and the Responses WebSocket handshake. It does not scan databases or
MCP servers. Probes use configured provider endpoints and do not follow redirects;
invalid configuration produces a report without network probes. A disabled
WebSocket transport is reported without probing it. External credential commands
and OAuth resolution are not run by these probes, and their absence is reported
as a partial result. Diagnostic output redacts credentials and raw wire errors.

The bundled `xdcode-delegate` skill explains how to select a worker model and
override its supervision deadline in a prompt.

## Provider setup and independent state

Create your own `~/.xdcode/config.toml` (or your chosen
`XDCODE_HOME/config.toml`) using the packaged `config.example.toml`:

```toml
model = "gpt-5.6-sol"
model_provider = "personal_proxy"

[model_providers.personal_proxy]
name = "My Responses proxy"
base_url = "https://example.invalid/v1"
env_key = "XDCODE_API_KEY"
wire_api = "responses"
requires_hosted_auth = false
```

The example uses the non-routable placeholder `https://example.invalid/v1`.
Replace it with your Responses provider's base URL and select a model that the
provider serves. Provide `XDCODE_API_KEY` from a secret manager or
an interactive shell; no actual key belongs in source, package metadata, logs
or a checked-in example. Keep `requires_hosted_auth = false` for this provider;
xdcode setup does not use hosted account login. Runtime configuration and
credentials belong to this application.

Keep production, development and test installations, runtime homes, task data
and local daemon state separate. Installing a binary does not migrate state.
Use explicit `XDCODE_HOME` per runtime and explicit installer paths for independent
build installations. Keep other applications' installations and runtime state
outside xdcode's installation/data paths.

## Checks and release responsibility

Focused checks (no V8/Cargo build, no model requests):

```bash
python3.11 -m unittest discover -s scripts/xdcode/tests -p 'test_*.py' -v
python3.11 -m py_compile scripts/xdcode/build.py scripts/xdcode/package.py \
  scripts/xdcode/install.py scripts/xdcode/tests/test_delivery.py scripts/xdcode/tests/test_review_fixes.py
git diff --check -- scripts/xdcode README.md docs/xdcode.md .github/workflows/xdcode.yml
```

The tests compile a small native fixture with `cc` and exercise the actual
packager and offline installer, integrity checks, malicious archive cases,
process refusals, upgrade retention and source digests. The Cargo/strip sequence
test substitutes the expensive build commands and verifies ordering and digest
propagation. A small direct-`rustc` harness executes the actual bwrap build script
with C compilation substituted and checks resolved-directory header tracking;
it does not invoke Cargo or rebuild the full runtime. These tests do not establish that the real Rust runtime builds or
that its sandbox, provider, MCP or daemon works.

`.github/workflows/xdcode.yml` runs focused delivery checks. Its manually enabled
build job prepares native GNU dependencies, builds/packages with the same scripts,
installs the real package into an isolated directory, verifies its version, and
uploads artifacts by default. Explicitly enable `publish_release` together with
all three platform builds, or with only Windows, to publish a GitHub Release
after every selected platform passes its native build, package integrity,
independent runtime, identity, Responses/tool loop, MCP, resume and reliability checks.
Each platform also smoke-tests the online install entrypoint against its real
local artifact in an isolated home. Publication rechecks all archive digests,
source commit, clean-build metadata, tag-bound build IDs and the Linux
installer when Linux is included, uploads the selected assets to a draft, then
publishes it. A full release includes seven assets. Tags use
`v<version>-g<source-commit-prefix>`. It does not replace an existing release.

```sh
gh workflow run xdcode.yml --ref main \
  -f build_package=true -f build_macos=true -f build_windows=true -f publish_release=true
```

To supplement the existing Linux/macOS release with Windows only:

```sh
gh workflow run xdcode.yml --ref main \
  -f build_package=false -f build_macos=false -f build_windows=true -f publish_release=true
```

Windows-only publication uploads its ZIP and checksum under a new source-derived
tag. It does not rebuild Linux/macOS, modify old releases or replace GitHub's
latest release. The Windows installer finds the most recently published stable
release with Windows assets independently of that latest marker. Source receipts
remain truthful; artifacts from different commits are not mixed under one tag.
Other partial platform combinations are rejected before expensive builds.

The lead must still review source, native dependencies and validation coverage
before declaring a release usable. Heavy CI builds need sufficient runner
memory/disk. A dispatched workflow is not evidence that a build passed.

The repository-root `install.sh` is a manually downloaded latest-release
bootstrapper, separate from the runtime and the offline Linux installer. It
detects Linux x86_64/macOS ARM64 and can pin a release with `--release`.
`XDCODE_INSTALL_PROXY` configures its downloads without changing global settings.
On Linux it executes only the offline installer extracted from the verified
archive; the Python 3.11+ prerequisite and same-build overwrite refusal remain.
macOS installation uses system tools and an atomic managed symlink, with no
Python prerequisite. Windows uses root `install.ps1` and a native immutable
launcher; see [the Windows delivery guide](xdcode-windows.md). Unsupported
architectures are explicit failures, not fallback downloads from another product.
Existing published two-platform releases remain installable on Linux/macOS,
but have no Windows asset. No script changes a daemon's version in place.
Personal build-ID overrides are for offline builds; publication requires the
commit-derived `g<commit-prefix>` identity so all installer filenames bind to
the release tag.

Identity acceptance also requires inspecting the complete instructions actually
sent to the model, including fallback and realtime paths and bundled
self-description skills. In fresh sessions ask "Who are you?", "What is your
name?" and "What is this command-line assistant?". The application should
identify itself as xdcode while describing its configured model and provider
accurately when asked. A text search alone does not establish runtime behavior.

## Manual upstream transplant workflow

AI-assisted upgrades start with `UPSTREAM_UPGRADE.md` at the repository root and
the repository's `AGENTS.md`. The upgrade guide defines baseline comparison,
feature selection, dependency review, application invariants, verification and
delivery requirements. These are source-maintenance documents; the offline
package carries this delivery guide and the recorded build provenance.

Keep the original upstream remote as a source reference and your xdcode
remote as the derivative's destination. Use a separate clean development
checkout or branch, select the upstream source commit and applicable stable
release deliberately, then transplant the upstream changes. Apply the personal
rename/provider/service-removal changes with review; broad replacements can
corrupt wire values, model IDs, dependency URLs and attribution. Preserve LICENSE
and NOTICE and inspect new upstream outbound services and updater entry points.
Keep protocol identifiers and dependency names truthful; never encode strings to
hide identity or service references. Selective fixes must be recorded as selected
commits, without claiming that the entire newer baseline has been adopted.

Update `upstream.json` with the product version, stable tag, exact selected
upstream commit and release-branch context. Reconcile shared Cargo/lock/Bazel
metadata through the lead, review migrations, and rerun focused plus real runtime
validation. Assign a fresh personal build ID, package from the reviewed source,
and install only after existing tasks complete. Installation does not pull,
merge, update a running development tree, or migrate another product's data.
