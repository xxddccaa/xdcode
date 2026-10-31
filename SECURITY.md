# xdcode security policy

Report security issues in xdcode to this repository's maintainer through a
private channel you already share. If you have no private contact, open an issue
requesting a private reporting channel without publishing vulnerability details.
Private vulnerability reporting can be used if it is enabled for this
repository. This project does not promise a bounty or a response deadline.

Include the affected `xdcode --version` output, package build ID and source
commit from `build-info.json`, operating system, impact and a minimal
reproduction. Remove credentials, private prompts, repository contents and
personal data from diagnostics. Coordinate disclosure with the maintainer
before posting exploit details publicly.

## Operating xdcode safely

Use the approvals and sandbox settings appropriate to the workspace. Review
commands and requested access before approving them. MCP servers, skills, hooks
and configured model providers may execute code or receive workspace data;
enable only those you trust and inspect the access they need. Sandboxing the
agent does not establish a security boundary around an independently operated
remote service.

Keep provider keys in a secret manager or the environment, and keep separate
`XDCODE_HOME` directories for independent runtimes. Avoid exposing local
app-server or daemon endpoints to untrusted clients. Share only redacted logs
and diagnostic reports.

Follow the [delivery guide](docs/xdcode.md) for trusted offline installation,
checksum verification, native dependencies and manual upgrades. Checksums
detect mismatched bytes but do not authenticate a publisher. Finish active
xdcode tasks and stop its processes before installing. Keep data backups when
changing builds; retaining an older binary does not reverse a data migration.

Source and dependency attribution is recorded in [NOTICE](NOTICE),
[MODIFICATIONS.md](MODIFICATIONS.md) and [upstream.json](upstream.json). Report
issues against the actual xdcode source and build involved.
