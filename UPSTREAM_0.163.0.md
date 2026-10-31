# xdcode 0.163.0 源码升级记录

## 基线和交付状态

本次将 xdcode 的源码基线从 0.162.0 推进到 Codex **稳定版 0.163.0**，保留独立应用行为。
这是为后续 xdcode 专属功能准备的源码分支，**未编译、未安装、未进行运行验收，未推送或触发 CI**。
当前已安装的 0.162.0 程序没有被替换，现有服务和运行数据没有操作。

| 项目 | 值 |
| --- | --- |
| 开发分支 | `feature/upstream-0.163.0` |
| xdcode 起始提交 | `47851fc134` |
| 原产品版本 | `0.162.0` |
| 原采用的上游源码 | `2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4` |
| 新稳定发布标签 | `rust-v0.163.0` |
| 标签指向的源码 / 发布提交 | `83086188ae0614bd882818ff1aa23f3194962012` |
| 发布分支基点 | `30c1058df67d3d28000cddb6faa8e80d9a9143e9` |
| 版本权威记录 | `upstream.json` |
| Rust toolchain | `1.95.0`，与原基线相同 |

比较上述真实源码范围，共 118 个上游提交、726 个路径变化（识别重命名后 721 条变更）。
迁移前将新旧上游路径和内容转换为现有 xdcode 命名，再与当前分支做三方合并；
人工处理依赖、协议接口及定制行为冲突。没有用上游整仓覆盖 xdcode。
当前目录的 `codex` 参考仓库位于该稳定标签提交，`codex-source` 未修改。

产品版本在 Cargo 工作区、`xdcode-cli/package.json`、`upstream.json` 中均为 **0.163.0**。
160 个 Rust 工作区包继承该版本。个人修改以后用独立 build ID 区分，不修改产品版本来冒充上游版本。
SDK 的开发版本、兼容性测试用旧程序版本和固定测试夹具版本各自保留原语义。

## 迁入范围

| 模块 | 采用的变化与依赖 |
| --- | --- |
| 上下文与请求 | 异步远程压缩的配置和生命周期、跨压缩保留客户端指定工具输出、加密输出 replay、未完整响应的错误与 token 计费；新实验能力保持上游的选择性启用策略 |
| 多 agent | 完整 fork 的可选前缀保留、预测 fork 继承上下文、子 agent 默认上下文限制开关、spawn 返回前维持临时所有权；消息板权限、结构化拒绝和本地连接管理 |
| 执行与沙箱 | 进程退出后的有界输出清理与保留、exec-server 配置超时及 V2 capability discovery、冷恢复权限还原、Windows MXC 拆包和远程环境处理 |
| MCP 与 app-server | rmcp 3.5.1、绑定原连接的服务端请求回复、按 ID 读取 thread item、沙箱不透明 policy 与 launcher 元数据分离 |
| Code mode | 可选 gRPC over stdio host、取消及恢复处理、保留 JSON key 校验、`exit()` 终止当前完整 cell；host URI 的连接器使用本地管道 |
| TUI | 可搜索 `/config` 偏好面板、键盘选择 transcript、OSC 7501 终端状态、剪贴板与 WSL 修复、重连/启动输入保护、配置即时生效及 agent 列表稳定性 |
| Guardian | 取消后先 interrupt/drain 再复用连接、过期分类取消、上下文预算与父压缩失败分类、显式 access program 随所属回合转发 |
| 网络与依赖 | 重复 Host header 拒绝、凭据别名不能绕过 MITM hooks、hyper 1.11.1、quinn-proto 0.11.18、相关 Cargo/Bazel 依赖与锁文件 |
| 协议导出 | 同步 Rust、JSON schema、TypeScript、Python 生成源码，以及稳定/实验两套预计算压缩导出 |

新 capability、配置键和可选协议字段随源码一起迁移。稳定协议压缩导出内的 747 个 TypeScript
文件、318 个 JSON schema 与工作树逐字一致；内部 RolloutLine schema 仅保存在归档中。
实验导出另含实验方法与字段，不能拿它与稳定目录要求逐字相同。
协议压缩文件用现成的 libzstd 解压、转换名称、重压缩并检查 round trip，没有运行 Rust 生成器。

## 保留的 xdcode 行为与排除项

- 保留命令/界面/默认提示词的 xdcode 身份，`XDCODE_*`、`~/.xdcode`、项目 `.xdcode`、
  `model_types`、`Provider…`、`requires_hosted_auth` 和 `agents/xdcode.yaml` 等现有接口约定。
- 保留显式配置的 Responses provider 与用户模型选择；不恢复托管登录、云任务、遥测上传、
  模型推广、自动模型切换、在线升级和程序下载。Linux 与 Apple Silicon 交付脚本保留。
- `/daemon` 只提供本地重启：目标须为本地 daemon、连接须已确认由 xdcode 管理，且在线、无进行中的重启。
  操作先确认，默认取消，明确提示可能中断其他会话的工作；确认和执行阶段共用相同资格检查。
  不迁入“安装最新稳定版”“使用此 CLI 构建”的更新选项。
- Guardian hosted decisions 比较采样继续返回 `None`，不迁入缓存托管凭据或环境 API key 回退。
  主分类器仍使用所属线程配置的 provider；相关已断开的托管测试不会作为可用能力恢复。
- 上游将 CLI Daybreak 和 API-key Cyber access programs 默认启用的两项变化不采用；默认均保持 `false`。
  协议兼容和显式请求处理仍迁入，不自动获得或选择相应模型。
- Gov MCP origin 判定作为现有授权代码的兼容逻辑保留；它只分类已配置服务，并不创建连接、
  登录入口或默认服务。托管 auth 在 xdcode 中继续受现有禁用策略限制。
- 不恢复原先已删除的上游 Bazel / Rust release workflows，仓库仍使用现有 xdcode delivery workflow。
- 不重新添加已移除的模型推广/托管 usage 测试；上游 daemon 菜单测试改为 xdcode 的重启确认与资格回归用例，
  不带回包含安装选项的无效 snapshot。修复外部文档迁移测试中遗留的 Codex 期望文本。
- 上游版权与许可证、外部依赖 URL、协议规定的 wire exceptions 保留原含义；不会为消除文本命中改坏协议。

## 兼容性与依赖处理

Cargo.lock 以转换命名后的新上游锁为基础，通过 `cargo +1.95.0 update --workspace`
解析工作区版本；这只做依赖解析，没有编译。rmcp 固定到 Git revision
`4048ac40e958fb1a7d614617a2ae8d26666ee2fa` / 3.5.1，MXC 采用新上游拆分 crate。
随后用 `cargo +1.95.0 metadata --locked --offline --no-deps --format-version 1`
核对清单与锁的静态一致性。

本范围没有新增/修改 SQL migration，toolchain 没有升级。会话与权限恢复的代码变化仍须以后在数据副本上验证；
没有进行数据库迁移或用新源码启动已安装服务。MCP sandbox metadata 新增独立 launcher envelope，
消费者应透传不透明 policy；旧 `xdcode/sandbox-state-meta` 仍是相同 policy 的兼容别名。

## 本次检查与待执行验证

已完成且不触发构建的检查：

- 624 个修改/新增 Rust 文件用已安装 1.95.0 `rustfmt` 单独解析与格式检查（`skip_children=true`）。
  仓库 nightly import 配置在 stable rustfmt 下产生提示，不代表已经通过 nightly 风格检查。
- Cargo 清单/锁的 offline metadata 检查，160 个工作区包产品版本一致。
- 修改的 JSON、TOML、Python 源码分别用 JSON parser、`tomllib`、`ast.parse` 检查，未生成 Python 字节码。
- 稳定协议归档与磁盘导出比对，实验归档 round trip 与名称转换检查；两套导出共检查
  10,287 个 schema 内部引用和 3,713 个 TypeScript import 引用，均可解析。
- 合并标记、重复 import、新旧身份、服务 URL、默认功能开关及新增源码审计；Rust module 路径和
  本轮新增协议字段的构造器做静态核对；`git diff --check`。

按用户要求没有执行 `cargo check/build/test`、Rust schema generator、release 构建、打包、安装、
发布、推送或 CI。新增/迁入的测试只有源码，**没有执行**。以下工作留到允许编译后的验收阶段：

1. 受影响 crate 的类型检查与单元/集成测试，尤其跨模块协议、rmcp 生命周期、Guardian、
   retained outputs / async compaction、exec-server、TUI 配置与重启回归。
2. Bazel 外部依赖重新解析、锁文件再生成/一致性检查；当前仅迁入上游文本变化并解析锁 JSON。
3. Linux 与 Apple Silicon 实际构建、打包、离线安装与回退演练；已有安装器进程检测相关 CI 问题仍待单独处理。
4. 独立运行目录下验证模型实际收到的提示词、新会话身份、工具调用、MCP、skills、子 agent、恢复、
   沙箱与网络路径，确认启动/空闲/退出没有托管或更新请求。
5. 恢复/重启使用数据副本验证兼容性；不要在活跃生产任务上试验，也不能将保留旧包等同于数据可回滚。

静态检查不能证明类型正确、测试通过或运行功能可用。本分支是后续定制开发的源码基线，
不应把它当成已验证的 0.163.0 可执行发行包。

## 上游提交清单

下面按真实基线范围列出全部 118 个提交，便于追踪迁移；“迁入”均指适配到 xdcode 源码，
不表示已执行测试或恢复原上游托管行为。分组依赖随对应源码、协议和测试一起处理。

| 上游提交 | 上游变化 | 本次处理 |
| --- | --- | --- |
| `a06545b311` | Add opt-in history prefix preservation for full agent forks (#52535) | 迁入，保留 xdcode 定制 |
| `5fe4fc8f7c` | Forward clipboard copies to the terminal in Herdr sessions (#52557) | 迁入，保留 xdcode 定制 |
| `36ae1561b9` | Expose message board permissions and structured denials (#52568) | 迁入，保留 xdcode 定制 |
| `9a59289fbe` | Fix races in remote environment and session replacement tests (#52591) | 迁入，保留 xdcode 定制 |
| `64335d1afd` | Upgrade `hyper` to 1.11.1 and test connection closure (#52608) | 迁入，保留 xdcode 定制 |
| `c63929af50` | Align Linux sandbox names and docs with bubblewrap and seccomp (#52610) | 迁入，保留 xdcode 定制 |
| `47b80571f0` | Add template setup for local agent message boards (#52621) | 迁入，保留 xdcode 定制 |
| `7f2f4fd46e` | Consolidate local message board storage helpers (#52629) | 迁入，保留 xdcode 定制 |
| `751e7edd0b` | Skip background workspace routing reads without ChatGPT auth (#52635) | 迁入，保留 xdcode 定制 |
| `47c063de51` | Add experimental thread item lookup by ID (#52637) | 迁入，保留 xdcode 定制 |
| `8ec80de0ef` | Count interrupted goal continuations toward the no-activity limit (#52640) | 迁入，保留 xdcode 定制 |
| `d3f9b22b74` | Limit daemon feature compatibility checks to explicit CLI overrides (#52648) | 迁入，保留 xdcode 定制 |
| `89c8759d8c` | Bound local exec-server output draining after process exit (#52652) | 迁入，保留 xdcode 定制 |
| `8a0437ff34` | Add an opt-in feature for subagent model context defaults (#52659) | 迁入，保留 xdcode 定制 |
| `b49e2211a5` | Handle completion events in either order in shell snapshot tests (#52660) | 迁入，保留 xdcode 定制 |
| `65b82cdf96` | Prevent brokered credential aliases from bypassing MITM hooks (#52661) | 迁入，保留 xdcode 定制 |
| `04ad786998` | Add retention annotations to submitted tool outputs (#52671) | 迁入，保留 xdcode 定制 |
| `cb6f6141f8` | Move the exec-server test fixture into shared test support (#52674) | 迁入，保留 xdcode 定制 |
| `aab4f96148` | Refresh persisted capability roots from owner-provided configuration (#52676) | 迁入，保留 xdcode 定制 |
| `6e093c6081` | Add timeout-aware exec-server configuration reads (#52679) | 迁入，保留 xdcode 定制 |
| `96be88cb53` | Reject reserved Serde JSON keys in code mode (#52681) | 迁入，保留 xdcode 定制 |
| `52950c5924` | Validate Windows sandbox accounts before password repair (#52682) | 迁入，保留 xdcode 定制 |
| `c261727370` | Preserve code mode cancellation during output serialization (#52685) | 迁入，保留 xdcode 定制 |
| `45311351c2` | Add opt-in retention for turn tool outputs (#52686) | 迁入，保留 xdcode 定制 |
| `10da569252` | Forward per-turn Cyber access programs to Guardian (#52689) | 迁入，保留 xdcode 定制 |
| `2f761ae821` | Fix marketplace path matching for Windows junctions (#52696) | 迁入，保留 xdcode 定制 |
| `0c22ce8f2e` | Update the exec-server stable compatibility baseline to Codex 0.162.1 (#52700) | 迁入，保留 xdcode 定制 |
| `4bc83aa5f7` | Retry bootstrap GETs through the system proxy after request failures (#52702) | 迁入，保留 xdcode 定制 |
| `4aa94dce27` | Migrate the Windows MXC sandbox to split MXC crates (#52707) | 迁入，保留 xdcode 定制 |
| `07757f448f` | Explain session creation failures during server shutdown (#52721) | 迁入，保留 xdcode 定制 |
| `2dc0791c7d` | Add opt-in gRPC over stdio for the code-mode host (#52723) | 迁入，保留 xdcode 定制 |
| `7efd184c24` | Add observers for initial exec-server connection attempts (#52724) | 迁入，保留 xdcode 定制 |
| `4bad6d78e9` | Report terminal program status with OSC 7501 (#52725) | 迁入，保留 xdcode 定制 |
| `e4490eff65` | Allow model catalogs to override incremental tool notices (#52736) | 迁入，保留 xdcode 定制 |
| `5ef96ab278` | Add opt-in output token replay for OpenAI requests (#52742) | 迁入，保留 xdcode 定制 |
| `1652992148` | Make code-mode `exit()` stop the entire cell (#52748) | 迁入，保留 xdcode 定制 |
| `322bbf4d84` | Classify voice session failures and record terminal outcomes (#52756) | 迁入，保留 xdcode 定制 |
| `c3d3b142d1` | Make pinned transcript prompts clickable (#52778) | 迁入，保留 xdcode 定制 |
| `806d9732c9` | Report exec runtime resets before accepting further requests (#52825) | 迁入，保留 xdcode 定制 |
| `be9060ff96` | Stabilize the realtime steering timeline test (#52923) | 迁入，保留 xdcode 定制 |
| `6c0c6759d0` | Retain client-marked tool outputs across compaction (#52937) | 迁入，保留 xdcode 定制 |
| `1badea29ab` | Keep agent command center rows stable across progress updates (#52946) | 迁入，保留 xdcode 定制 |
| `b871a1b9f6` | Verify reset notification during code-mode host recovery (#52952) | 迁入，保留 xdcode 定制 |
| `47bcb3a3f9` | Reduce stack usage in async TUI tests (#52959) | 迁入，保留 xdcode 定制 |
| `ecebd679c3` | Defer owned transcript redraws during raw paste bursts (#52964) | 迁入，保留 xdcode 定制 |
| `7a8a9456e9` | Reuse and prewarm the WSL clipboard text reader (#52967) | 迁入，保留 xdcode 定制 |
| `34f33e4ada` | Keep startup modals protected when terminal input draining times out (#52968) | 迁入，保留 xdcode 定制 |
| `de8fab6d7a` | Upgrade `rmcp` to `3.5.1` and harden lifecycle tests (#52972) | 迁入，保留 xdcode 定制 |
| `cc7ba33601` | Add a searchable `/config` preferences panel to the TUI (#52990) | 迁入，保留 xdcode 定制 |
| `8c0f701167` | Add connection-bound reply handles for app-server requests (#53049) | 迁入，保留 xdcode 定制 |
| `a07c8ebd57` | Handle broken pipes gracefully in plugin list output (#53109) | 迁入，保留 xdcode 定制 |
| `97c1f62898` | Fix startup draft recovery for pending Vim Replace keys (#53148) | 迁入，保留 xdcode 定制 |
| `7c9bfb910e` | Restore thread-owned permissions on cold resume (#53150) | 迁入，保留 xdcode 定制 |
| `41fd60f561` | Allow selecting and copying TUI turn tips (#53152) | 迁入，保留 xdcode 定制 |
| `8553558345` | Show subagent navigation shortcuts in TUI help (#53153) | 迁入，保留 xdcode 定制 |
| `95db2cd5d5` | Keep remote app-server events deliverable during shutdown (#53155) | 迁入，保留 xdcode 定制 |
| `72f999edc7` | Simplify the custom terminal drawing API (#53156) | 迁入，保留 xdcode 定制 |
| `27065a91cc` | Allow restarting managed background servers from the TUI (#53173) | 迁入，保留 xdcode 定制 |
| `9f3d25588b` | Keep agents navigation available after a thread is archived (#53201) | 迁入，保留 xdcode 定制 |
| `a9b0815167` | Support Windows clipboard text paste shortcuts in the TUI (#53224) | 迁入，保留 xdcode 定制 |
| `7e9c061ceb` | Finish remote exec cleanup when output streams remain open (#53331) | 迁入，保留 xdcode 定制 |
| `6e5f665feb` | Test final polling of remote processes with open output streams (#53339) | 迁入，保留 xdcode 定制 |
| `ddb33e34b2` | Remove selected protocol, apply-patch, MCP, and TUI tests (#53358) | 迁入，保留 xdcode 定制 |
| `6c372fbef4` | Allow turns when the agent message board is unavailable (#53369) | 迁入，保留 xdcode 定制 |
| `0c7362d121` | Ignore the unsafe-libyaml advisory in dependency audits (#53382) | 迁入，保留 xdcode 定制 |
| `27e6192e84` | Use cached login credentials for Guardian decisions (#53389) | 迁入，保留 xdcode 定制 |
| `cab328f275` | Distinguish Guardian parent-compaction failure reasons (#53393) | 迁入，保留 xdcode 定制 |
| `97239fd052` | Add explicit local board connections with fixed runtime access (#53400) | 迁入，保留 xdcode 定制 |
| `16845bb8e8` | Isolate arg0 helper directories by operating system (#53412) | 迁入，保留 xdcode 定制 |
| `bf4ca52b61` | Restore saved thread permissions during configuration loading (#53419) | 迁入，保留 xdcode 定制 |
| `fc4921f487` | Add regression coverage for network grants on cold thread resume (#53424) | 迁入，保留 xdcode 定制 |
| `f605162354` | Preserve Windows runtime registration failure diagnostics (#53426) | 迁入，保留 xdcode 定制 |
| `b3c3bad976` | Apply configuration toggles live and improve settings navigation (#53429) | 迁入，保留 xdcode 定制 |
| `6f47fcb0be` | Route TUI request replies to their originating connection and thread (#53436) | 迁入，保留 xdcode 定制 |
| `7c1b64c5df` | Stop identifying Apple Terminal from `TERM_SESSION_ID` (#53438) | 迁入，保留 xdcode 定制 |
| `6c7d409a6d` | Preserve status line accents on 16-color terminals (#53440) | 迁入，保留 xdcode 定制 |
| `3ece80b784` | Use local MSVC toolchains for native Windows Bazel tests (#53443) | 迁入，保留 xdcode 定制 |
| `c308e127e0` | Suppress repeated turn diffs in `codex exec` human output (#53444) | 迁入，保留 xdcode 定制 |
| `bc9cad3473` | Treat incomplete responses as terminal errors (#53451) | 迁入，保留 xdcode 定制 |
| `7a0a8ac9c4` | Close input admission while a turn finishes recording (#53452) | 迁入，保留 xdcode 定制 |
| `0c329ad2c9` | Prefer the Windows clipboard and bound copy attempts on WSL (#53453) | 迁入，保留 xdcode 定制 |
| `4a93bd249a` | Update `quinn-proto` to 0.11.18 (#53485) | 迁入，保留 xdcode 定制 |
| `92caad7828` | Preserve pending Effects toggles across save refreshes (#53496) | 迁入，保留 xdcode 定制 |
| `12cca49c81` | Reject duplicate Host headers in the network proxy (#53506) | 迁入，保留 xdcode 定制 |
| `f82e365922` | Deduplicate TUI modifier handling and terminal color caching (#53556) | 迁入，保留 xdcode 定制 |
| `669449b02c` | Add async compaction feature configuration (#53595) | 迁入，保留 xdcode 定制 |
| `be5296d467` | Prepare compaction helpers for background requests (#53606) | 迁入，保留 xdcode 定制 |
| `1a69ad3237` | Add infrastructure for speculative remote V2 compaction (#53614) | 迁入，保留 xdcode 定制 |
| `308ddf5e7c` | Allow hiding native tools while retaining the multi-agent V2 runtime (#53617) | 迁入，保留 xdcode 定制 |
| `9167a0b2d4` | Reuse background results for remote V2 compaction (#53620) | 迁入，保留 xdcode 定制 |
| `b043762f9e` | Preserve remote exec output until it is collected (#53622) | 迁入，保留 xdcode 定制 |
| `f5297229c1` | Record truncation metrics for accepted Guardian review inputs (#53628) | 迁入，保留 xdcode 定制 |
| `86966c3230` | Materialize the thread before testing a discovered daemon restart (#53631) | 迁入，保留 xdcode 定制 |
| `6042ec37b0` | Stabilize write ordering in the shared message board scenario (#53632) | 迁入，保留 xdcode 定制 |
| `90f00bd89e` | Add an environment API key fallback for Guardian Decisions (#53633) | 迁入，保留 xdcode 定制 |
| `4e7ef7d19d` | Keep spawned children provisional until the spawn result returns (#53654) | 迁入，保留 xdcode 定制 |
| `ab82af0b2c` | Share hook configuration across cloned hook views (#53657) | 迁入，保留 xdcode 定制 |
| `73bbb654b3` | Extract shared multi-agent v2 thread spawning (#53660) | 迁入，保留 xdcode 定制 |
| `a68a7a6ab0` | Improve remote exec-server test startup diagnostics (#53668) | 迁入，保留 xdcode 定制 |
| `816fa702ec` | Account for token usage from incomplete responses (#53670) | 迁入，保留 xdcode 定制 |
| `4c1cd8bbcc` | Keep discovered Apps tools available during startup recovery (#53672) | 迁入，保留 xdcode 定制 |
| `7b06e4af66` | Publish Rust releases with the GitHub CLI (#53673) | 迁入，保留 xdcode 定制 |
| `044e74fef3` | Preserve inherited context in prediction forks (#53678) | 迁入，保留 xdcode 定制 |
| `c03bbff483` | Constrain API-key model access programs to server support (#53689) | 迁入，保留 xdcode 定制 |
| `979771476a` | Account for replay caches in encrypted compaction budgets (#53691) | 迁入，保留 xdcode 定制 |
| `31434401fa` | Add keyboard selection support to the transcript view (#53693) | 迁入，保留 xdcode 定制 |
| `ffc1402633` | Interrupt and drain canceled classifier responses before socket reuse (#53698) | 迁入，保留 xdcode 定制 |
| `f6aca7dc46` | Cancel Guardian V2 classifications that exceed the tool-call lag window (#53699) | 迁入，保留 xdcode 定制 |
| `6fbfc352ca` | Enable CLI Daybreak by default and enforce background-turn eligibility (#53701) | 迁入，保留 xdcode 定制 |
| `cb7091b180` | Preserve URLs and email addresses during external agent migration (#53706) | 迁入，保留 xdcode 定制 |
| `7d0b91d5d6` | Separate MCP sandbox policy from launcher metadata (#53709) | 迁入，保留 xdcode 定制 |
| `6cc7d9cc5c` | Enable Cyber access programs for API-key sessions by default (#53717) | 迁入，保留 xdcode 定制 |
| `703d253ed6` | Wire V2 capability discovery through exec-server RPC (#53723) | 迁入，保留 xdcode 定制 |
| `f234b9b82d` | Preserve Windows startup variables and MXC child environment defaults (#54027) | 迁入，保留 xdcode 定制 |
| `938ddfb66c` | Fix Windows environment handling for remote MCP servers (#54069) | 迁入，保留 xdcode 定制 |
| `8bfe049811` | Wire background compaction into the turn lifecycle (#53800) | 迁入，保留 xdcode 定制 |
| `30c1058df6` | Preserve ChatGPT authentication for Gov MCP servers (#54170) | 迁入，保留 xdcode 定制 |
| `83086188ae` | ## New Features | 迁入，保留 xdcode 定制 |
