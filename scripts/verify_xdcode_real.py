#!/usr/bin/env python3
"""Optional minimal generation smoke against an explicitly configured provider.

Requires XDCODE_API_KEY in the environment. Never persists or prints the key.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--base-url", required=True, help="Explicit Responses provider base URL")
    parser.add_argument("--model", default="gpt-5.6-sol")
    parser.add_argument("--temp-root", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--identity", action="store_true", help="ask the assistant its identity without naming the expected application")
    mode.add_argument("--tools", action="store_true", help="require a real read-only shell tool call")
    args = parser.parse_args()
    api_key = os.environ.get("XDCODE_API_KEY")
    if not api_key:
        raise SystemExit("XDCODE_API_KEY is required")
    with tempfile.TemporaryDirectory(prefix="xdcode-real-verification-", dir=args.temp_root) as temporary:
        runtime = Path(temporary).resolve()
        project = runtime / "project"
        project.mkdir()
        (runtime / "config.toml").write_text(
            f'model = {json.dumps(args.model)}\nmodel_provider = "verification"\n'
            'approval_policy = "never"\nsandbox_mode = "read-only"\n'
            '[model_providers.verification]\nname = "Verification provider"\n'
            f'base_url = {json.dumps(args.base_url)}\nenv_key = "XDCODE_API_KEY"\nwire_api = "responses"\n',
            encoding="utf-8",
        )
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("CODEX_", "OPENAI_", "OTEL_", "XDCODE_"))
                and not key.endswith(("_HOME", "_API_KEY", "_TOKEN", "_SECRET"))}
        env.update({"XDCODE_HOME": str(runtime), "XDCODE_API_KEY": api_key})
        prompt = "Reply with exactly XDCODE_OK. Do not call tools."
        if args.identity:
            prompt = "你是谁？这个助手叫什么名字？请简短回答，不要调用工具。"
        elif args.tools:
            prompt = "请使用命令执行工具运行一次 printf XDCODE_TOOL_OK，然后只回复 XDCODE_OK。"
        result = subprocess.run(
            [str(args.binary.resolve()), "exec", "--skip-git-repo-check", "--json",
             prompt],
            cwd=project, env=env, stdin=subprocess.DEVNULL, text=True, capture_output=True, timeout=120,
        )
        events = []
        for line in result.stdout.splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        messages = [event.get("item", {}).get("text", "") for event in events
                    if event.get("type") == "item.completed" and event.get("item", {}).get("type") == "agent_message"]
        if args.identity:
            answer = "\n".join(messages).strip()
            success = result.returncode == 0 and "xdcode" in answer.lower() and not any(name in answer.lower() for name in ("codex", "openai"))
        else:
            success = result.returncode == 0 and any(message.strip() == "XDCODE_OK" for message in messages)
            if args.tools:
                success = success and any(
                    event.get("item", {}).get("type") == "command_execution"
                    and event.get("item", {}).get("exit_code") == 0
                    and "XDCODE_TOOL_OK" in event.get("item", {}).get("aggregated_output", "")
                    for event in events
                )
        report = {"model": args.model, "returncode": result.returncode,
                  "check": "identity" if args.identity else "real_tool" if args.tools else "generation",
                  "result": "passed" if success else "failed"}
        if args.identity:
            report["answer"] = answer.replace(api_key, "<redacted>")
        print(json.dumps(report, ensure_ascii=False))
        if not success:
            errors = [event.get("message", event.get("error", {})) for event in events
                      if event.get("type") in ("error", "turn.failed")]
            print(json.dumps(errors).replace(api_key, "<redacted>")[:2500])
            raise SystemExit(1)


if __name__ == "__main__":
    main()
