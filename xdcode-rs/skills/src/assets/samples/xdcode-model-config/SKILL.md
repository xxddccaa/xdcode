---
name: xdcode-model-config
description: Add or maintain xdcode model catalog entries, providers, and reasoning effort settings, including requests to add a model from its official model-page URL. Use for 添加模型、配置厂家、调整思考等级 and 把模型链接加入 xdcode.
metadata:
  short-description: Configure xdcode models and provider credentials safely.
---

# xdcode model configuration

Maintain the installed application's unified model catalog using `xdcode models`.
This skill ships inside xdcode; its references and helpers work without a source
checkout. Read [the configuration contract](references/configuration.md) before
editing, and [the OpenRouter reference](references/openrouter.md) for OpenRouter
links or providers.

Inspect `xdcode models list --json` with the current profile and startup config
overrides. Use its effective source and version; the active file may differ from
`~/.xdcode/config.toml`. Preserve the current profile (`-p`) when running commands.
If the session exposes an effective configuration path or profile, use those
values. Resolve uncertainty using local command help and configuration source
metadata, without printing credential values or dumping the full configuration.

Use `models upsert --json --preview` with a JSON object on standard input, then
apply the validated payload using its configuration version. Merge by stable ID,
reuse providers, preserve unrelated entries, and re-read after conflicts. Add a
model to the catalog by default. Switch the current mode only when the user also
requests switching; a config edit alone must not change the active session.
When switching is requested, use the application's confirmed thread-settings
operation if available; otherwise explain that `/model` applies the new entry.
Do not claim a switch occurred merely because a file changed.

Treat user-supplied effort lists as authoritative, including custom strings.
An explicit empty list means service default. Omission requests discovery; do not
invent confirmed levels or rewrite a manual list from generic model knowledge.
A genuine request rejection is reported with its reason; do not replay a turn,
change model silently, or replace a manual list to hide it.

`models check ID` is static. `--probe` and `models refresh ID` can contact the
configured provider; use them only when capability verification or refresh is
within the user's request and permitted by the current task. Explicit source-only
or offline constraints prohibit them. A failed authentication, timeout, rate limit,
or unavailable search tool leaves capabilities unknown. Report what is confirmed,
what remains unverified, and whether configuration was actually saved.

Only Responses API is supported. A model confirmed to support only Chat
Completions may be retained as an incompatible, unselectable entry. Do not imply
that adding its URL makes it usable, switch to a paid sibling, or introduce a
new transport. Keep exact model IDs, including routing and price suffixes.

Use the runtime's available search tools and its local tool rules for official
model documentation. If those tools are unavailable, keep verification unknown;
no fabricated lookup or API success. Credentials stay in provider environment
variables or the existing credential store. Explain the required environment
variable when missing; never ask for a key in chat or put it in JSON, argv,
examples, reports, or committed files.
