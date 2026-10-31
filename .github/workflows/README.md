# xdcode delivery workflow

`xdcode.yml` validates the offline package and installer. Its manually enabled
Linux job builds the pinned source, tests the actual installation, and uploads
artifacts. The optional Apple Silicon job builds on macOS 15, checks the package
and its signatures, then runs tools/MCP/resume and Plan/rate-limit/cancellation
acceptance tests. Both builders install the version in
`xdcode-rs/rust-toolchain.toml`. Package jobs require delivery tests to pass.
The workflow does not deploy a service or update an installed application.

To prepare release assets, push the clean source and a version/build tag, then
dispatch this workflow at that tag with `build_macos` and/or `build_package`.
Download the successful artifacts, verify their checksums and source commit,
and attach them to the matching GitHub Release. Linux packages built locally
through the documented workflow may be attached when they use the same commit.

Use the root delivery guide for local builds and `UPSTREAM_UPGRADE.md` for
AI-assisted feature transplants. Historical automation is recoverable from Git.
