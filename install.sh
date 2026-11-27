#!/usr/bin/env bash
# One command installs or upgrades xdcode on Linux x86_64 and macOS ARM64.
# Needs only bash, curl, tar and a SHA-256 tool (no Python). Re-run it to upgrade.
set -euo pipefail

release_base=https://github.com/xxddccaa/xdcode/releases
release=${XDCODE_RELEASE:-latest}
temporary=""
staging=""
new_link=""

fail() {
  printf 'xdcode install: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  local status=$?
  trap - EXIT
  [[ -z "$staging" ]] || rm -rf -- "$staging"
  [[ -z "$new_link" ]] || rm -f -- "$new_link"
  [[ -z "$temporary" ]] || rm -rf -- "$temporary"
  exit "$status"
}

usage() {
  cat <<'EOF'
Usage: bash install.sh [--release vVERSION-gCOMMIT]

Installs or upgrades xdcode (Linux x86_64, macOS ARM64) into ~/.local and points
~/.local/bin/xdcode at it. Run the same command again to upgrade. Running sessions
keep their current build; new sessions use the new one. Old builds, configuration
and sessions are kept. Requires bash, curl, tar and sha256sum or shasum.

--release pins an exact release instead of GitHub's latest release.
XDCODE_INSTALL_PROXY optionally overrides the normal curl proxy environment.
Windows users should use install.ps1 from the same repository instead.
EOF
}

curl_request() {
  local arguments=(--fail --show-error --location --proto '=https' --proto-redir '=https'
    --connect-timeout 15 --retry 2 --max-time 1800)
  if [[ -n "${XDCODE_INSTALL_PROXY:-}" ]]; then
    arguments+=(--proxy "$XDCODE_INSTALL_PROXY")
  fi
  curl "${arguments[@]}" "$@"
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

verify_archive() {
  local contents expected member
  contents="$(cat "$temporary/$archive_name.sha256")"
  expected=${contents:0:64}
  [[ "$expected" =~ ^[a-f0-9]{64}$ && "$contents" == "$expected  $archive_name" ]] ||
    fail "checksum file does not name the selected archive"
  [[ "$(hash_file "$temporary/$archive_name")" == "$expected" ]] || fail "archive checksum mismatch"
  # Only plain files and directories with simple relative names may be extracted.
  tar -tvzf "$temporary/$archive_name" |
    awk 'substr($0, 1, 1) != "-" && substr($0, 1, 1) != "d" { exit 1 }' ||
    fail "archive contains links or special files"
  while IFS= read -r member; do
    member=${member%/}
    [[ "$member" =~ ^[A-Za-z0-9._/-]+$ ]] || fail "unsafe archive member"
    case "/$member/" in
      *'/../'* | *'/./'* | *'//'*) fail "unsafe archive member: $member" ;;
    esac
  done < <(tar -tzf "$temporary/$archive_name")
}

main() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --force) ;; # Accepted for older instructions; installing never stops running sessions.
      --release)
        [[ $# -ge 2 ]] || fail "--release requires a tag"
        release=$2
        shift
        ;;
      -h | --help) usage; return ;;
      *) fail "unknown argument: $1" ;;
    esac
    shift
  done

  local host platform tool
  host="$(uname -s)-$(uname -m)"
  if [[ "$host" == Darwin-x86_64 && "$(sysctl -in sysctl.proc_translated 2>/dev/null || true)" == 1 ]]; then
    host=Darwin-arm64
  fi
  case "$host" in
    Linux-x86_64) platform=linux-x86_64 ;;
    Darwin-arm64) platform=macos-arm64 ;;
    MINGW* | MSYS* | CYGWIN*) fail "use install.ps1 from PowerShell for native Windows" ;;
    *) fail "no published package for $host; supported: Linux x86_64, macOS ARM64" ;;
  esac
  for tool in curl tar awk; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: $tool"
  done
  command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 ||
    fail "a SHA-256 tool (sha256sum or shasum) is required"
  [[ "$HOME" == /* ]] || fail "HOME must be an absolute path"

  if [[ "$release" == latest ]]; then
    local resolved
    resolved="$(curl_request --silent --head --output /dev/null --write-out '%{url_effective}' "$release_base/latest")"
    [[ "$resolved" == "$release_base/tag/"* ]] || fail "unexpected latest-release redirect"
    release=${resolved#"$release_base/tag/"}
  fi
  [[ "$release" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-g[a-f0-9]{12}$ ]] ||
    fail "unsupported release tag: $release"
  identity=${release#v}
  version=${identity%-g*}
  archive_name="xdcode-$identity-$platform.tar.gz"

  local install_root="$HOME/.local/lib/xdcode" bin_directory="$HOME/.local/bin"
  local entry="$HOME/.local/bin/xdcode" destination="$HOME/.local/lib/xdcode/$identity"
  if [[ -e "$entry" && ! -L "$entry" ]]; then
    fail "$entry exists and is not an xdcode link; move it away and rerun"
  fi
  if [[ -L "$entry" && "$(readlink "$entry")" == "$destination/bin/xdcode" && -x "$destination/bin/xdcode" ]]; then
    printf 'xdcode %s is already installed (%s)\n' "$version" "$release"
    return
  fi

  temporary="$(mktemp -d)"
  trap cleanup EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  mkdir -p "$install_root" "$bin_directory"
  if [[ -x "$destination/bin/xdcode" ]]; then
    printf 'Using the already downloaded %s\n' "$release"
  else
    printf 'Installing %s for %s\n' "$release" "$platform"
    curl_request --output "$temporary/$archive_name" "$release_base/download/$release/$archive_name"
    curl_request --max-filesize 1024 --output "$temporary/$archive_name.sha256" \
      "$release_base/download/$release/$archive_name.sha256"
    verify_archive
    staging="$(mktemp -d "$install_root/.staging.XXXXXX")"
    tar --no-same-owner --no-same-permissions -xzf "$temporary/$archive_name" -C "$staging"
    [[ -x "$staging/bin/xdcode" ]] || fail "package has no bin/xdcode"
    local reported
    reported="$(cd "$temporary" && XDCODE_HOME="$temporary/probe-home" "$staging/bin/xdcode" --version 2>/dev/null)" ||
      fail "the downloaded xdcode does not run on this system"
    [[ "$reported" == "xdcode $version" ]] || fail "package reports '$reported', expected 'xdcode $version'"
    mv "$staging" "$destination"
    staging=""
  fi

  # Swap the command link with one rename in the same directory; old builds stay on disk.
  new_link="$bin_directory/.xdcode.new.$$"
  ln -s "$destination/bin/xdcode" "$new_link"
  mv -f "$new_link" "$entry"
  new_link=""
  printf 'Installed xdcode %s\nCommand: %s\n' "$version" "$entry"
  printf 'Running sessions keep their current build; new sessions use this one.\n'
  case ":$PATH:" in
    *":$bin_directory:"*) ;;
    *) printf 'Add %s to PATH, for example: export PATH="$HOME/.local/bin:$PATH"\n' "$bin_directory" ;;
  esac
}

main "$@"
