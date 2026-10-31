# Native delegation

Inspect the tools exposed to the current parent. Do not synthesize missing tools
or fields from repository source, another session, or this reference. Native
`multi_agent_v1` commonly exposes the following shapes; use them only if the
current schema agrees:

| Operation | Typical v1 inputs | Purpose |
| --- | --- | --- |
| `spawn_agent` | `message` (or `items`), `model`, `reasoning_effort`, `fork_context` | Start a child and retain the returned `agent_id`. |
| `send_input` | `target`, `message` (or `items`), `interrupt` | Continue related work; `interrupt=true` interrupts the current task. |
| `wait_agent` | `targets`, `timeout_ms` | Wait for a final status; timeout leaves agents running. |
| `close_agent` | `target` | Request shutdown and return previous status; also closes descendants. |

Some runtimes instead expose task names, `fork_turns`, separate follow-up/message
and interrupt tools, or a mailbox-only wait. Follow that schema and its returned
identifier. Do not translate `fork_context` into a made-up argument, assume a
mailbox notification contains results, or interpret “previous status” as proof
that shutdown has completed. Inspect status/events and remaining activity.

## Model and effort selection

When the **live spawn schema advertises catalog ID selection**, pass the exact
`models[].id` as `model`. For example, `model="worker-model"` selects both
the configured `worker_provider` and its wire model only if that entry
exists and the runtime supports this contract. Do not send the display label.
No separate `provider` argument may be invented.

When the result includes `selection`, compare its `catalog_id`, `model`,
`provider`, and `reasoning_effort` with the requested tuple. These are effective
child settings after role overrides. A mismatch needs correction before further
work; the requested arguments alone are not confirmation. Older tools may return
only an agent identifier, in which case inspect available runtime metadata.

If only wire names are supported, use a name only when the runtime documents
selection within the inherited provider, that provider is confirmed to match the
user's choice, and the wire name is unambiguous there. Cross-provider selection
or same-name ambiguity needs a supported exact ID or the authorized exec route.
An older binary rejecting an ID must not trigger retry with a different model.

Supply an authorized supported `reasoning_effort` only through an exposed field.
Check what omission does: it may inherit the parent's effort. If the desired
selection is service default, omission is safe only when the runtime confirms
that no effort will be sent for this child. Never encode service default as
`"none"`, empty string, or an invented null field. Catalog metadata and native
schema limitations must both be respected.

Use a fresh context for a different provider/model when required by the live
schema or history portability rules. Send the bounded task and minimum relevant
facts. Full-history fork is appropriate only when permitted, useful, and supported
for the selected identity; never forward provider-bound encrypted history across
an interface boundary. Do not modify the parent to influence inheritance.

## Parent supervision

Maintain the deadline ledger in the parent, not just in the child prompt. Poll
status/logs at useful intervals and reserve a bounded stop/review window. At the
5-minute check (or the user's interval), inspect the diff and evidence of progress.
If redirecting, reuse `send_input` with the original deadline and write set.

When time expires, snapshot available results and promptly interrupt the target
with a request to stop writing and return partial work. Close it if unfinished or
no longer needed; do not wait for a final answer indefinitely before shutdown.
Recheck after shutdown and include partially written/unvalidated work in review.
Close completed agents after collecting their evidence unless a concrete related
task will reuse them. Closure consumes no authority to kill unrelated processes.

A parent-controlled native deadline is bounded supervision, not an independent
process watchdog. A long tool call or suspended parent can delay reclamation;
acknowledge that limit when a user requires a hard process cutoff.
