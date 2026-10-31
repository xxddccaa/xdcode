# Add an OpenRouter model

For a request such as “把 https://openrouter.ai/minimax/minimax-m3.1-flash:free
加进去”, preserve `minimax/minimax-m3.1-flash:free` exactly. The bundled helper
extracts that ID without fetching a page:

```sh
python3 scripts/openrouter_model_id.py 'https://openrouter.ai/minimax/minimax-m3.1-flash:free'
```

Resolve the helper relative to this skill directory. Its `verification` value
is always `unverified`: parsing a URL establishes no price, availability,
reasoning, Responses or tool-calling capability.

Inspect the current catalog first and reuse the user's existing OpenRouter
provider definition. Do not assume its ID is `openrouter` or overwrite its
headers, credentials route or endpoint. If none exists, the documented
OpenRouter API base is `https://openrouter.ai/api/v1`; the Responses transport
uses its Responses endpoint under that base. A proposed provider definition is:

```json
{
  "providers": {
    "openrouter": {
      "name": "OpenRouter",
      "base_url": "https://openrouter.ai/api/v1",
      "env_key": "OPENROUTER_API_KEY",
      "wire_api": "responses",
      "requires_hosted_auth": false
    }
  },
  "models": [
    {
      "id": "openrouter-minimax-m3-1-flash-free",
      "name": "MiniMax M3.1 Flash (OpenRouter :free)",
      "provider": "openrouter",
      "model": "minimax/minimax-m3.1-flash:free",
      "compatibility": "unverified"
    }
  ]
}
```

This is an unverified configuration example, not a compatibility endorsement.
Choose a new stable ID only when one does not already identify this model. Omit
`reasoning_efforts` to request automatic discovery; do not supply a guessed empty
list, because that explicitly disables effort sending.

Consult official sources using the runtime's available search/fetch tools:
model page, OpenRouter model API metadata, Responses documentation, reasoning
parameter documentation and tool-calling documentation. Record the date and
source for claims that were actually checked. OpenRouter provider support for
Responses does not establish that this particular routed model supports it.
Model tool-calling support matters for a coding agent; check it separately from
text generation. A successful static configuration check proves neither.

Apply the user's constraints when deciding whether a network check is permitted.
When permitted, `check ID --probe` can establish actual endpoint behavior, while
`refresh ID` discovers efforts. Never send project files, conversation history,
tools or credentials in a public page lookup. Credentialed probes use the
configured credential mechanism.

Report outcomes according to evidence:

| Observation | Action and user-visible result |
| --- | --- |
| Responses and required capabilities confirmed | Save the supported state and report evidence; selection remains the user's explicit action unless switching was requested |
| Official source confirms Chat Completions only, or endpoint rejects Responses as unsupported | Retain `compatibility = "incompatible"` if adding is desired; explain why `/model` cannot activate it |
| `OPENROUTER_API_KEY` (or reused provider env key) missing | Save unverified entry, name the missing variable and explain provisioning through the shell or credential store; do not ask for its value |
| Search unavailable, credential absent, timeout or rate limit | Preserve `unverified`, report the unperformed/failed check, and do not turn the failure into an empty effort list or incompatibility claim |

The local `web-search` MCP is required when the environment's instructions say
so. If it is disconnected, do not fall back to a prohibited built-in search.
No current price or compatibility is verified by this bundled reference.
`:free` remains part of the request model; do not substitute a paid sibling.
Any observed free price applies to the official information at the lookup time,
not a future-price guarantee. Add-only requests do not start a model turn.

Provider reuse must compare the sanitized endpoint identity returned by `models list --json`, including its host and path. Provider IDs and display names alone do not identify the service. Never dump raw provider configuration to recover an endpoint; an old URL may contain credentials or sensitive query parameters.
