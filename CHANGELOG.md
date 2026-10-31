# xdcode changelog

This file records xdcode changes. [upstream.json](upstream.json) identifies the
selected baseline and source commit; [MODIFICATIONS.md](MODIFICATIONS.md) and
[NOTICE](NOTICE) record provenance and attribution. The product version follows
the recorded baseline, and a separate build ID distinguishes personal revisions.
See the [delivery guide](docs/xdcode.md) for source receipts and package checks.

## Unreleased

- Retire obsolete documentation redirects, hosted login and funding guidance,
  the inherited contribution agreement, archived workflows and their issue
  automation prompts and ownership rules.
- Make delivery, configuration, security reporting and issue templates describe
  xdcode. Provider examples use `requires_hosted_auth = false`; application
  identity remains xdcode across model and provider choices.
- Document the root [AI upgrade guide](UPSTREAM_UPGRADE.md), source and build
  identity, and the distinction between static checks and runtime acceptance.
- Retain the command lifecycle event fix for unified exec process creation
  failures, including approval rejection and sandbox retry behavior. Original
  contribution credit: @Marvinthebored, issue #44557 in the repository recorded
  by `upstream.json`.

These entries describe source changes, not a published or validated package.
Each delivered build must record its actual source commit, source digest, build
ID and binary checksums. Selected backports do not imply adoption of an entire
newer baseline.
