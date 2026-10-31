# Unified configuration contract

## Source, profiles, and precedence

Use `xdcode models list --json` to obtain `source`, `version`, `models` and
redacted `providers`. The default user file is `$XDCODE_HOME/config.toml`, with
`~/.xdcode` as the default home. A session may have a profile and explicit
configuration overrides; preserve those options when invoking the command.
`-p NAME` selects an existing profile. Do not create a separate configuration
file per model or treat `-p` as a model catalog ID.

Explicit startup parameters win over remembered project choices. Explicit
historical-session restoration wins over project defaults, subject to startup
parameters. An ordinary one-startup override does not rewrite project memory.
Confirmed user model selections and mode switches can persist project choices.
Ordinary and Plan modes remember independent provider, directory ID, model and
effort settings, scoped to canonical Git worktree root (startup directory outside
Git) and active configuration identity. Plan initially inherits the ordinary
model. The current mode determines which choice is changed; no save-scope prompt
is required.

When a user switches modes, an invalid remembered catalog entry or a structured
server rejection of cross-provider encrypted history permits one fallback to the
currently acknowledged provider, exact catalog ID, model and effort. The server
must accept that fallback before the target mode's project preference is saved.
The application reports the reason and preserves the other mode's preference.
This applies only to restoring a mode's remembered choice, not explicit `/model`
selections, transport failures, arbitrary server errors or inference failures.
If fallback fails, retain the previous mode, model and draft. Never replay an
inference request or transfer encrypted history across providers to make it work.


Profiles share session history within the same `XDCODE_HOME`: `/resume`, named
resume, latest resume and fork search all providers, with their existing directory
filters. Project model preferences remain scoped to the configuration identity.
Shared visibility does not permit forwarding provider-specific encrypted history.

## TOML schema

Providers remain `[model_providers.ID]`. A model's `provider` is that ID, its
stable unique `id` distinguishes catalog entries, `name` is the display name,
and `model` is the exact API identifier. Display and request names can coincide
across providers. Each provider owns its actual `base_url`, including any API
path prefix, credential references and headers. Reusing a model name never means
reusing another provider's endpoint or credentials. Example:

```toml
[model_providers.my_provider]
name = "My Responses service"
base_url = "https://example.invalid/v1"
env_key = "MY_MODEL_API_KEY"
wire_api = "responses"
requires_hosted_auth = false

[[models]]
id = "custom-coder"
name = "My coder"
provider = "my_provider"
model = "vendor/exact-model-id"
reasoning_efforts = ["low", "high", "vendor-level"]
default_reasoning_effort = "vendor-level"
context_window = 128000
auto_compact_token_limit = 100000
compatibility = "unverified"
```

Context fields are optional positive token counts; use verified provider values.
`compatibility` is `unverified`, `responses` or `incompatible`. A `responses`
mark requires actual supporting evidence; configuration validation is not proof
of model availability, tool support or endpoint compatibility.

`reasoning_efforts` has three distinct meanings:

| Configuration | Behavior |
| --- | --- |
| Omitted | Discover supported levels; unknown until confirmed |
| `[]` | Offer service default; omit request `reasoning.effort` |
| A nonempty string list | Trust this manual list without discovery |
| `["none"]` | Send the literal string `none` when chosen |

An optional `default_reasoning_effort` must occur in a configured list. Custom
strings are supported. Never carry an unsupported effort across models. Service
default means clear the effort, which differs from leaving a setting unchanged.
When no catalog exists, legacy `model`, `model_provider`, reasoning configuration
and profiles retain their behavior, including custom configured model names.

## Command workflow

Commands use the same parser, validation, discovery and atomic writer as the
application. Get fresh help if an installed build predates these commands.
List output is redacted: do not reconstruct or print provider credential fields.
To reuse a provider, supply only a new model and omit `providers`.

```sh
xdcode models list --json
xdcode models upsert --json --preview --expected-version VERSION < model-patch.json
xdcode models upsert --json --expected-version VERSION < model-patch.json
xdcode models check custom-coder --json
```

Replace `VERSION` with `list`'s actual content version, not the literal word.
The input file contains an object of this shape:

```json
{
  "models": [
    {
      "id": "custom-coder",
      "name": "My coder",
      "provider": "my_provider",
      "model": "vendor/exact-model-id",
      "reasoning_efforts": ["low", "high", "vendor-level"],
      "default_reasoning_effort": "vendor-level",
      "compatibility": "unverified"
    }
  ]
}
```

`providers` optionally maps provider IDs to provider definitions; its fields
match TOML provider fields, with no secret values. Treat each upsert model as the
complete intended entry: retain its existing optional fields unless the user
asks to remove them. Do not send the redacted provider listing back as a provider
definition. A preview validates but does not write. A version conflict requires
re-reading, merging the intended change and previewing again. The writer
preserves comments and unrelated fields and replaces the file atomically.
Permission or read-only failures do not count as a successful save.

Static `check` reports configuration, credential availability and compatibility
state without requests. Use `xdcode models check custom-coder --probe --json`
only for authorized real verification; it can call the provider. The effort
refresh command is `xdcode models refresh custom-coder --json`; manual lists are
protected unless the user explicitly requested replacing them, in which case
`--overwrite-manual` makes that intent explicit. Refresh is a network operation.

## Discovery and failures

Discovery prefers authoritative model metadata, then explicit API enumerations,
then finite candidate probes. It uses at most two concurrent requests and a
60-second total deadline. Requests have limited output and contain no user
history, project files or tools. Candidate strings include standard levels and
custom levels returned by the service. No authentication, rate-limit, network or
server failure proves absence of reasoning support.

Generated `reasoning_discovery` records contain `source`, Unix-second
`discovered_at`, `model`, `interface_fingerprint`, `complete` and
`efforts_fingerprint`. The source is `authoritative_metadata`, `api_enum` or
`verified_probes`. Only an authoritative enumeration is complete; probes mean
verified levels with unknown completeness. Do not manufacture this record in
upsert payloads. Runtime discovery owns it.

Saved results are reused until the model or interface fingerprint changes.
Editing the generated effort list makes it user-maintained. An explicit refresh
does not silently override that list. Discovery merges against the latest file
and retains in-memory results if saving is impossible; report the failure to
save. On real request rejection, report the original error without automatic
replay or switching. Opening `/model` reloads the catalog and provider definitions;
adding entries becomes visible without restarting and without changing the
session's active choice.

A confirmed selection updates provider routing, model, directory ID, effort and
mode atomically for the next turn. The active turn keeps its original settings.
Cross-provider switching retains portable history and thread state while
isolating provider authentication, connections, encrypted history and compaction
caches. If portable history is unavailable, the application blocks switching
rather than silently dropping context. A rejected update leaves the old choice
active; failure to save project memory means the confirmed choice lasts only
for this run.

Current command limits: JSON provider patches preserve existing true capability
flags; change such a flag to false by editing its TOML field. Discovery uses
environment or existing bearer credentials. Command, OAuth and AWS authentication
require verification through the normal runtime client. A basic Responses probe
leaves tool-calling support unverified. Report these boundaries when relevant.
