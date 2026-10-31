"""Python SDK for running Xdcode workflows.

Start with :class:`Xdcode` for synchronous applications or
:class:`AsyncXdcode` for async applications. Most programs create a thread and
run a turn::

    from openai_xdcode import Xdcode, Sandbox

    with Xdcode() as xdcode:
        thread = xdcode.thread_start(sandbox=Sandbox.workspace_write)
        result = thread.run("Describe this project.")
        print(result.final_response)
"""

from ._version import __version__
from .api import (
    ApprovalMode,
    AsyncChatgptLoginHandle,
    AsyncXdcode,
    AsyncDeviceCodeLoginHandle,
    AsyncThread,
    AsyncTurnHandle,
    ChatgptLoginHandle,
    Xdcode,
    DeviceCodeLoginHandle,
    ExternalMessage,
    ImageInput,
    Input,
    InputItem,
    LocalImageInput,
    MentionInput,
    RunInput,
    Sandbox,
    SkillInput,
    TextInput,
    Thread,
    TurnHandle,
    TurnResult,
)
from .client import XdcodeConfig
from .errors import (
    XdcodeError,
    XdcodeRpcError,
    InternalRpcError,
    InvalidParamsError,
    InvalidRequestError,
    JsonRpcError,
    MethodNotFoundError,
    ParseError,
    RetryLimitExceededError,
    ServerBusyError,
    TransportClosedError,
    is_retryable_error,
)
from .retry import retry_on_overload

__all__ = [
    "__version__",
    "XdcodeConfig",
    "Xdcode",
    "AsyncXdcode",
    "ApprovalMode",
    "Sandbox",
    "ChatgptLoginHandle",
    "DeviceCodeLoginHandle",
    "AsyncChatgptLoginHandle",
    "AsyncDeviceCodeLoginHandle",
    "Thread",
    "AsyncThread",
    "TurnHandle",
    "AsyncTurnHandle",
    "TurnResult",
    "Input",
    "InputItem",
    "RunInput",
    "ExternalMessage",
    "TextInput",
    "ImageInput",
    "LocalImageInput",
    "SkillInput",
    "MentionInput",
    "retry_on_overload",
    "XdcodeError",
    "TransportClosedError",
    "JsonRpcError",
    "XdcodeRpcError",
    "ParseError",
    "InvalidRequestError",
    "MethodNotFoundError",
    "InvalidParamsError",
    "InternalRpcError",
    "ServerBusyError",
    "RetryLimitExceededError",
    "is_retryable_error",
]
