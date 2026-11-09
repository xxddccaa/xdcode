# xdcode modifications and upstream attribution

xdcode is a modified derivative of OpenAI Codex, distributed under Apache-2.0.
The original copyright and attribution notices are retained in LICENSE and
NOTICE. The original authors do not endorse this derivative.

All first-party source paths formerly named `codex-rs`, `codex-cli`, and their
Codex-prefixed identifiers have been changed to xdcode equivalents. Corresponding
tests, build files, schemas, scripts and documentation are modified as well.
The exact upstream source revision and release baseline are in `upstream.json`.
The recorded upstream revisions identify the original source; this document
records the derivative changes, including in source-only publications.

Behavioral changes remove hosted account/product integrations, official
telemetry, curated-plugin automatic downloads, and all application updaters.
Model calls use explicitly configured Responses-compatible providers. Runtime
state, environment variables, helpers and local app-server namespaces are
independent of other CLI applications. Linux packages carry a build receipt and
an offline installer. Product versions and the actual upstream baseline are
recorded separately when a release selectively backports fixes.

The GNU/Linux offline installer accepts an explicit `--force` to install while
existing xdcode processes continue running. Only the active-process gate is
skipped; integrity checks, safe paths, install locking and old builds remain.
The manually invoked root `install.sh` follows the latest published release,
detects Linux x86_64/macOS ARM64 and preserves the offline installer's integrity
and process protections. macOS uses system tools rather than requiring Python.
The root `install.ps1` supports native Windows x86_64 with checksum/path/PE checks,
an exclusive lock and a fixed native launcher. The launcher forwards arguments
without a command shell and reads only a local atomic build selection, so force
upgrades do not overwrite an executable used by existing sessions.
The manual GitHub delivery workflow can publish Linux x86_64, Apple Silicon and
Windows x86_64 artifacts after their native build and runtime checks pass. It verifies
matching clean source receipts and uploads all assets to a draft before making
the release public. Windows can also be published on its own under a new
source-derived tag, without rebuilding Linux/macOS or replacing their latest
release. Its installer selects the most recently published stable release with
Windows assets. The documented Windows command uses the local `10826` proxy for
both script and package downloads. These operations do not add an application updater or stop
existing sessions. Windows publication is newly configured; native success must
be established by CI rather than inferred from local structural/unit checks.
Windows verification does not provision elevated sandbox accounts.

Third-party protocol/model identifiers, pinned external build sources and
license notices intentionally retain their original spellings. Those names
are not compatibility aliases for the old product.

External wire exceptions include `x-openai-internal-codex-residency`,
`codex/encryptedContent`, `codex/imageDetail`, and the Responses usage field
`codex_rollout_budget_units`. Renaming these would change the service/MCP
contract rather than the product identity.

Obsolete upstream automation and redirect documentation have been removed.
Their original versions remain available at the recorded upstream revisions.
Only the xdcode delivery workflow is registered in this repository.

The 0.163.0 source migration follows upstream tag `rust-v0.163.0` at
`83086188ae0614bd882818ff1aa23f3194962012`. Core, exec-server, MCP, protocol,
TUI and dependency changes are adapted to the existing xdcode namespace and
provider policy. The daemon menu exposes a confirmed local restart without
package update actions; hosted Guardian comparison remains disabled, as do
the default Daybreak and API-key Cyber access-program features. See
UPSTREAM_0.163.0.md for the commit inventory, exceptions and validation limits.
This source migration is not a built or runtime-verified release.

The subsequent 0.163.0 personal model-configuration revision adds a shared model
catalog, Responses provider routing with independent endpoints and authentication,
bounded reasoning discovery, atomic configuration writes, per-project ordinary
and Plan selections, and shared history visibility across profiles. Protocol and
SDK fields carry the provider/catalog selection and explicit effort clearing.
The bundled xdcode-model-config skill and models command family maintain the
same configuration. Discovery table writes retain their owning model when
merging TOML documents. Individual build receipts identify the clean source
commit; release notes describe verification and remaining limits. Personal
credentials and runtime model selections are not included in release packages.

The 0.165.0 migration follows OpenAI Codex stable tag `rust-v0.165.0` at
`360319d8dee62d87e2d59fc0c21805d79519cfed`. It adapts session/history and compaction
changes, MCP cancellation, executor and sandbox fixes, Rust 1.99.1, V8 152.2.0,
and protocol/SDK updates to xdcode's explicit Responses-provider policy.
Hosted login, cloud tasks, telemetry uploads and automatic updates remain disabled.
Mode-bound implementation messages wait for a successful server settings update;
missing preset discovery uses local Default/Plan presets. Temporary HTTP, SSE and
WebSocket rate limits preserve the current turn and retry until success or user
cancellation, including foreground compaction. Server retry deadlines are retained;
authentication, exhausted quota and invalid-model errors remain terminal. See
UPSTREAM_0.165.0.md and the build receipt for verification and delivery details.

## 2026-11-08: xdcode 0.167.0 selective maintenance

The product version advances to 0.167.0 while the actual upstream baseline stays
at Codex 0.165.0. Nine upstream fixes cover context usage, pending agent messages,
paginated history, concurrent SQLite migrations, metadata persistence, filename
ranking, instruction-budget warnings, literal shell prefixes and fork drafts.
Exact commits and adaptation details are recorded in `upstream.json` and
[UPSTREAM_0.167.0.md](UPSTREAM_0.167.0.md); this is not a full Codex 0.167.0 sync.

xdcode-specific changes make ordinary, named and new-worktree `/new` inherit the
confirmed provider, exact catalog ID, model, reasoning effort and collaboration
mode. The server must acknowledge settings before navigation or first input;
failure preserves the old session and draft and attempts empty-candidate cleanup.
`/model` opens at the first selectable row in both catalog and provider views,
while retaining exact current-ID markers. User configuration, other entry-point
priorities, independent project/mode preferences and provider history isolation
remain unchanged. This work does not install, publish or replace a running build.

## 2026-10-31 selective backports

These changes retain the 0.165.0 baseline and product version. The comparison is
upstream stable `rust-v0.166.0` at `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`
and the local main snapshot `67f0994b532b8740f14b5b738af7b7b2a915b1d6`.
`upstream.json` records every adopted commit, including required dependencies.
This selection does not claim a complete 0.166.0 baseline migration.

- The model picker groups entries by provider display name, using case-insensitive
  ordering and provider IDs to disambiguate equal names. Configured order inside
  each group is retained. Search includes provider display names and preserves the
  exact selected catalog ID. Status-line items expose provider and profile.
- Mode switches recover an invalid remembered model or a typed, pre-commit
  cross-provider history rejection by trying the current acknowledged catalog
  selection once. A successful fallback updates only the target mode's project
  preference, preserves exact identity and effort, and reports its reason.
  Explicit model choices and transport failures never trigger this fallback.
  Failed fallback retains the mode, preferences and draft; mode-bound messages
  wait for acceptance and are submitted once. Settings errors include their
  complete cause chain. Provider-specific encrypted history stays isolated.

- Resume and fork retain acknowledged permissions. Reasoning-token accounting
  survives persistence and compaction. Reconnection preserves drafts, reports the
  failed stage, supports offline exit, and avoids resubmitting a startup prompt
  after an explicit draining-server refusal.
- Transcript selection supports keyboard motion, character and line marks,
  copying, and Find. The configurable selection action defaults to `Ctrl+X`, then
  `[`, when that leader chord is free. Search and selection preserve composer
  drafts. `Ctrl+L` clears terminal presentation while keeping detailed history.
- Asynchronous questions, choices and replies render Markdown, including links,
  with consistent live and restored transcripts. The required user-message
  presentation enum is included; xdcode's existing thread-item protocol is retained.
- Image attachments survive killed-text restoration and rich Vim registers.
  The composer shares one attachment type and shows thumbnails where the terminal
  supports images. Viewed-image paths are clickable. Patch approvals allow inline
  expansion while retaining the pending approval. The CLI drops its large
  dispatcher future before polling the interactive TUI.
- `xdcode queue --resume` queues first, then resumes an unloaded thread on the
  selected server. An already loaded thread is left running. A resume failure
  reports a warning without undoing successful queueing. Unsupported queue flags
  fail explicitly. Cross-provider history lookup remains shared.
- `xdcode doctor --network-only` checks effective local configuration, network
  environment, provider HTTP reachability and the Responses WebSocket handshake.
  Probes target only configured endpoints and do not follow HTTP redirects.
  Configuration failure returns a redacted report without guessing endpoints.
  TLS, HTTP rejection and post-upgrade read failures retain typed evidence; unknown
  stages remain unknown. Wire errors, close reasons, tracking-header values and
  credential-bearing URLs are redacted. Auth requiring an external command or
  OAuth resolution produces a partial WebSocket warning without contacting an
  unrelated authority.

Native `spawn_agent(model=...)` resolves an exact catalog ID to its provider,
wire model, supported effort and context limits. A wire model name resolves only
inside the inherited provider; ambiguity is an error. Child configuration clears
stale catalog identity and the parent's cached runtime transport, including after
`/model`. The v1 result exposes the actual selection. Existing role precedence,
service-default effort omission, v2 behavior and encrypted-history boundaries
are retained.

The bundled `xdcode-delegate` skill resolves the user's selected model from the
active configuration and supports native tools and authorized `xdcode exec`.
Default supervision is ten minutes per worker with a five-minute checkpoint;
users may override these limits and specify the model/provider in their prompt.
The parent verifies runtime identity, bounds the write set and reviews the result.
Native deadlines require an available parent; exec hard limits use an external
watchdog. Reusing a worker for the same assignment does not reset its deadline.
The skill and references are packaged and refreshed through content fingerprints.

Validation includes real mock-provider requests for native child routing and
parent isolation, model-switch and encrypted-history regressions, persisted
permissions, 429 recovery, TUI interaction tests, and an empty-cache skill loader.
The skill's initial behavior evaluation verified actual delegated edits and parent
review. Its evaluator exceeded a 240-second soft target by 11.56 seconds while
staying within the external 300-second bound; worker deadlines were satisfied.
That finding informed the total-budget and ephemeral-session guidance. The final
build receipt identifies the source snapshot and binaries; a development receipt
with `source_dirty=true` remains a development artifact until release from a
clean reviewed commit.

Larger optional changes remain outside this selection: unread-result interfaces,
visual interaction changes, switching app-server backends, shared MCP connections,
Code Mode server truncation and the Guardian rewrite. Hosted login, cloud tasks,
telemetry uploads, automatic updates and implicit provider changes remain disabled.

## Fixed-main selective migration — November 8, 2026

The following development pass targets Codex
`c9b4267c370a755da12ec1c8648c2451163212b7` without rolling main tracking.
xdcode remains 0.167.0 on the recorded 0.165.0 upstream baseline. It preserves the
prior 31 backports, confirmed-selection `/new` inheritance, exact catalog IDs,
custom and service-default effort, Default/Plan preferences and provider-bound
encrypted history. Sources, selected portions and exclusions are recorded in
[UPSTREAM_MAIN_C9B4267C.md](UPSTREAM_MAIN_C9B4267C.md) and appended to
`upstream.json`; older attribution records remain intact.

This pass adds applicable storage, lifecycle, approval, sandbox and trust repairs;
per-thread model catalogs and user-layer Provider editing; negotiated MCP leases
and optional stdio startup; permission-aware skill catalogs; and the local agent
message board. It also connects the embedded command center, observed unread
receipts, optional conversation sidebar, independent input state, Markdown,
uncertain-send recovery and returned safety-recourse metadata. The optional
unread/shared-MCP/visual exclusions in the previous section describe that earlier
pass, not this fixed-target selection.

New experiments remain disabled by default. Provider saves do not switch the
current session or project mode defaults. Message-board activation never creates
agents. Sidebar visibility, closing and archiving have distinct history semantics;
uncertain sends are not replayed automatically. Owning-server request views do not
close another live connection. No general multi-backend registration UI is added.

Hosted services, Guardian/reviewer integration, new managed authentication,
automatic plugin downloads or upgrades, host cycling and new-provider `/new`
defaults remain excluded. The development build uses a new source digest and
build ID, records dirty state and is not installed or published by this pass.
