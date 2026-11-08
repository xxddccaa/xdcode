# xdcode 统一模型配置

本文说明 xdcode **0.167.0** 的模型目录接口与维护约定；真实 Codex 基线仍为 **0.165.0**，
另有选择性修补，详见 [维护记录](../UPSTREAM_0.167.0.md)。源码和开发构建不代表已安装程序；
本轮不安装、不替换当前程序。

## 配置一个目录

默认用户配置是 `~/.xdcode/config.toml`；设置 `XDCODE_HOME` 时使用该 home 的
`config.toml`。提供方继续定义在 `[model_providers.<id>]`，模型统一定义为 `[[models]]`，
无需给每个模型创建文件。`-p NAME` 仍选择已有 profile。已有 `model`、`model_provider`
及推理配置继续有效；没有目录时沿用原模型选择行为，并显示当前配置的自定义模型。

```toml
[model_providers.ds]
name = "My Responses provider"
base_url = "https://example.invalid/v1"
env_key = "DS_API_KEY"
wire_api = "responses"
requires_hosted_auth = false

[[models]]
id = "ds-flash"
name = "DeepSeek Flash"
provider = "ds"
model = "deepseek-flash"
# 不写 reasoning_efforts：自动发现，成功后回写。

[[models]]
id = "custom-model"
name = "我的另一个模型"
provider = "ds"
model = "provider-model-id"
reasoning_efforts = ["low", "high", "custom-level"]
default_reasoning_effort = "custom-level"
compatibility = "unverified"
```

URL 和模型名称只是配置结构示例，不是模型服务可用性声明。凭据通过 `env_key` 指定的
环境变量或既有凭据机制提供，值不写入示例、JSON、命令参数或聊天。多个厂家可以有相同
`name` 或 `model`，通过全目录唯一的 `id` 区分；`provider` 必须引用存在的提供方。

| 模型字段                   | 含义                                              |
| -------------------------- | ------------------------------------------------- |
| `id`                       | 稳定、非空、唯一的目录标识                        |
| `name`                     | 用户可读名称                                      |
| `provider`                 | 提供方 ID                                         |
| `model`                    | 原样发送的模型标识，包括 `:free` 等后缀           |
| `reasoning_efforts`        | 可选字符串列表，支持厂家自定义值                  |
| `default_reasoning_effort` | 可选默认值，已配置列表时必须属于列表              |
| `context_window`           | 可选正整数，模型上下文 token 数                   |
| `auto_compact_token_limit` | 可选正整数，自动压缩 token 阈值                   |
| `compatibility`            | `unverified`（默认）、`responses`、`incompatible` |
| `reasoning_discovery`      | 自动发现生成的来源及失效判断记录，不应手工伪造    |

三种列表含义不同：不填写表示自动发现；填写 `[]` 表示只使用服务默认，请求不发送
`reasoning.effort`；填写非空列表表示用户维护，直接信任，不进行发现。`["none"]`
表示可选择并发送字面字符串 `none`，和服务默认不同。切换模型时只能恢复目标支持的等级。

## `/model`、模式和项目记忆

打开 `/model` 会重新读取模型目录及提供方定义，新增条目无需重启就可见。目录支持搜索，
显示当前模式和选择状态，以及缺少凭据、未验证、接口不兼容等状态。读取新配置不会自行
切换当前会话；只有明确选择并获得服务端确认后才应用和保存。统一目录和厂家子页每次
从首个可选项打开，不自动跳到当前模型；当前模型仍按精确目录 ID 标记。长列表正常滚动，
搜索、分组及禁用状态不变。

普通模式和 Plan 模式分别保存厂家、目录 ID、模型和等级。选择仅修改当前模式，不再询问
“仅 Plan／所有模式”。首次 Plan 使用普通模式模型，已有 Plan 等级只在目标模型支持时
保留；以后两种模式独立。记忆还包括每个模型上次等级及最后模式。

切模式时，若记住的目标模型已被删除、配置失效，或服务端明确拒绝向该厂家转移当前加密
历史，界面会尝试用当前已确认的厂家、模型、目录 ID 和等级进入目标模式一次。回退也须
得到服务端确认；成功后更新目标模式的项目记忆并显示原因，另一模式的选择保留。例如
普通模式使用 `provider_a`，而 Plan 记住了 `provider_b`，若当前历史不能转给
`provider_b`，Plan 会回退为普通模式当前的模型，下次进入 Plan 继续使用这一选择。

该回退只用于切模式时恢复模型，不用于手动 `/model` 选择。传输中断、未分类的服务端错误
和不支持设置更新的旧服务端均不会触发回退。回退仍失败时保留原模式、模型和草稿；
带有“执行计划”文本的切换只在确认成功后发送一次。记忆写入失败时会提示仅当前会话有效。

项目以规范化 Git worktree 根目录为键，非 Git 目录以启动目录为键；不同 worktree 分开。
配置身份也进入记忆的隔离范围，防止不同有效文件或 profile 互相覆盖。记忆独立存放于
运行状态目录，不能为了保存选择重写项目源代码配置或提交运行数据。

普通、命名及新 worktree 的 `/new` 继承当前**已确认**的厂家、精确目录 ID、模型、等级
和 Default／Plan 模式；服务默认等级仍表示不发送 effort。创建时显式传入厂家与模型，
再用已有设置接口确认目录、等级和模式，确认后才切换界面或发送首条消息。确认失败或
旧服务端不支持该接口时，保留原会话、命令草稿与附件，并尝试归档空候选线程。
待确认的模型／模式选择不算生效。该继承不改写 `config.toml`，不复制原项目的持久偏好到
新 worktree；同项目另一模式的记忆保持独立。普通启动、恢复、fork 和 `/clear` 的优先级不变。

同一 `XDCODE_HOME` 下，会话历史跨 profile 和厂家共享。使用 `xdcode -p ds` 或
`xdcode -p deepkey` 后，`/resume`、按名称恢复、`resume --last` 和 fork 都查询所有
厂家；现有目录/项目筛选继续有效，`--all` 用于扩大目录范围。不同 home 的历史仍各自
存储。配置隔离仅用于模型偏好，不会创建或筛出一套独立历史。恢复或切换时仍检查厂家
专属加密内容，不能因为列表可见就将不兼容的密文发送给另一家。

恢复显式历史会话时优先使用该会话设置；显式启动参数仍有最高优先级。单次启动覆盖不
自动改写项目记忆，之后明确选择模型或切换模式并获确认时才保存。设置更新失败时保留原设置；项目
记忆保存失败时说明当前选择仅本次有效。

跨厂家切换保留同一线程、可读历史、工作目录及审批状态。当前回合固定原运行配置，新的
厂家、目录 ID、模型、effort 和模式作为一次原子更新从下一回合生效，并在界面提示。
每家提供方的 `base_url` 可以有不同域名和路径；切换会更新实际请求目标、客户端、
认证路由、能力及缓存，不能沿用旧厂家的 endpoint 或密钥；旧连接及压缩结果不能跨厂家复用。
厂家专属加密历史不能转发给另一家，缺少可移植历史时应阻止切换并解释原因。
历史设置及回合记录厂家接口指纹，恢复和 fork 会按有效历史检查，因此同一厂家 ID 改了 `base_url`
也不能复用其旧密文。较早的历史若含密文且没有可核验的接口指纹，会保留原文件并拒绝
恢复；普通文本历史不受该限制。该提示不表示会话列表被隔离。

## 自动发现与保存

发现依次尝试权威模型元数据、接口明确返回的枚举和有限候选探测。候选包括已有标准值及
接口返回的自定义值，最多两个并发请求、总时限 60 秒，限制输出，不携带会话历史、项目
内容或工具。认证、网络、限流及服务失败保留未知状态，不能据此写成“不支持思考”。

`reasoning_discovery` 记录来源 `authoritative_metadata`、`api_enum` 或 `verified_probes`、
Unix 秒时间 `discovered_at`、模型标识、接口指纹、完整性 `complete` 及等级列表指纹。
只有权威枚举才能标记完整；候选探测只表示“已验证，完整性未知”。保存后不在每次启动
重探测；模型或接口变化使自动结果失效，用户修改列表后转为手工维护。

配置写入保留注释和无关字段，版本校验后原子替换。发现期间文件发生修改时重新读取和
合并；不能覆盖用户新内容。只读文件下保留本次内存结果，明确说明未保存。真实请求拒绝
某等级时报告原因，不自动重放、不偷偷换模型，也不改写用户维护的列表。

## 共用的管理命令

`xdcode models` 与运行时共用解析、校验、发现和写入逻辑。当前会话的配置路径、profile
和启动覆盖需保留在命令上下文中；先看 `list --json` 的 `source`，不要猜测配置位置。

```sh
xdcode models list --json
xdcode -p my-profile models list --json
xdcode models upsert --json --preview --expected-version VERSION < model-patch.json
xdcode models upsert --json --expected-version VERSION < model-patch.json
xdcode models check custom-model --json
```

把 `VERSION` 替换为 `list` 返回的真实内容版本。`list` 返回有效模型、配置来源、版本和
提供方脱敏状态，包括环境变量名及凭据是否可用，不返回凭据值。不要把脱敏提供方输出
当作完整提供方定义写回。`upsert` 从标准输入接收 JSON，按稳定 ID 合并模型和提供方：

```json
{
  "models": [
    {
      "id": "custom-model",
      "name": "我的另一个模型",
      "provider": "ds",
      "model": "provider-model-id",
      "reasoning_efforts": ["low", "high", "custom-level"],
      "default_reasoning_effort": "custom-level",
      "compatibility": "unverified"
    }
  ]
}
```

`providers` 可选，以 ID 映射到提供方定义。复用已有提供方时省略它；新增时只传必要的
路由定义及 `env_key`，不传密钥、认证命令、token、header 或 query 中的秘密。每个模型
条目视为完整目标定义，更新时保留用户没有要求删除的可选字段。`--preview` 不写入；版本
冲突时重新读取、合并并预览，不能盲目去掉版本条件重试。

`check ID` 默认只做静态检查。显式 `check ID --probe` 才会执行实际能力请求，
`refresh ID` 刷新自动发现等级；两者都受当前任务是否允许联网和真实模型请求的约束。
手工列表默认不被刷新覆盖，用户明确要求替换时才使用 `refresh ID --overwrite-manual`。

当前命令的边界：JSON 提供方合并会保留已有的布尔能力开关；需要把原先的 `true`
明确改成 `false` 时，应编辑对应 TOML 字段。能力发现支持环境变量或现有 bearer 凭据；
命令认证、OAuth 和 AWS 认证的探测需通过正常运行客户端另行验证，不会在发现命令中
执行认证命令或重建这些凭据流程。`check --probe` 的简单文本请求只能确认 Responses
请求被接受，工具调用能力仍需独立验证。

## 用自然语言添加模型

程序内置 `xdcode-model-config` skill，资源由程序打包，按内容指纹更新到
`$XDCODE_HOME/skills/.system`。安装版本无需访问源码仓库即可发现它；`xdcode-guide`
会把添加模型、配置厂家、调整等级等任务引导到该 skill。元数据使用 `agents/xdcode.yaml`。

用户可以说“把这个模型加进去”或提供官方模型页面链接。skill 使用上面的命令，检查有效
配置源及 profile，预览并合并；默认只加入目录。用户同时要求切换时才发起当前模式的
设置更新，并在确认后保存项目记忆。没有可用的会话设置接口时说明需要 `/model` 选择，
不能仅改文件就宣称已切换。

例如 `https://openrouter.ai/minimax/minimax-m3.1-flash:free` 必须保留精确模型 ID
`minimax/minimax-m3.1-flash:free`。复用已有 OpenRouter 提供方；没有时可使用
`https://openrouter.ai/api/v1`、`wire_api = "responses"` 和 `OPENROUTER_API_KEY`
环境变量名提出未验证配置。分别查官方资料或显式探测 Responses、推理参数、工具调用
及当前价格；提供方支持某 API 不代表该型号可用。

本实现范围仅为 Responses API。确认只能通过 Chat Completions 使用的型号可保留为
`incompatible` 条目，不能选择并声称可用，也不隐式增加 Chat 传输。缺少凭据时说明环境
变量名，不要求在聊天里粘贴值；查询工具不可用或验证失败时保留 `unverified`，区分已确认
和未知。免费状态只反映查询时官方信息，不保证长期价格，也不替换为付费型号。

本机联网查询按工作区约定只用本地 `web-search` MCP。当前该 MCP 未连接；本次没有
查验上述示例型号的当前价格、可用性或 Responses 兼容性，没有发真实模型请求。

## 委派给指定模型

内置 `xdcode-delegate` skill 处理用户明确要求的小模型开发、委派或并行任务。
例如“用 `worker_provider` 的 `worker-model` 开发，你来审查，十分钟后检查”。
技能先使用当前配置源及 profile 的 `xdcode models list --json` 解析目录 ID、提供商、
请求模型和思考等级；不会改写父会话或全局模型设置。

原生 `spawn_agent` 的 `model` 可填写精确的目录 ID，一次选择该条目的提供商、请求
模型、思考等级和上下文限制。填写请求模型名时只在继承的提供商内匹配；同名条目
无法消歧时要求目录 ID。显式参数或工作 agent 默认配置指定等级时按该等级校验，
否则使用条目的默认等级；空列表及未设默认等级对应省略 effort，不沿用父模型不支持
的等级。字面值 `none` 保持独立含义。
角色配置仍遵循原有覆盖顺序。v1 返回的 `selection` 包含最终生效的 `catalog_id`、
`model`、`provider` 和 `reasoning_effort`，供父 agent 核对，不包含凭据或接口地址。
跨提供商 fork 继续执行已有的加密历史归属检查。

使用前检查当前工具 schema；旧二进制可能尚不支持目录 ID 或返回 `selection`。
用户指定 `xdcode exec` 时走该途径：CLI 的 `-m` 使用请求模型名，提供商和等级通过
单次进程覆盖传入。不能因为原生接口接受目录 ID，就假定 `exec -m` 也接受它。

默认每个工作 agent 限时十分钟，五分钟检查进度；用户可指定更短期限。父 agent
记录并检查实际进度，在期限到达时回收工作，审查差异、实际行为和验证结果。
原生工具依赖父 agent 持续监督，`wait_agent` 超时不会终止工作；要求进程级硬时限时，
`exec` 必须由独立 watchdog 监督并只终止其拥有的进程组。代码、日志和部分结果都应
在回收后复核，不能把工作 agent 自述完成当成验收通过。

技能及原生/exec 参考文件随程序打包，并通过同一内容指纹机制更新；详见
[`xdcode-delegate`](../xdcode-rs/skills/src/assets/samples/xdcode-delegate/SKILL.md)。
这些行为属于本轮源码改动；是否已在安装版本中启用，以对应构建及运行验收为准。

## 验证范围与发布记录

最初的源码交付只允许静态及独立离线检查。用户在 2026-10-21 后续授权了测试、构建、
安装试用及正式发布；该授权不表示以下所有验收都已完成。每个构建的发布说明分别记录
实际结果，静态检查不能替代编译、TUI 或安装包验收。维护时持续覆盖：

- 运行配置、协议、发现与并发原子写入的 Rust 测试，包括只读和旧配置兼容。
- 运行两个模拟 Responses 服务的路由、认证、模型和 effort、历史迁移、执行中切换、
  压缩缓存失效及失败回滚测试。模拟服务也不等于真实模型兼容性验收。
- 项目/worktree/配置源隔离、普通/Plan、最后模式、历史会话及显式启动覆盖恢复；
  `ds`/`deepkey` 两个 profile 的共享历史、按名称/最近恢复及目录范围筛选。
- 实际构建、安装资源打包发现、自然语言调用、添加后 `/model` 即时可见和 TUI 验收。
- 用户允许后的真实接口兼容性、工具调用和思考等级探测；记录证据和未确认部分。

最初的离线记录（2026-10-21）：skill 相关 Python 测试 15 项通过，配置/协议资源检查
10 项通过；skill 元数据校验、Python 语法检查、Cargo 文件及文档 TOML 解析通过。
直接调用 rustfmt 检查 103 个修改或新增的 Rust 源码文件，关闭既有 imports/modules 排序以避免无关重排；这仅
检查格式和可解析性。独立源码复查发现并补齐历史恢复标记、跨厂家密文检查、目录 ID、
上下文长度及 effort 字符串序列化等边界。未运行 Rust 类型检查、Rust 测试、构建、
安装或真实模型请求，未替换当前程序，未提交或推送。

正式发布前的补充检查（2026-10-21）：模型目录 Rust 测试 24 项通过，含双模拟
Responses 服务和多模型发现记录回写的回归检查；协议资源检查 10 项、配置 skill 检查
7 项和内置 skill 来源检查 6 项通过。打包/安装器测试 44 项中 43 项通过、1 项因独立
测试环境未提供 rustc 跳过。测试使用独立系统用户，保留本机运行中的 daemon。
安装包从干净提交构建，实际产物与补充验收结果以对应 GitHub Release 的记录为准。

本机用户配置的显示名称采用普通英文、空格与括号，例如 `GPT-6.1 Sol (Step)`；目录
ID 与 API 模型 ID 保持稳定。此命名习惯不改变接口路由，也不表示程序对 Unicode
显示名称做了兼容性保证。个人配置、凭据和项目选择不进入源码及安装包。

维护者升级上游时逐项对照根目录 `AGENTS.md` 与 `UPSTREAM_UPGRADE.md`，同时检查核心
运行配置、协议及客户端和内置资源，不能只保留界面。
