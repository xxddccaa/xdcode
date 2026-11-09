#!/usr/bin/env python3
"""Verify Plan exit, rate-limit recovery and cancellation through the real app-server.

Uses an isolated home, a local Responses server and a disposable workspace. No
personal configuration, credentials or running daemon is read or changed.
"""

import argparse
import json
import os
from pathlib import Path
from queue import Empty, Queue
import subprocess
import tempfile
from threading import Event, Thread
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import time


class Responses(BaseHTTPRequestHandler):
    phase = "plan"
    captured = []
    limited = 0
    cancel_requested = Event()

    def log_message(self, *_args):
        pass

    def do_POST(self):
        assert self.path == "/v1/responses", self.path
        assert self.headers.get("Authorization") == "Bearer reliability-test-key"
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        tools = {}
        for tool in body.get("tools", []):
            if tool.get("type") == "namespace":
                for member in tool.get("tools", []):
                    tools[member.get("name")] = tool.get("name")
            else:
                tools[tool.get("name")] = None
        if not tools:
            self.respond("Verification session")
            return
        self.captured.append((self.phase, body))
        if self.phase == "cancel" or (self.phase == "implement" and self.limited < 3):
            type(self).limited += 1
            data = json.dumps({"error": {
                "code": "429",
                "message": "Your requests to gpt-6-astra for gpt-6-astra in eastus2 have exceeded token rate limit.",
            }}).encode()
            self.send_response(429)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            # The client must enforce a minimum delay even for zero advice.
            self.send_header("Retry-After", "0")
            self.end_headers()
            self.wfile.write(data)
            if self.phase == "cancel":
                self.cancel_requested.set()
            return
        if self.phase == "plan":
            self.respond("Plan: write the verification file after leaving Plan mode.")
            return
        has_output = any(
            item.get("type") == "function_call_output"
            and item.get("call_id") == "implementation-tool"
            for item in body.get("input", [])
        )
        if has_output:
            self.respond("Implementation complete.")
            return
        command = "printf 'XDCODE_IMPLEMENTED\\n' >> implementation.txt"
        if os.name == "nt":
            command = "Add-Content -LiteralPath implementation.txt -Value XDCODE_IMPLEMENTED -Encoding ascii"
        if "exec_command" in tools:
            name, arguments = "exec_command", {"cmd": command, "max_output_tokens": 100}
        elif "shell_command" in tools:
            name, arguments = "shell_command", {"command": command}
        elif "shell" in tools:
            shell = ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command"] if os.name == "nt" else ["/bin/sh", "-c"]
            name, arguments = "shell", {"command": [*shell, command]}
        else:
            raise AssertionError(f"No shell tool offered: {list(tools)}")
        item = {"type": "function_call", "call_id": "implementation-tool",
                "name": name, "arguments": json.dumps(arguments)}
        if tools[name]:
            item["namespace"] = tools[name]
        self.respond(item)

    def respond(self, output):
        if isinstance(output, str):
            output = {"type": "message", "id": "verification-message", "role": "assistant",
                      "content": [{"type": "output_text", "text": output}]}
        events = [
            {"type": "response.created", "response": {"id": "verification"}},
            {"type": "response.output_item.done", "item": output},
            {"type": "response.completed", "response": {"id": "verification", "usage": {
                "input_tokens": 10, "output_tokens": 10, "total_tokens": 20,
            }}},
        ]
        data = "".join(f"data: {json.dumps(event)}\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


class Client:
    def __init__(self, process):
        self.process = process
        self.sequence = 0
        self.messages = Queue()
        self.pending = []
        Thread(target=self.read, daemon=True).start()

    def read(self):
        for line in self.process.stdout:
            self.messages.put(json.loads(line))
        self.messages.put(None)

    def wait(self, predicate, timeout=90):
        deadline = time.monotonic() + timeout
        for index, message in enumerate(self.pending):
            if predicate(message):
                return self.pending.pop(index)
        while time.monotonic() < deadline:
            try:
                message = self.messages.get(timeout=max(0.01, deadline - time.monotonic()))
            except Empty as error:
                raise AssertionError("Timed out waiting for app-server") from error
            if message is None:
                raise AssertionError("App-server closed stdout")
            if predicate(message):
                return message
            self.pending.append(message)
        raise AssertionError("Timed out waiting for app-server")

    def request(self, method, params):
        self.sequence += 1
        request_id = self.sequence
        self.process.stdin.write(json.dumps({"id": request_id, "method": method, "params": params}) + "\n")
        self.process.stdin.flush()
        reply = self.wait(lambda message: message.get("id") == request_id)
        assert "error" not in reply, reply
        return reply["result"]

    def complete(self, turn_id):
        reply = self.wait(lambda message: message.get("method") == "turn/completed"
                          and message["params"]["turn"]["id"] == turn_id)
        return reply["params"]["turn"]["status"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    with tempfile.TemporaryDirectory(prefix="xdcode-reliability-") as temporary:
        root = Path(temporary)
        home, project = root / "runtime", root / "project"
        home.mkdir()
        project.mkdir()
        server = ThreadingHTTPServer(("127.0.0.1", 0), Responses)
        Thread(target=server.serve_forever, daemon=True).start()
        (home / "config.toml").write_text(f'''
model = "gpt-5.5"
model_provider = "verification"
approval_policy = "never"
sandbox_mode = "danger-full-access"
[model_providers.verification]
name = "Local verification"
base_url = "http://127.0.0.1:{server.server_port}/v1"
env_key = "XDCODE_API_KEY"
wire_api = "responses"
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
[features]
code_mode = false
''')
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("XDCODE_", "CODEX_", "OPENAI_", "OTEL_"))
                and not key.endswith(("_HOME", "_API_KEY", "_TOKEN", "_SECRET"))}
        env.update(XDCODE_HOME=str(home), XDCODE_API_KEY="reliability-test-key",
                   NO_PROXY="127.0.0.1,localhost", no_proxy="127.0.0.1,localhost")
        with (root / "stderr.log").open("w+") as stderr:
            process = subprocess.Popen([binary, "app-server"], cwd=project, env=env,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=stderr, text=True)
            client = Client(process)
            try:
                client.request("initialize", {"clientInfo": {"name": "reliability_test", "version": "1"},
                                              "capabilities": {"experimentalApi": True}})
                thread = client.request("thread/start", {"cwd": str(project)})["thread"]["id"]
                for mode, phase in [("plan", "plan"), ("default", "implement")]:
                    Responses.phase = phase
                    client.request("thread/settings/update", {"threadId": thread, "collaborationMode": {
                        "mode": mode, "settings": {"model": "gpt-5.5", "reasoning_effort": None,
                                                   "developer_instructions": None},
                    }})
                    turn = client.request("turn/start", {"threadId": thread, "input": [
                        {"type": "text", "text": "Plan the change." if mode == "plan" else "Implement the plan.",
                         "text_elements": []},
                    ]})["turn"]["id"]
                    assert client.complete(turn) == "completed"
                    if mode == "plan":
                        assert not (project / "implementation.txt").exists()
                assert (project / "implementation.txt").read_text() == "XDCODE_IMPLEMENTED\n"
                assert Responses.limited == 3
                for phase, expected in [("plan", "# Plan Mode"),
                                        ("implement", "# Collaboration Mode: Default")]:
                    body = next(body for captured_phase, body in Responses.captured if captured_phase == phase)
                    mode_instructions = [part["text"] for item in body["input"]
                                         if item.get("role") == "developer"
                                         for part in item.get("content", [])
                                         if "<collaboration_mode>" in part.get("text", "")]
                    assert mode_instructions, f"Missing mode instructions in {phase}"
                    assert f"<collaboration_mode>{expected}" in mode_instructions[-1], phase
                Responses.phase = "cancel"
                turn = client.request("turn/start", {"threadId": thread, "input": [
                    {"type": "text", "text": "Wait for the provider.", "text_elements": []},
                ]})["turn"]["id"]
                assert Responses.cancel_requested.wait(10)
                started = time.monotonic()
                client.request("turn/interrupt", {"threadId": thread, "turnId": turn})
                assert client.complete(turn) == "interrupted"
                elapsed = time.monotonic() - started
                assert elapsed < 5, elapsed
                print(json.dumps({"result": "passed", "plan_to_default": True, "file_written_once": True,
                                  "consecutive_rate_limits_recovered": 3, "cancel_seconds": round(elapsed, 3)}))
            except BaseException:
                stderr.seek(0)
                print(stderr.read()[-6000:])
                raise
            finally:
                process.stdin.close()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=5)
                server.shutdown()


if __name__ == "__main__":
    main()
