# Xdcode MCP runtime

This crate manages Xdcode's MCP connections, tool calls, and their captured authority.

## Sandbox request metadata

An MCP server advertises the experimental `xdcode/sandbox-state` capability to
receive sandbox metadata on tool calls. The deprecated capability name
`xdcode/sandbox-state-meta` opts into the same contract during migration.

Xdcode supplies these entries in the request's `_meta` object:

| Key                        | Value                                                                                                                    |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `xdcode/sandbox-state`      | Opaque policy to pass to `xdcode sandbox --sandbox-state-json`.                                                           |
| `xdcode/sandbox-state-meta` | An identical copy of that policy under the deprecated name.                                                              |
| `xdcode/sandbox-launcher`   | When available, an object with an absolute `xdcodeExecutable` path to a verified full Xdcode CLI on the MCP server's host. |

Use the launcher path to invoke `sandbox --sandbox-state-json` with the entire
state value serialized as one argument, followed by `--` and the command to run.
The state contains neither launcher discovery data nor deprecation messages.
Preserve unknown policy fields by forwarding the original value rather than
deserializing and reconstructing it. The two state names are aliases, not separate
policies; new consumers should prefer `xdcode/sandbox-state`.

Launcher metadata is currently available only for local stdio servers. An HTTP
endpoint, including loopback, or a remote executor does not establish a shared
filesystem with the harness. Xdcode omits the launcher when its path is unknown,
not representable as UTF-8, or fails the cached capability probe. Absence does not
authorize a PATH search or unsandboxed fallback. Servers that require a launcher
must fail closed or require an explicit operator opt-out.

The state and launcher describe the issuing call. Retain both when starting a
long-lived subprocess or caching policy-bound results; reject changed authority
before reusing them. Tool arguments supplied by the model are not authority.
See [`sandbox_metadata.rs`](src/sandbox_metadata.rs) for the wire types and
[`binding.rs`](src/binding.rs) for execution-environment selection.
