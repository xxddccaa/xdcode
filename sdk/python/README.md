# OpenAI Xdcode Python SDK

Build Python applications that start Xdcode threads, run turns, stream progress,
and control workspace access.

## Install

Install the SDK:

```bash
pip install openai-xdcode
```

## Quickstart

The SDK reuses your existing Xdcode authentication when one is already
available:

```python
from openai_xdcode import Xdcode

with Xdcode() as xdcode:
    thread = xdcode.thread_start()
    result = thread.run("Explain this repository in three bullets.")
    print(result.final_response)
```

`thread.run(...)` returns a `TurnResult` containing the final response,
collected items, and token usage.

## Authentication

Existing Xdcode authentication is reused automatically. To start ChatGPT
browser login explicitly:

```python
from openai_xdcode import Xdcode

with Xdcode() as xdcode:
    login = xdcode.login_chatgpt()
    print(login.auth_url)
    print(login.wait().success)
```

For device-code login:

```python
with Xdcode() as xdcode:
    login = xdcode.login_chatgpt_device_code()
    print(login.verification_url, login.user_code)
    login.wait()
```

For API-key login:

```python
with Xdcode() as xdcode:
    xdcode.login_api_key("sk-...")
```

## Built-In Help

Use Python's standard `help(openai_xdcode)`, `help(Xdcode)`, or
`python -m pydoc openai_xdcode` documentation tools.

## Documentation

- [Getting started](https://github.com/openai/codex/blob/main/sdk/python/docs/getting-started.md)
- [API reference](https://github.com/openai/codex/blob/main/sdk/python/docs/api-reference.md)
- [Untrusted external messages](https://github.com/openai/codex/blob/main/sdk/python/docs/api-reference.md#externalmessage)
- [FAQ](https://github.com/openai/codex/blob/main/sdk/python/docs/faq.md)
- [Examples](https://github.com/openai/codex/blob/main/sdk/python/examples/README.md)

The package is licensed under the
[repository Apache License 2.0](https://github.com/openai/codex/blob/main/LICENSE).
