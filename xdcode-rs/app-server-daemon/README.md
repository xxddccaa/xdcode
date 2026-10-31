# xdcode-app-server-daemon

Local app-server lifecycle management for xdcode. The daemon uses a complete
package from the current installation. It never checks upstream versions,
downloads packages, runs an installer, launches an updater, or automatically
replaces a running server.

## Commands

```sh
xdcode app-server daemon start
xdcode app-server daemon restart
xdcode app-server daemon stop
xdcode app-server daemon version
xdcode app-server daemon bootstrap
```

Successful lifecycle commands emit a JSON object with the backend, socket path,
local CLI version, and running app-server version when applicable.

`start` is idempotent and waits for the local app-server initialize handshake.
`restart` stops the managed daemon and starts the selected local package.
`stop` requests graceful shutdown, then forces termination after the configured
grace window. `version` queries the local server and installed executable.

## Local packages

When no daemon installation exists, `start` and `bootstrap` copy the invoking
CLI's complete package into `XDCODE_HOME/packages/app-server-daemon`. Package
validation covers the manifest, platform, executable, helpers, and source tree
integrity. The selected release is named by its local content hash. A bare
executable cannot seed a daemon installation.

Existing dedicated packages and previously launched legacy standalone packages
are reused. A broken selection is reported as an error and is never silently
replaced. Seeding does not create an automatic-update channel marker, even if
the source installation has one. Existing updater settings and channel markers
cannot enable updates.

The compatibility APIs `update`, `run_pid_update_loop`, and `update_from_cli`
return an unsupported error before reading the environment, creating files,
contacting services, prompting, or spawning processes. Bootstrap output always
reports `autoUpdateEnabled: false`.

Install a complete local xdcode package manually to change versions. Lifecycle
commands do not watch executable files for replacement. An explicit restart
uses the selected package; an already running daemon stays on its current binary.

## State and shutdown

State lives in `XDCODE_HOME/app-server-daemon`:

- `settings.json` stores launch settings and the shutdown grace period.
- `daemon.pid` records the managed process identity.
- `daemon.stderr.log` contains local startup and runtime diagnostics.
- `daemon.lock` serializes lifecycle operations.

Previously launched legacy installations retain their `app-server.pid` and
log namespace. Legacy updater process records can still be recognized for safe
cleanup, but attempting to launch such a process is unsupported.

`shutdownGraceSeconds` defaults to 60 and accepts integers from 0 through 300.
Zero requests graceful shutdown and immediately proceeds to force termination.
Stop remains usable with malformed settings. Obsolete `updater` settings are
ignored by local lifecycle operations.

## Platform behavior

The daemon uses Unix or Windows process and file-locking primitives. Windows
startup requires a non-elevated terminal whose host permits detached children.
The canonical Windows socket address must fit the 108-byte AF_UNIX limit.

Shared clients use the environment inherited when the daemon started. Opening
a new terminal does not change that environment. An invocation setting
`XDCODE_EXEC_SERVER_URL` skips implicit daemon attachment. If an implicitly
discovered daemon cannot initialize its connection, the TUI uses an embedded
server.
