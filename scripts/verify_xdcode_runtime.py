#!/usr/bin/env python3
"""Exercise the installed agent with a local Responses server and isolated state.

No production credentials or model service are used. Checks one real tool loop,
session persistence, naming, legacy-home isolation and removed CLI entry points.
"""

import argparse
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread


class ModelServer(BaseHTTPRequestHandler):
    requests = []
    tools_called = 0
    mcp_called = 0

    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.send_response(404)
        self.end_headers()

    def do_POST(self):
        if self.path != "/v1/responses":
            self.send_response(404)
            self.end_headers()
            return
        assert self.headers.get("Authorization") == "Bearer xdcode-local-test-key"
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.requests.append(body)
        response_id = f"resp_{len(self.requests)}"
        events = [{"type": "response.created", "response": {"id": response_id}}]
        call_results = [item for item in body.get("input", []) if item.get("type") in ("function_call_output", "custom_tool_call_output")]
        tools = {}
        offered_tools = list(body.get("tools") or [])
        for item in body.get("input", []):
            if item.get("type") == "additional_tools":
                offered_tools.extend(item.get("tools", []))
        for tool in offered_tools:
            if tool.get("type") == "namespace":
                for member in tool.get("tools", []):
                    tools[member.get("name")] = tool.get("name")
            else:
                tools[tool.get("name")] = None
        if not tools:
            # Session-title generation shares the provider but has no tools.
            # It must not consume the scripted agent conversation.
            self.requests.remove(body)
            events.append({"type": "response.output_item.done", "item": {
                "id": "msg_title", "type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": '{"title":"xdcode smoke"}'}],
            }})
        elif not call_results and self.tools_called == 0:
            self.__class__.tools_called += 1
            if "exec_command" in tools:
                name, arguments = "exec_command", {"cmd": "printf xdcode-tool-ok", "max_output_tokens": 100}
            elif "shell_command" in tools:
                name, arguments = "shell_command", {"command": "printf xdcode-tool-ok"}
            elif "shell" in tools:
                name, arguments = "shell", {"command": ["/bin/sh", "-c", "printf xdcode-tool-ok"]}
            elif "exec" in tools:
                name, arguments = "exec", None
            else:
                raise AssertionError(f"No shell tool offered: {list(tools)}")
            item = {
                "type": "function_call", "call_id": "call_xdcode_smoke",
                "name": name, "arguments": json.dumps(arguments),
            }
            if name == "exec":
                item = {"type": "custom_tool_call", "call_id": "call_xdcode_smoke", "name": "exec",
                        "input": 'text(JSON.stringify(await tools.exec_command({cmd:"printf xdcode-tool-ok",max_output_tokens:100})));'}
            if tools[name]:
                item["namespace"] = tools[name]
            events.append({"type": "response.output_item.done", "item": item})
        elif self.mcp_called == 0:
            if not any("xdcode-tool-ok" in str(item.get("output", "")) for item in call_results):
                print("Unexpected shell result:", json.dumps(call_results), flush=True)
                self.send_error(400, "verification shell tool failed")
                return
            name = next((name for name in tools if name and (name == "ping" or name.endswith("__ping"))), None)
            assert name or "exec" in tools, f"MCP ping was not offered: {tools}"
            self.__class__.mcp_called += 1
            if not name:
                name = "exec"
                item = {"type": "custom_tool_call", "call_id": "call_xdcode_mcp", "name": "exec",
                        "input": 'const ping=tools.mcp__smoke__ping ?? tools.smoke__ping; text(JSON.stringify(await ping({})));'}
            else:
                item = {"type": "function_call", "call_id": "call_xdcode_mcp", "name": name, "arguments": "{}"}
            if tools[name]:
                item["namespace"] = tools[name]
            events.append({"type": "response.output_item.done", "item": item})
        else:
            if self.tools_called:
                assert any("xdcode-tool-ok" in str(item.get("output", "")) for item in call_results), call_results
            assert any("xdcode-mcp-ok" in str(item.get("output", "")) for item in call_results), call_results
            events.append({"type": "response.output_item.done", "item": {
                "id": "msg_xdcode_smoke", "type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": "xdcode smoke verified"}],
            }})
        events.append({"type": "response.completed", "response": {
            "id": response_id, "status": "completed",
            "usage": {"input_tokens": 20, "output_tokens": 10, "total_tokens": 30},
        }})
        payload = "".join(f"data: {json.dumps(event)}\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def checked(command, env, cwd, expected=0):
    result = subprocess.run(command, env=env, cwd=cwd, stdin=subprocess.DEVNULL, text=True, capture_output=True, timeout=120)
    if result.returncode != expected:
        raise AssertionError(f"{command[:3]} exited {result.returncode}:\n{result.stdout[-6000:]}\n{result.stderr[-6000:]}")
    return result


def mcp_server():
    for line in sys.stdin:
        request = json.loads(line)
        if "id" not in request:
            continue
        method = request.get("method")
        if method == "initialize":
            result = {"protocolVersion": request["params"]["protocolVersion"],
                      "capabilities": {"tools": {}}, "serverInfo": {"name": "xdcode-smoke", "version": "1"}}
        elif method == "tools/list":
            result = {"tools": [{"name": "ping", "description": "Return a local verification marker.",
                                  "annotations": {"readOnlyHint": True, "destructiveHint": False, "openWorldHint": False},
                                  "inputSchema": {"type": "object", "properties": {}}}]}
        elif method == "tools/call":
            result = {"content": [{"type": "text", "text": "xdcode-mcp-ok"}]}
        elif method == "ping":
            result = {}
        else:
            print(json.dumps({"jsonrpc": "2.0", "id": request["id"],
                              "error": {"code": -32601, "message": "Unknown method"}}), flush=True)
            continue
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--temp-root", type=Path)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    with tempfile.TemporaryDirectory(prefix="xdcode-verification-", dir=args.temp_root) as temporary:
        root = Path(temporary).resolve()
        runtime = root / "runtime"
        legacy = root / "legacy"
        project = root / "project"
        for directory in (runtime, legacy, project):
            directory.mkdir()
        skill = runtime / "skills/xdcode-smoke-skill"
        skill.mkdir(parents=True)
        (skill / "SKILL.md").write_text("---\nname: xdcode-smoke-skill\ndescription: Local xdcode verification skill.\n---\nUse local tools for verification.\n")
        sentinel = legacy / "config.toml"
        sentinel.write_text('this must never be parsed = [\n')
        server = ThreadingHTTPServer(("127.0.0.1", 0), ModelServer)
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        config = f'''model_provider = "local_test"
model = "gpt-5.6-sol"
approval_policy = "never"
sandbox_mode = "workspace-write"
[model_providers.local_test]
name = "Local verification"
base_url = "http://127.0.0.1:{server.server_port}/v1"
env_key = "XDCODE_API_KEY"
wire_api = "responses"
supports_websockets = false
[features]
code_mode = false
[mcp_servers.smoke]
command = {json.dumps(sys.executable)}
args = [{json.dumps(str(Path(__file__).resolve()))}, "--mcp-server"]
startup_timeout_sec = 10
default_tools_approval_mode = "approve"
'''
        (runtime / "config.toml").write_text(config)
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("CODEX_", "OPENAI_", "OTEL_", "XDCODE_"))
                and not key.endswith(("_HOME", "_API_KEY", "_TOKEN", "_SECRET"))}
        env.update({"XDCODE_HOME": str(runtime), "XDCODE_API_KEY": "xdcode-local-test-key",
                    "CODEX_HOME": str(legacy), "NO_PROXY": "127.0.0.1,localhost", "no_proxy": "127.0.0.1,localhost"})
        try:
            version = checked([binary, "--version"], env, project).stdout.strip()
            assert version.startswith("xdcode "), version
            help_text = checked([binary, "--help"], env, project).stdout
            # Unrecognized bare words are valid initial prompts, so inspect the
            # registered command list rather than treating prompts as commands.
            for command in ("login", "logout", "cloud", "remote-control", "update"):
                assert not re.search(rf"^  {re.escape(command)}(?:\s|$)", help_text, re.MULTILINE), command
            result = checked([binary, "exec", "--skip-git-repo-check", "--json",
                              "Run the requested read-only shell tool and report its result."], env, project)
            assert "xdcode smoke verified" in result.stdout, result.stdout
            assert len(ModelServer.requests) >= 2, "tool output was not sent back to the model"
            assert ModelServer.tools_called == 1
            assert ModelServer.mcp_called == 1
            assert "xdcode-smoke-skill" in json.dumps(ModelServer.requests[0]), "skills catalog missing"
            assert "Your model and model provider are configurable backend details" in json.dumps(ModelServer.requests[0]), "application identity was not sent to the model"
            requests_before_resume = len(ModelServer.requests)
            resumed = checked([binary, "exec", "--skip-git-repo-check", "--json",
                               "resume", "--last", "Continue the previous verification."], env, project)
            assert "xdcode smoke verified" in resumed.stdout, resumed.stdout
            assert len(ModelServer.requests) > requests_before_resume, "resume did not call the model"
            assert list(legacy.iterdir()) == [sentinel], "legacy home was mutated"
            assert sentinel.read_text() == 'this must never be parsed = [\n'
            assert any(runtime.rglob("*.jsonl")) or any(runtime.glob("*.sqlite")), "session not persisted"
            print(json.dumps({"version": version, "requests": len(ModelServer.requests),
                              "tool_loop": "passed", "legacy_home_isolation": "passed",
                              "removed_commands": "passed", "session_persistence": "passed",
                              "resume": "passed", "mcp_tool": "passed", "skills_catalog": "passed"}))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


if __name__ == "__main__":
    if sys.argv[1:] == ["--mcp-server"]:
        mcp_server()
    else:
        main()
