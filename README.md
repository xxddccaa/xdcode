# xdcode

xdcode 是基于 [OpenAI Codex](https://github.com/openai/codex) 源码的个人维护版本，
面向终端里的日常编码工作。版本号跟随目标 Codex 版本：目标上游是 Codex 0.167.0，
就发布 xdcode 0.167.0。每个版本会在对应上游源码的基础上删除不需要的功能，并加入
个人使用需要的增强，因此 xdcode 不是 Codex 的完整发行版。

支持平台：

- Linux x86_64（GNU libc；安装器需要 Python 3.11+）
- Apple Silicon macOS（使用系统自带工具，不需要 Python、Rust 或 Homebrew）
- Windows x86_64（PowerShell 安装器）

Intel Mac 和 Linux ARM64 暂不支持。

## 核心特点

- **本地终端编码 agent**：工具调用、审批、沙箱、MCP、skills、Plan、会话恢复和
  多 agent 都在本地运行。
- **模型自己配置**：连接你自己的 Responses-compatible provider；支持多厂家、多模型，
  在会话中用 `/model` 切换。
- **个人增强**：统一模型目录、项目和模式记忆、跨厂家共享历史、限流重试、受控的
  小模型委派。
- **不依赖托管服务**：不需要托管登录、云任务、遥测上传或自动更新；凭据通过你自己的
  环境变量提供。

## 快速开始

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/xxddccaa/xdcode/main/install.sh | bash
xdcode --version
xdcode
```

安装器会选择最新发布版本，并自动识别 Linux x86_64 或 Apple Silicon macOS。默认要求
xdcode 进程已停止；如果确实需要在进程运行时安装，再显式加 `--force`。固定版本用
`--release v<version>-g<commit>`。

### Windows

在 PowerShell 5.1 或更高版本中运行：

```powershell
& ([scriptblock]::Create((Invoke-RestMethod -Uri 'https://raw.githubusercontent.com/xxddccaa/xdcode/main/install.ps1')))
xdcode --version
```

Windows 的前置条件、PATH 行为和验证范围见 [Windows 安装说明](docs/xdcode-windows.md)。

## 配置模型

把配置写到 `~/.xdcode/config.toml`，或者设置 `XDCODE_HOME` 后写到
`$XDCODE_HOME/config.toml`。最小配置如下：

```toml
model = "your-model"
model_provider = "your-provider"

[model_providers.your-provider]
name = "Your provider"
base_url = "https://your-provider.example/v1"
env_key = "XDCODE_API_KEY"
wire_api = "responses"
requires_hosted_auth = false
```

把 `base_url` 和 `model` 换成你的服务实际使用的值，并通过环境变量或密钥管理器提供
`XDCODE_API_KEY`：

```bash
export XDCODE_API_KEY="..."
xdcode
```

需要配置多个厂家、模型目录、思考等级或自动发现时，见
[模型配置说明](docs/model-configuration.md)。示例中的 URL、模型名和 provider ID 都只是
占位，不是服务可用性声明。

## 常用操作

- `/model`：重新读取模型目录，按厂家分组选择模型；切换在服务端确认后生效。
- `/new`：新建会话；继承当前已确认的厂家、模型、思考等级和模式。
- `/resume`：恢复历史会话；同一 `XDCODE_HOME` 下跨厂家和 profile 共享历史。
- `xdcode exec "..."`：非交互执行一次任务，适合脚本和自动化。
- `xdcode models list` / `check` / `refresh`：查看和维护模型目录。

更完整的命令和配置说明见 [交付指南](docs/xdcode.md) 和
[配置说明](docs/config.md)。

## 安装细节

- 升级时默认要求 xdcode 进程已停止。`--force` 只跳过活跃进程检查，不会跳过校验和、
  安装锁或旧版本保留，也不会结束已有会话。
- 固定发布版本：`--release v<version>-g<commit>`。
- 需要代理时，通过 `XDCODE_INSTALL_PROXY=<proxy-url>` 传给安装器；Windows 安装器
  使用 `-Proxy <proxy-url>`。不要把本机代理地址写进仓库。
- 离线安装：把 `.tar.gz`、`.sha256` 和 `.install.py` 放在同一目录，然后运行：

  ```bash
  python3.11 xdcode-<version>-<build-id>-linux-x86_64.install.py \
    xdcode-<version>-<build-id>-linux-x86_64.tar.gz
  ```

安装器会校验完整性，把新版本放到独立目录，并只切换受管理的 `xdcode` 入口；旧版本和
运行数据保留。

## 从源码构建

维护者使用 Python 3.11+：

```bash
xdcode_upstream_commit="$(python3.11 -c 'import json; print(json.load(open("upstream.json"))["upstream_commit"])')"
if ! git cat-file -e "$xdcode_upstream_commit^{commit}" 2>/dev/null; then
  git fetch --no-tags --depth=1 https://github.com/openai/codex.git "$xdcode_upstream_commit"
fi
python3.11 scripts/xdcode/build.py --build-id personal1
python3.11 scripts/xdcode/package.py --build-dir target/xdcode-builds/personal1
```

构建产物在 `target/xdcode-dist/`，包含压缩包、`.sha256` 和离线安装器。开发快照需要
显式加 `--allow-dirty`；正式发布必须使用干净提交。构建前置依赖、测试路径和发布校验见
[交付指南](docs/xdcode.md)。

## 文档

- [交付指南](docs/xdcode.md)：构建、测试、校验和、安装与发布流程。
- [配置说明](docs/config.md)：`config.toml`、MCP 和生命周期钩子。
- [模型配置](docs/model-configuration.md)：多厂家模型目录、思考等级和 `/model`。
- [macOS 安装](docs/xdcode-macos.md)
- [Windows 安装](docs/xdcode-windows.md)
- [上游迁移说明](UPSTREAM_UPGRADE.md)

## 版本与上游

xdcode 不维护独立于上游的产品版本号；版本号跟随目标 Codex 版本。每个版本在对应上游
源码的基础上删除不需要的功能，加入个人增强，因此功能集不等同于 Codex，也不应把
xdcode 描述成 Codex 的完整发行版。

当前源码实际采用的上游提交、已移植内容和未覆盖范围，以 [upstream.json](upstream.json)、
[0.167.0 维护记录](UPSTREAM_0.167.0.md) 和 [改动记录](MODIFICATIONS.md) 为准。源码更新
不等于已安装程序更新；只有构建并安装新包后才会生效。

## 许可证与归属

xdcode 以 Apache-2.0 许可证发布，保留 OpenAI Codex 的原始版权和归属。许可证与归属声明见
[LICENSE](LICENSE) 和 [NOTICE](NOTICE)，派生改动见 [MODIFICATIONS.md](MODIFICATIONS.md)。
上游作者不对本派生版本背书。

## 维护者

调整上游基线或移植功能前，先读 [AGENTS.md](AGENTS.md) 和
[UPSTREAM_UPGRADE.md](UPSTREAM_UPGRADE.md)。发布流程、验证分级和安装保护规则见
[docs/xdcode.md](docs/xdcode.md)。
