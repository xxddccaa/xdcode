# xdcode delivery workflow

`xdcode.yml` validates the offline package and installer. `delivery-checks`
runs the shared unit tests and static checks. Each manually enabled platform
then has a separate `*-build` job and `*-verify` job. Build jobs compile and
upload the platform artifact immediately; verify jobs download that artifact
and run the real installer, tool/MCP/resume, and Plan/rate-limit/cancellation
checks against it. A failed verification can be rerun without rebuilding.
Build jobs install the version in `xdcode-rs/rust-toolchain.toml`. Package
jobs require `delivery-checks` to pass. The workflow does not deploy a service
or update an installed application.

To prepare release assets, push the clean source and a version/build tag, then
dispatch this workflow at that tag with the desired platform inputs. Enable
`publish_release` to create a release after every selected platform's verify
job succeeds. Windows-only publication uses `build_windows=true` with both
other platform inputs false and does not replace the latest Linux/macOS
release. Download the successful artifacts, verify their checksums and source
commit, and attach them to the matching GitHub Release. Linux packages built
locally through the documented workflow may be attached when they use the same
commit.

Use the root delivery guide for local builds and `UPSTREAM_UPGRADE.md` for
AI-assisted feature transplants. Historical automation is recoverable from Git.
