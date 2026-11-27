# npm releases

Use the staging helper in the repo root to generate npm tarballs for a release. For
example, to stage the CLI, responses proxy, and SDK packages for version `0.6.0`:

```bash
./scripts/stage_npm_packages.py \
  --release-version 0.6.0 \
  --package xdcode \
  --package xdcode-responses-api-proxy \
  --package xdcode-sdk
```

This downloads the required native package archive artifacts, hydrates `vendor/` for
each package, and writes tarballs to `dist/npm/`.

When `--package xdcode` is provided, the staging helper builds the lightweight
`@xxddccaa/xdcode` meta package plus all platform-native `@xxddccaa/xdcode` variants
that are later published under platform-specific dist-tags.

The npm variants omit the standalone `xdcode-exec-server` executable and its
`execServer` manifest entry to keep uploads within npm's package size limit.
They retain `xdcode exec-server` through the CLI, the code-mode host, and the
platform sandbox helpers. Canonical release archives retain the standalone
executor; npm staging changes only the package copy.

Direct `build_npm_package.py` invocations are still useful for package-specific
debugging, but native packages expect `--vendor-src` to point at a prehydrated
`vendor/` tree. Release packaging should use `scripts/stage_npm_packages.py`
