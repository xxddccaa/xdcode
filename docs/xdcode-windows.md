# xdcode for Windows x86_64

The native target is `x86_64-pc-windows-msvc`. The delivery workflow builds and
checks it on `windows-2025`; this is not evidence of separate desktop-version
coverage. ARM64 and 32-bit Windows are not supported by this archive.
The first Windows package is available only after its native CI job and either
Windows-only or three-platform publication succeed.

## One-command installation and upgrades

Run from PowerShell 5.1 or newer (PowerShell 7 is recommended):

```powershell
& ([scriptblock]::Create((Invoke-RestMethod -Uri 'https://raw.githubusercontent.com/xxddccaa/xdcode/main/install.ps1' -Proxy 'http://127.0.0.1:10826'))) -Force -Proxy 'http://127.0.0.1:10826'
```

The documented command uses the local `10826` proxy for both the initial script
and subsequent package downloads. Remove both `-Proxy` arguments for direct
access, or replace the address with another proxy. `XDCODE_INSTALL_PROXY` can
also configure the installer's downloads without changing global settings.
The installer selects the most recently published stable release containing
both the Windows ZIP and its checksum, even when it is a supplemental release
not marked as GitHub's latest. Drafts, prereleases and other platforms are ignored.
Omit `-Force` to require stopped xdcode processes. To pin a build, add
`-Release v<version>-g<source-commit-prefix>`.

The script verifies the external archive digest and filename, ZIP paths and file
types, every internal checksum, clean-source metadata and native x86_64 PE
headers before activation. It probes the CLI, code-mode helper and ripgrep.
An exclusive installation lock remains mandatory even with `-Force`.

Builds live in `%LOCALAPPDATA%\xdcode\builds\<version>-<build-id>`. The entry is
`%LOCALAPPDATA%\xdcode\bin\xdcode.exe`, a small native launcher that forwards
arguments without a batch file or extra command shell. It reads only the local
`current.txt` selection; it does not contact an update service. Upgrades retain
the verified launcher and its original build, and atomically change that
selection instead of overwriting an executable used by existing sessions.
Unrecognized entrypoints or modified existing builds are refusals.

The installer adds its bin directory to the user and current PowerShell PATH.
Open a new terminal if an existing terminal has cached the previous PATH.
`-NoPathUpdate` skips both PATH changes for isolated or manually managed installs.
No administrator privileges, Rust, Node.js or source checkout are needed to
install a published package.

Existing processes continue their old builds. Installing does not stop sessions
or upgrade a running local daemon in place. Finish tasks and restart the relevant
daemon safely before mixing a new CLI with an old daemon.

## Runtime and security boundaries

Configuration and history remain in `~/.xdcode`, or an explicit `XDCODE_HOME`.
Installing does not import another CLI's configuration, change provider keys,
remove old builds or provision Windows sandbox accounts. The sandbox setup and
command-runner helpers are included but are not executed by installation probes.
The automated Responses/tool/MCP checks use an isolated unsandboxed test runtime;
they do not establish elevated Windows sandbox behavior.

Executables are **not Authenticode signed**. SHA-256 checksums establish integrity,
not publisher signatures. Obtain the scripts and assets from this repository's
trusted HTTPS release source. Do not disable system security globally.
If a build reports a missing VCRUNTIME/MSVCP DLL, check
`build-info.json.binary_dependencies` and use Microsoft's supported x64 Visual
C++ Redistributable. The installer does not silently install system runtimes.

## Rebuilding and verification

To publish only Windows without repeating the existing Linux/macOS builds:

```sh
gh workflow run xdcode.yml --ref main \
  -f build_package=false -f build_macos=false -f build_windows=true -f publish_release=true
```

The supplemental release uses its own clean source commit and tag, uploads only
the Windows ZIP and checksum, and does not replace GitHub's latest release.
Existing Linux/macOS releases stay intact. A newer commit's Windows binary is
not attached to an older source-derived tag.

Use a native x86_64 Windows host, Visual Studio's x64 developer PowerShell with
`dumpbin` available, Python 3.11+, and the pinned Rust toolchain:

```powershell
python scripts/xdcode/windows.py
python scripts/verify_xdcode_runtime.py target/xdcode-windows-package/bin/xdcode.exe
python scripts/verify_xdcode_reliability.py target/xdcode-windows-package/bin/xdcode.exe
```

The builder requires a clean commit, uses the locked Cargo graph and checksum-pinned
V8/ripgrep inputs, includes all helper executables and records native DLL imports.
It produces `xdcode-<version>-g<commit>-windows-x86_64.zip` and `.zip.sha256`.
The native CI installation smoke test uses an isolated runtime and does not
modify the runner's user PATH. A configured or dispatched job is not a successful
build; native build, installer and runtime results remain pending until CI passes.
