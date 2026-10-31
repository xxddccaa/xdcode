# xdcode configuration

Put user configuration in `~/.xdcode/config.toml`, or
`XDCODE_HOME/config.toml` when using an explicit runtime home. Project
configuration belongs in `.xdcode`. Keep development and production state
separate, and keep credentials out of checked-in configuration.

The [delivery guide](xdcode.md#provider-setup-and-independent-state) contains the
Responses provider example. Set `model` and `model_provider` to match your
service, use `env_key` to name the environment variable containing its key, and
set `requires_hosted_auth = false`. A model or provider change does not change
the application's xdcode identity.

The source tree's [configuration schema](../xdcode-rs/core/config.schema.json)
describes configuration fields for that source revision. Use the schema that
matches your build. A schema entry alone does not establish that an integration
is supported by the delivered package.

## Connecting to MCP servers

Configure servers you operate or trust in `[mcp_servers.<name>]`. For an already
installed local stdio server, replace the executable path below with its actual
path:

```toml
[mcp_servers.local_tools]
command = "/absolute/path/to/mcp-server"
args = []
```

Add arguments required by that server. If it needs a credential from the current
environment, explicitly forward the variable name with `env_vars`, keeping the
value out of the file. Start a fresh xdcode session after editing configuration
and verify both tool discovery and an actual call. A server's tools may read,
write or send data using the server's own permissions; review that boundary
before enabling it.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
