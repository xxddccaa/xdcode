# xdcode 0.165.0 migration and reliability fixes

Migration started October 29, 2026; verification continued October 30, 2026.
This is an independently maintained derivative of OpenAI Codex. Attribution and
licenses remain in LICENSE, NOTICE and MODIFICATIONS.md.

## Baseline and scope

- Previous baseline: `rust-v0.163.0`, `83086188ae0614bd882818ff1aa23f3194962012`.
- Selected stable release: `rust-v0.165.0`, `360319d8dee62d87e2d59fc0c21805d79519cfed`.
- Release branch base: `b559e6a3988489b636e1e79672eff330bf0679dc`.
- Personal starting commit: `bafc5b03eb0bef03092c90a23a0b1464a62a58cb`.
- Development branch: `feature/upstream-0.165.0`.
- Upstream main was fetched into the sibling `codex-source` checkout at
  `588a277b301c3994455885d7f771d7d9db5d82b8`. The release target is the stable
  tag above, not the newer `0.166.0-alpha` tags or main.

The source range contains 400 upstream commits and 2,234 changed files before
namespace conversion and xdcode adaptations. The migration includes session
settings/history restoration, compaction and cancellation, MCP cancellation and
tool discovery, local executor/sandbox fixes, updated protocol/SDK artifacts,
Rust 1.99.1 and V8 152.2.0. Representative upstream changes are:

| Commit | Adopted behavior |
| --- | --- |
| `430a16799d` | Preserve thread model settings across resume and revert |
| `8531184b49` | Agent-message wakes respect interrupts and Plan mode |
| `2dbe6bfa44` | Cancel MCP requests when response futures are dropped |
| `cb2581dae8` | Preserve MCP disable reasons across runtime overlays |
| `4610b573e5` | Run environment hooks on their owning executors |
| `c5ef8cfcaa` | Preserve SystemRoot for Windows sandbox helpers |
| `90894a780d` | Avoid racing memory imports with consolidation |
| `85de07048c` | Isolate V8 source dependencies and C++ runtime settings |

Windows/macOS source adaptations are included; this maintenance run verifies
Linux x86_64. New database migration `0062_threads_attached_parent.sql` requires
normal backups before upgrading a populated runtime. An older binary is not a
database rollback mechanism.

## Personal behavior preserved

The command and runtime identity remain xdcode, using XDCODE_HOME and an explicit
user-configured Responses endpoint. Hosted login, hosted cloud tasks, telemetry
uploads, hosted model defaults and online self-updates remain disabled. Protocol
identifiers and external source attribution retain their upstream spelling where
the wire contract requires it.

Model catalog IDs, arbitrary configured reasoning levels, atomic configuration
writes, profile/worktree preferences, distinct Default/Plan selections, and
shared history across providers remain. Provider changes retain portable history
and reject incompatible encrypted history. No hosted model fallback is enabled
to satisfy upstream-only tests.

## Plan mode and rate limits

An implementation message now waits for `thread/settings/update` to succeed
before it is sent. Failure keeps Plan mode and restores the draft. Success sends
one message with the confirmed Default settings. Missing or incomplete mode
discovery falls back to local Default/Plan presets without inventing a model.

Temporary HTTP 429, Responses SSE and WebSocket rate limits keep the current turn
alive until success or cancellation. The rate-limit counter is independent of
the ordinary stream retry budget. Server advice retains its original deadline;
without advice, delays grow from 5 seconds to a maximum of 60 seconds with jitter.
Even zero advice waits at least one second. The UI shows the attempt and next
wait. Foreground/local compaction uses the same cancellable wait. Authentication,
exhausted quota and invalid-model errors remain terminal. Requests retain their
provider/model and completed tool results.

The Azure-style regression contains the reported gpt-6-astra / eastus2 token-limit
message. Classification does not special-case either the model name or region.

## Verified source and test results

These results are actual executions, not inferred from compilation:

| Check | Result |
| --- | --- |
| Core compaction, cancellation, rate-limit deadline and provider-history tests | 124 passed |
| HTTP/SSE/WebSocket retry integration, including Azure message and cancellation | 39 passed |
| TUI Plan transitions, catalog selection, project memory and profile history | 93 passed |
| Responses API library tests | 195 passed |
| HTTP client, including TLS and redirect behavior (serial execution) | 119 passed |
| Model catalog/discovery, concurrent writes and two mock provider endpoints | 24 passed |
| Rust protocol and generated schema/export agreement | 318 passed, 1 ignored |
| Memory rollout extraction | 3 passed |
| Python protocol/config artifact checks | 10 passed |
| Python SDK contract, signatures and RPC methods | 86 passed |
| Bundled skill/source checks | 15 passed |
| Personal build/package/installer tests | 42 passed, 2 skipped |
| Shared package helpers | 27 passed |
| CLI parsing, command definitions and configuration arguments | 144 passed |

Installer test skips protect the active same-user xdcode processes and reflect
the test subprocess tool availability; they are not successful installation tests.
Test logs use `/tmp/xdcode-finish-*.log` on the development host.
The HTTP-client suite needs serial execution here: concurrent tests mutate
process-wide environment and caused transient TLS/redirect failures; all 119
tests passed with `--test-threads=1`.

The inherited complete core/TUI suites are not green: an initial broad run found
604 core and 607 TUI failures, primarily fixtures for removed hosted providers,
authentication and UI. Shared fixtures and affected regression cases were adapted;
the complete suites were not reclassified as passing. Unreviewed generated
snapshots were not accepted. Only the reviewed footer-width and unsupported
worktree-command snapshots were updated.
The broad CLI run had 248 passes, 20 failures and one ignored test. Most failures
were hosted-provider/daemon expectations, output snapshots or root permission
assumptions. The affected CLI cases were adapted and the focused 144-test run
passed; the complete CLI suite is not claimed as green. A command-definition
regression test now checks all subcommands and prevents the stale removed
`remote_control` conflict from crashing app-server startup.

## Build and runtime acceptance

Use the pinned build/package workflow in `docs/xdcode.md`. The receipt records
the source digest, dirty state, toolchain and executable hashes. A dirty source
artifact is a development build and must not be advertised as a clean release.
The source must remain unchanged between the build and packaging steps.

The complete debug binary reports `xdcode 0.165.0`. Actual runtime verification
passed for shell tools, MCP, bundled skill discovery, identity instructions,
session persistence/resume, and TUI identity/model preservation. The reliability
check passed Plan-to-Default transition, one real file write after three rate
limits with ordinary retry budgets zero, and interruption in 0.019 seconds.
Fresh sessions with the configured `gpt-6.1-sol` provider passed both an unprompted
application-identity question and a real command execution. Credentials were
passed in the child environment only and were not saved in the verification home.

SQLite's online backup API copied the running state database for migration
rehearsal. Starting the new app-server against this disposable copy upgraded
schema 61 to 62, preserved all existing columns of all 15 thread rows, installed
the new nullable parent column/index, and passed `PRAGMA integrity_check`.
The live database was opened read-only and was not migrated. This validates the
state schema upgrade; it is not a claim that production installation took place.

Runtime verification is reproducible with:

```bash
python3 scripts/verify_xdcode_runtime.py /path/to/bin/xdcode
python3 scripts/verify_xdcode_reliability.py /path/to/bin/xdcode
```

The second command uses real app-server RPC and a local mock Responses service:
it checks Plan/Default instructions, a real file write after leaving Plan, recovery
from three consecutive token rate limits with the ordinary retry budget set to
zero, exactly one file write, and interruption during rate-limit waiting. These
checks use disposable homes and fake credentials. Their execution results and
the final package path belong in the accompanying delivery report; the existence
of these scripts alone is not runtime acceptance. Real upstream model responses,
production-history migration and installation are separate acceptance steps.

The current maintenance conversation runs in the installed 0.163.0 application.
Do not stop its daemon or replace that installation while this or other xdcode
tasks are active. Prepare and verify the new package first, then install after
all sessions have finished using the offline installer and retained backups.
