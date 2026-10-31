# Exec supervision

Use this route when the user chooses `xdcode exec` or native tools cannot safely
express the authorized child identity. Read the local `xdcode --help` and
`xdcode exec --help` first, and verify the executable/build in use. Do not use
unsupported flags or assume new source code is installed. This skill does not
require a source checkout, a fixed binary path, or a bundled runner.

## Configure only the owned process

Carry the active configuration source, profile and working directory into the
child command. Derive the provider ID, exact wire model and supported effort
from `xdcode models list --json`. Use documented process overrides: commonly
`-m`/`--model` sets the **wire model**, and `-c` sets `model_provider` and
`model_reasoning_effort`. Resolve the catalog ID to this tuple first; do not assume
`-m` accepts catalog IDs just because `spawn_agent` does. Inspect local help and
runtime effective settings rather than assuming precedence or inventing a config
field to select a catalog ID.

Construct argv as an array (for example with Python `subprocess.Popen`) and
serialize each `-c` value as a TOML string; JSON string escaping is not shell
quoting. Pass the task prompt on stdin (often `-`), never interpolate it into a
shell command. For explicit service default, use the runtime's documented clearing
operation; if none exists and an effort would be inherited, stop. Do not use
`"none"` as a substitute or edit config files to create a temporary default.
Keep credentials in the existing environment/store, out of argv, prompts and
logs. Do not add bypass/sandbox/approval flags that exceed the parent's authority;
even convenient automation flags must be checked against the parent permissions.

## Own and limit the process

Before starting, allocate task-specific result, stdout/stderr (or structured
event) logs and a supervision record in an authorized temporary/state directory.
Record the baseline diff, command identity without secrets, launch wall time,
PID, and monotonic deadline. Enable structured events and a final-result file
only through flags confirmed in local help. Inspect actual startup/session
settings for the selected provider/model/effort; a handmade “provider confirmed”
line or startup model label does not verify the actual route.
If using `--ephemeral`, retain the runtime identity evidence exposed by that
version in the supervised logs: no durable worker rollout is expected. Missing
worker-visible metadata is unavailable evidence, not a detected model mismatch;
the parent must verify the launch and should not send the worker searching for
invented environment variables.

On POSIX, create a dedicated session/process group with
`subprocess.Popen(argv, stdin=..., stdout=..., stderr=..., start_new_session=True)`.
Retain the returned `Popen`/PID and group ownership in the **external supervisor**.
Compute `deadline = time.monotonic() + budget_seconds` before launch, with the
budget capped by the parent deadline. Arm the watchdog as soon as the owned PID
is known, before feeding stdin or waiting for startup verification. Use bounded
or nonblocking I/O so a child that does not read its prompt cannot delay the stop.
Do not give the worker control over the watchdog or leave it awaiting a model turn.

The supervisor loop should:

1. Feed the prompt on stdin, close it, and keep worker output in bounded logs.
2. Poll the owned process and monotonic clock; at the progress-check time make
   diff/logs available to the parent. The parent can reclaim early if it loops.
3. At the deadline, if still running, send TERM to the dedicated owned group,
   wait a short grace period, then KILL that same group if needed and reap the
   child. Account for this grace period before the parent's hard deadline.
4. Record exit code, elapsed time, timeout/early-stop reason and remaining output.
   Ensure cleanup also runs on supervisor error or parent cancellation.

Use `os.killpg` only for the session/group created and retained by this supervisor;
if it is not available, terminate only the owned child with the platform's process
API. Never use broad `pkill`, process-name matching, terminate a pre-existing
session, or kill another application. Avoid delayed cleanup against a stale PID;
ownership and lifecycle must still be established. Confirm that no owned worker
descendant continues writing before reviewing the final diff.

An exec tool's `yield_time_ms`, a shell command returning a session ID, or a
Python `wait(timeout=...)` exception alone does not terminate a process. An
independent watchdog must continue enforcing the recorded deadline while the
parent does other work. If this cannot be arranged, report the supervision limit
and do not claim a hard timeout. On timeout, collect and review partial artifacts;
neither exit code zero nor the presence of a result log proves the requested
development task is complete.
