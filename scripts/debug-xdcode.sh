#!/bin/bash

# Set "chatgpt.cliExecutable": "/Users/<USERNAME>/code/xdcode/scripts/debug-xdcode.sh" in VSCode settings to always get the 
# latest xdcode-rs binary when debugging Xdcode Extension.


set -euo pipefail

XDCODE_RS_DIR=$(realpath "$(dirname "$0")/../xdcode-rs")
(cd "$XDCODE_RS_DIR" && cargo run --quiet --bin xdcode -- "$@")