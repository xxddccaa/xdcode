#!/usr/bin/env python3
"""Check startup identity and model preservation in a real terminal session."""

import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import tempfile
import termios
import time
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    # Native helper aliases require a user-owned parent rather than shared /tmp.
    with tempfile.TemporaryDirectory(prefix="xdcode-ui-", dir=Path.home()) as temporary:
        runtime = Path(temporary)
        project = runtime / "project"
        project.mkdir()
        config = runtime / "config.toml"
        config.write_text(
            'model = "gpt-5.6-sol"\nmodel_provider = "verification"\n'
            '[model_providers.verification]\nname = "Local verification"\n'
            'base_url = "http://127.0.0.1:9/v1"\nenv_key = "XDCODE_API_KEY"\nwire_api = "responses"\n'
            f'[projects.{json.dumps(str(project))}]\ntrust_level = "trusted"\n'
            '[tui]\nscreen_reader_detection_done = true\n',
            encoding="utf-8",
        )
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("CODEX_", "OPENAI_", "OTEL_", "XDCODE_"))
                and not key.endswith(("_HOME", "_API_KEY", "_TOKEN", "_SECRET"))}
        env.update({"TERM": "xterm-256color", "XDCODE_HOME": str(runtime), "XDCODE_API_KEY": "local-ui-test"})
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
        process = subprocess.Popen([str(args.binary.resolve()), "--no-daemon", "--no-alt-screen"],
                                   cwd=project, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        os.close(slave)
        output = bytearray()
        started = time.monotonic()
        try:
            while time.monotonic() - started < 20 and process.poll() is None:
                if select.select([master], [], [], 0.2)[0]:
                    try:
                        chunk = os.read(master, 65536)
                    except OSError:
                        break
                    output.extend(chunk)
                    for query, answer in ((b"\x1b[6n", b"\x1b[1;1R"), (b"\x1b[?u", b"\x1b[?0u"), (b"\x1b[c", b"\x1b[?1;2c")):
                        if query in chunk:
                            os.write(master, answer)
                if b"gpt-5.6-sol" in output.lower() and time.monotonic() - started > 6:
                    break
        finally:
            if process.poll() is None:
                os.write(master, b"\x03\x03")
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=5)
            os.close(master)
        text = output.decode("utf-8", "replace")
        text = re.sub(r"\x1b\](?:[^\x07\x1b]|\x1b(?!\\))*(?:\x07|\x1b\\)", "", text)
        text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
        if args.output:
            args.output.write_text(text, encoding="utf-8")
        preserved = tomllib.loads(config.read_text())["model"] == "gpt-5.6-sol"
        promotion = "Try new model" in text or "Meet GPT" in text
        visible = "gpt-5.6-sol" in text.lower() and "xdcode" in text.lower()
        legacy = re.search(r"\bcodex\b|openai", text, re.IGNORECASE) is not None
        success = visible and preserved and not promotion and not legacy
        print(json.dumps({"tui": "passed" if success else "failed", "model_preserved": preserved,
                          "model_visible": visible, "promotion_present": promotion, "legacy_brand": legacy}))
        if not success:
            print(text[-3000:])
            raise SystemExit(1)


if __name__ == "__main__":
    main()
