#!/usr/bin/env bash
set -euo pipefail

release_base=https://github.com/xxddccaa/xdcode/releases
release=${XDCODE_RELEASE:-latest}
force=0
temporary=""
staging=""
lock_directory=""

fail() {
  printf 'xdcode install: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$staging" ]]; then
    rm -rf -- "$staging"
  fi
  if [[ -n "$lock_directory" ]]; then
    rm -f -- "$lock_directory/xdcode"
    rmdir "$lock_directory"
  fi
  if [[ -n "$temporary" ]]; then
    rm -rf -- "$temporary"
  fi
  exit "$status"
}

usage() {
  cat <<'EOF'
Usage: bash install.sh [--force] [--release vVERSION-gCOMMIT]

Installs the latest published xdcode release for Linux x86_64 or macOS ARM64.
--force allows existing xdcode processes; it does not stop them or replace builds.
--release pins an exact release instead of following GitHub's latest release.
Linux requires Python 3.11+. macOS uses the system's plutil and checksum tools.
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

safe_member() {
  local member=${1%/}
  [[ "$member" =~ ^[A-Za-z0-9._/-]+$ ]] || fail "unsafe archive member"
  case "/$member/" in
    *'/../'* | *'/./'* | *'//'*) fail "unsafe archive member: $member" ;;
  esac
}

verify_archive() {
  local contents expected
  contents="$(cat "$temporary/$archive_name.sha256")"
  expected=${contents:0:64}
  [[ "$expected" =~ ^[a-f0-9]{64}$ && "$contents" == "$expected  $archive_name" ]] ||
    fail "checksum sidecar does not name the selected archive"
  [[ "$(hash_file "$temporary/$archive_name")" == "$expected" ]] ||
    fail "archive checksum mismatch"
  tar -tzf "$temporary/$archive_name" > "$temporary/members"
  tar -tvzf "$temporary/$archive_name" > "$temporary/member-types"
  awk 'substr($0, 1, 1) != "-" && substr($0, 1, 1) != "d" { exit 1 }' \
    "$temporary/member-types" || fail "archive contains links or special files"
  [[ -z "$(awk '{print tolower($0)}' "$temporary/members" | sort | uniq -d)" ]] || fail "duplicate archive members"
  local member
  while IFS= read -r member; do
    safe_member "$member"
  done < "$temporary/members"
}

json_value() {
  plutil -extract "$2" raw -o - "$1"
}

verify_macos_metadata() {
  local directory=$1 source_commit
  [[ "$(json_value "$directory/build-info.json" schema_version)" == 1 &&
     "$(json_value "$directory/build-info.json" version)" == "$version" &&
     "$(json_value "$directory/build-info.json" build_id)" == "$build_id" &&
     "$(json_value "$directory/build-info.json" source_dirty)" == false &&
     "$(json_value "$directory/build-info.json" target)" == aarch64-apple-darwin ]] ||
    fail "package metadata does not match the selected release"
  source_commit="$(json_value "$directory/build-info.json" source_commit)"
  [[ "$source_commit" =~ ^[a-f0-9]{40}$ && "g${source_commit:0:12}" == "$build_id" ]] ||
    fail "package source commit does not match the release tag"
  [[ "$(json_value "$directory/xdcode-package.json" layoutVersion)" == 1 &&
     "$(json_value "$directory/xdcode-package.json" version)" == "$version" &&
     "$(json_value "$directory/xdcode-package.json" buildId)" == "$build_id" &&
     "$(json_value "$directory/xdcode-package.json" target)" == aarch64-apple-darwin &&
     "$(json_value "$directory/xdcode-package.json" variant)" == xdcode &&
     "$(json_value "$directory/xdcode-package.json" entrypoint)" == bin/xdcode &&
     "$(json_value "$directory/xdcode-package.json" resourcesDir)" == xdcode-resources &&
     "$(json_value "$directory/xdcode-package.json" pathDir)" == xdcode-path ]] ||
    fail "invalid package layout metadata"
}

verify_macos_payload() {
  local directory=$1 line digest member
  [[ -d "$directory/bin" && -d "$directory/xdcode-resources" && -d "$directory/xdcode-path" ]] ||
    fail "missing package directories"
  [[ -f "$directory/SHA256SUMS" && ! -L "$directory/SHA256SUMS" ]] ||
    fail "missing package checksums"
  : > "$temporary/expected-files"
  while IFS= read -r line || [[ -n "$line" ]]; do
    digest=${line:0:64}
    member=${line:66}
    [[ "$digest" =~ ^[a-f0-9]{64}$ && "$line" == "$digest  $member" ]] ||
      fail "invalid package checksum entry"
    safe_member "$member"
    [[ "$member" != SHA256SUMS && -f "$directory/$member" && ! -L "$directory/$member" ]] ||
      fail "missing or unsafe package file: $member"
    printf '%s\n' "$member" >> "$temporary/expected-files"
  done < "$directory/SHA256SUMS"
  [[ -s "$temporary/expected-files" && -z "$(sort "$temporary/expected-files" | uniq -d)" ]] ||
    fail "empty or duplicate package checksums"
  (cd "$directory"; find . -type f ! -path ./SHA256SUMS | sed 's|^\./||' | sort) > "$temporary/actual-files"
  sort "$temporary/expected-files" > "$temporary/sorted-files"
  cmp -s "$temporary/sorted-files" "$temporary/actual-files" || fail "unlisted package files"
  [[ -z "$(find "$directory" -type l -print)" ]] || fail "installed package contains symlinks"
  (cd "$directory"; shasum -a 256 -c SHA256SUMS) || fail "package checksum mismatch"
  verify_macos_metadata "$directory"
}

require_macos_idle() {
  if [[ "$force" == 1 ]]; then
    return
  fi
  local status=0
  pgrep -u "$(id -u)" -f '(^|/)(xdcode|xdcode-code-mode-host|xdcode-app-server)([[:space:]]|$)' \
    >/dev/null || status=$?
  [[ "$status" == 1 ]] || fail "active xdcode processes or process inspection failed; finish tasks or pass --force"
}

check_macos_entry() {
  local target previous
  if [[ ! -e "$entry" && ! -L "$entry" ]]; then
    return
  fi
  [[ -L "$entry" ]] || fail "refusing to replace a non-managed entry: $entry"
  target="$(readlink "$entry")"
  previous=${target%/bin/xdcode}
  [[ "$target" == "$previous/bin/xdcode" && "${previous%/*}" == "$install_root" &&
     -d "$previous" && ! -L "$previous" && -f "$target" &&
     -f "$previous/build-info.json" && -f "$previous/xdcode-package.json" ]] ||
    fail "existing xdcode entry does not point to a recognized build in $install_root"
  [[ "$(json_value "$previous/build-info.json" target)" == aarch64-apple-darwin &&
     "$(json_value "$previous/xdcode-package.json" entrypoint)" == bin/xdcode ]] ||
    fail "existing xdcode entry has invalid metadata"
}

install_macos() {
  local install_root="$HOME/.local/lib/xdcode" bin_directory="$HOME/.local/bin"
  local entry="$bin_directory/xdcode" destination="$install_root/$identity"
  [[ ! -L "$install_root" && ! -L "$bin_directory" ]] || fail "installation directories must not be symlinks"
  mkdir -p "$install_root" "$bin_directory"
  [[ "$(cd "$install_root" && pwd -P)" == "$install_root" &&
     "$(cd "$bin_directory" && pwd -P)" == "$bin_directory" ]] ||
    fail "installation path contains a symlink or traversal"
  mkdir "$install_root/.macos-install.lock" || fail "another macOS installation holds the lock"
  lock_directory="$install_root/.macos-install.lock"
  require_macos_idle
  check_macos_entry
  staging="$(mktemp -d "$install_root/.staging.XXXXXX")"
  tar -xzf "$temporary/$archive_name" -C "$staging"
  verify_macos_payload "$staging"
  local binary argument actual_version description
  for binary in bin/xdcode bin/xdcode-code-mode-host xdcode-path/rg; do
    [[ -x "$staging/$binary" ]] || fail "missing executable: $binary"
    description="$(LC_ALL=C file -b "$staging/$binary")"
    [[ "$description" == 'Mach-O 64-bit'* && "$description" == *arm64* && "$description" == *executable* ]] ||
      fail "package executable is not native ARM64: $binary"
    argument=--help
    [[ "$binary" != bin/xdcode && "$binary" != xdcode-path/rg ]] || argument=--version
    actual_version="$(XDCODE_HOME="$temporary/probe-home" "$staging/$binary" "$argument")"
    [[ "$binary" != bin/xdcode || "$actual_version" == "xdcode $version" ]] ||
      fail "native CLI version differs from release metadata"
  done
  if [[ -e "$destination" || -L "$destination" ]]; then
    [[ -d "$destination" && ! -L "$destination" ]] || fail "unsafe existing build directory"
    cmp -s "$staging/SHA256SUMS" "$destination/SHA256SUMS" || fail "existing build differs; never overwritten"
    verify_macos_payload "$destination"
  else
    mv "$staging" "$destination"
    staging=""
  fi
  require_macos_idle
  check_macos_entry
  [[ ! -d "$entry" ]] || fail "existing entry points to a directory"
  ln -s "$destination/bin/xdcode" "$lock_directory/xdcode"
  mv -f "$lock_directory/xdcode" "$entry"
  printf 'Installed %s\nEntrypoint: %s\n' "$identity" "$entry"
}

install_linux() {
  local interpreter="" candidate
  for candidate in python3.11 python3; do
    if command -v "$candidate" >/dev/null 2>&1 &&
       "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 11))'; then
      interpreter=$candidate
      break
    fi
  done
  [[ -n "$interpreter" ]] || fail "Linux installation requires Python 3.11 or newer"
  [[ "$(awk '$0 == "install.py" { count++ } END { print count+0 }' "$temporary/members")" == 1 ]] ||
    fail "missing or duplicate archived offline installer"
  tar -xOf "$temporary/$archive_name" install.py > "$temporary/install.py"
  "$interpreter" - "$temporary/$archive_name" "$version" "$build_id" <<'PY'
import json
import re
import sys
import tarfile

with tarfile.open(sys.argv[1]) as archive:
    member = archive.getmember("build-info.json")
    if not member.isfile() or member.size > 1024 * 1024:
        sys.exit("xdcode install: unsafe Linux build metadata")
    with archive.extractfile(member) as stream:
        info = json.load(stream)
source = info.get("source_commit", "")
if not (
    type(info.get("schema_version")) is int and info["schema_version"] == 1
    and info.get("version") == sys.argv[2] and info.get("build_id") == sys.argv[3]
    and info.get("source_dirty") is False and info.get("target") == "x86_64-unknown-linux-gnu"
    and isinstance(source, str) and re.fullmatch(r"[a-f0-9]{40}", source)
    and "g" + source[:12] == sys.argv[3]
):
    sys.exit("xdcode install: Linux package metadata does not match the release tag")
PY
  local arguments=("$temporary/install.py")
  [[ "$force" != 1 ]] || arguments+=(--force)
  "$interpreter" "${arguments[@]}" "$temporary/$archive_name"
}

main() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --force) force=1 ;;
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
  for tool in curl tar awk sort uniq cmp; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: $tool"
  done
  command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || fail "a SHA-256 tool is required"
  if [[ "$platform" == macos-arm64 ]]; then
    for tool in plutil shasum pgrep file; do
      command -v "$tool" >/dev/null 2>&1 || fail "missing macOS system tool: $tool"
    done
  fi
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
  build_id=g${identity##*-g}
  archive_name="xdcode-$identity-$platform.tar.gz"
  temporary="$(mktemp -d)"
  trap cleanup EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  printf 'Installing %s for %s\n' "$release" "$platform"
  if [[ "$force" == 1 ]]; then
    printf 'Warning: --force keeps existing processes on their current builds; restart them after tasks finish.\n' >&2
  fi
  curl_request --output "$temporary/$archive_name" "$release_base/download/$release/$archive_name"
  curl_request --max-filesize 1024 --output "$temporary/$archive_name.sha256" "$release_base/download/$release/$archive_name.sha256"
  verify_archive
  if [[ "$platform" == linux-x86_64 ]]; then
    install_linux
  else
    install_macos
  fi
  printf 'Old builds, configuration and sessions retained. Ensure $HOME/.local/bin is on PATH.\n'
}

main "$@"
