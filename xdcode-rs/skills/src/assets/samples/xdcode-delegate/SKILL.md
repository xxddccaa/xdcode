---
name: xdcode-delegate
description: Delegate bounded development to a user-selected model and review its result. Apply only when the user explicitly requests delegation, a smaller worker model, or parallel agents (例如“用我指定的小模型开发，你来审查”); ordinary requests such as “仔细看看” do not authorize delegation.
metadata:
  short-description: Delegate bounded work and review the result.
---

# xdcode delegate

The parent xdcode agent owns model selection, supervision, and final review.
Automatic discovery is enabled; loading this skill alone does not authorize
starting a worker. Preserve the user's scope, model/provider choice, and execution
permissions. Never change the parent's session settings or global configuration.

## Resolve the requested worker

Run `xdcode models list --json` using the session's active config source, profile
(`-p`), working directory, and relevant startup overrides. Inspect local
`xdcode --help` / `xdcode models --help` to express those settings correctly;
do not assume the default config path. Parse `models[].id`, `.provider`, `.model`
(the wire name), `.reasoning_efforts`, and the matching `providers` entry/status.
Display names are labels. The JSON `current` describes loaded configuration,
not necessarily the live parent model after session changes or resume. Obtain
live identity from runtime settings/metadata when it matters; do not infer it
from a file's default model or the parent's self-description.

Resolve an exact catalog ID first, otherwise an unambiguous user-requested
provider plus wire model. Reuse the user's previously authorized preference or an
unambiguous configured worker default when the model is unspecified. Ask only
when the selection cannot be inferred; continue independent parent work meanwhile.
Do not silently substitute another provider or model. An absent
catalog, unknown routing, missing credentials, or incompatible transport requires
reporting the unresolved selection rather than inventing capability or success.
For an authorized catalog change, use the bundled `xdcode-model-config` skill.

Trust configured effort strings, including custom values. An explicit empty
list means service default (no effort sent); literal `none` is different. An
omitted list is unknown, not proof of unsupported reasoning. Do not carry an
unsupported parent effort to the worker or refresh/write the catalog merely to
delegate. If the available launch interface cannot express the requested/default
effort safely, stop and explain that capability gap.

Prefer available native agent tools. Read their current schema and
[native delegation](references/native.md) before launching. Use
[exec supervision](references/exec.md) only when the user chooses `xdcode exec`
or native tools cannot represent the authorized selection and exec can. A source
change or documentation example does not prove the installed runtime supports it.

## Bound and supervise the assignment

Default to **10 minutes per worker**, with a **5-minute progress check**; the user
may override both. Before launch, record the launch time, wall-clock deadline,
monotonic deadline when available, next check, worker ID or owned PID/process
group, selected catalog/provider/wire/effort tuple, and baseline worktree changes.
Cap each worker at the parent's remaining deadline with enough time for stopping
and review. A total parent budget includes setup and final reporting; measure it
from the start of the task, not the first worker launch. Reusing an agent for the
same assignment does not reset its deadline.
A genuinely different, bounded subtask may receive its own recorded budget within
the user's existing authorization; do not reset the clock to disguise a stalled task.

Give the worker a concrete deliverable, explicit write set, relevant starting
facts, acceptance checks, log/result location, deadline, and stopping conditions.
Require it to preserve existing changes, report uncertainty and partial results,
and stop on an observed scope/model mismatch or a blocker. Tell the worker its
selected identity and that the parent owns runtime verification; do not make it
rediscover startup metadata that only the parent can see. Keep any further
delegation or work outside the write set with the parent unless it is already
authorized. A useful task contract is:

> Implement [observable behavior]. You may write only [paths]; preserve the
> supplied baseline changes. Verify with [targeted checks]. Stop by [deadline]
> or on [blocker]. Return changed paths, actual validation, remaining work, and
> runtime identity evidence if exposed; do not guess your provider from your name.

Keep parent and worker write sets disjoint; reuse an available related agent only
when its identity/permissions fit and its previous work has stopped. The parent
should advance necessary independent work, such as inspecting callers or preparing
acceptance checks, while retaining time to supervise. Shared build caches also
need coordination; do not run competing Cargo builds in one workspace.

The parent tracks elapsed time outside the worker prompt. `wait_agent`'s
`timeout_ms` limits one wait and **does not stop the worker**. Bound every blocking
operation by the next check/deadline and current tool limits; use short waits
(at most 60 seconds when ongoing communication requires it). At the progress
check inspect actual diff/status/logs, detect looping or scope drift, and narrow
or stop work without extending the original deadline. If a tool's minimum wait
exceeds the remaining budget, skip it and stop the worker.

At the deadline inspect available diff/logs/status immediately. If unfinished,
interrupt/close the native agent, or terminate only the owned exec process/group,
then collect partial results and inspect again after it stops. Do not silently extend the same assignment or restart it with a fresh clock.
The parent can take over or define a smaller subtask within the existing scope;
respect an explicit user limit on total time or cost.
Prompt text saying “10 minutes” and a timed-out wait are not enforced timeouts.
Native supervision needs an active parent; if the parent cannot remain available
until the deadline, do not promise a hard stop. Use an authorized externally
watched exec process when a process-level hard limit is required.

## Reclaim and review

Use runtime metadata/events or actual session/request logs to confirm the child
provider, wire model, catalog ID when exposed, and effort. Launch acceptance,
process existence, and a worker's assertion are not independent identity proof.
If identity remains unknown or differs from the requested tuple, interrupt/stop
and report it; do not silently reroute or accept unverified work as that model's
result. If identity is uncertain and verification needs a live child, begin with bounded
read-only discovery before giving it implementation work.

After completion or timeout, review changes against the baseline and write set,
inspect the implementation and relevant real behavior, and run only the checks
needed for the remaining uncertainty. Preserve unrelated user changes; resolve
issues yourself within the authorized scope. A worker final message, successful
launch, or passing unit test alone does not establish task completion. Report
requested/observed identity, elapsed time and stop reason, reviewed changes,
executed checks, and any uncompleted acceptance or runtime validation.
