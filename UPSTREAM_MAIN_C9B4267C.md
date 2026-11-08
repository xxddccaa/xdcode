# 选择性迁入 Codex main：c9b4267c

本轮在现有工作树上继续开发，不重置上一轮 31 项回移及个人模型管理增强。产品版本仍为 xdcode **0.167.0**，真实 Codex 基线仍为 **0.165.0**。候选目标固定为 `c9b4267c370a755da12ec1c8648c2451163212b7`，核对日期为 **2026 年 11 月 8 日**；这不是完整同步 main，也不表示当前已安装程序启用了这些改动。

## 使用入口与限制

常规可靠性、权限、模型目录及输入修补直接生效。以下实验默认关闭，可在 `/experimental` 保存，或在隔离的开发程序中显式传入 CLI 开关；重启要求是功能合同的一部分。

| 实验             | CLI 开关                                               | 前提与关闭语义                                                                                                                           |
| ---------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| 会话侧栏         | `--enable tui_workspace`                               | 使用 fullscreen transcript 与 alternate screen，重启 TUI；隐藏只收起视图，关闭标签保留已保存聊天，归档移出保存列表。关闭实验不删除历史。 |
| stdio MCP 懒启动 | `--enable mcp_lazy_stdio_startup`                      | 重启所属 App Server；旧协议保留独立启动和兼容降级。                                                                                      |
| MCP 连接共享     | `--enable mcp_connection_sharing`                      | 重启所属 App Server；只共享协商支持租约的服务。权限、工作目录、审批、认证、超时和取消按会话隔离。                                        |
| 本地消息板       | `--enable multi_agent_v2 --enable agent_message_board` | 重启所属 App Server，仅服务已获授权的 agent；启用不创建或恢复接收方。                                                                    |
| 消息板 MCP       | 再加 `--enable agent_message_board_mcp`                | 同时要求消息板与 multi_agent_v2；本地 HTTP/MCP 实现不依赖 Cloud。                                                                        |
| skill 增量目录   | `--enable skill_catalog_deltas`                        | 重启所属 App Server，按当前文件系统权限发现和读取；关闭后恢复完整目录工作流。                                                            |

`xdcode agents --no-daemon` 使用内嵌 App Server，不能与 `--remote` 合用。Provider 浏览和编辑入口为 `/config` → `Model providers`，使用所属服务的配置读取及批量写入接口；只有明确保存的字段被修改，不自动切换会话、不改写项目 Default/Plan 选择。发送回执不确定时保留带未确认状态的输入，不自动重放；查到对应历史客户端消息 ID 后才消除未确认项。

本轮保留内嵌与既有 `--remote` 入口，迁入连接所有权和恢复修补。选择器只查询所属显示后端；**不提供任意多个本地 backend 的注册与切换**。系统代理仅接入支持的平台，Linux 仍遵守现有代理环境，不修改本机代理设置。

## 按批次交付

1. 存储、会话与执行：启停及通知竞态、取消归档子树、fork 历史边界与祖先索引、SQLite 有界写锁与失败反馈、输出背压及终止预算错误、下一回合临时环境重试。
2. 人工审批与权限：网络审批生命周期和同 endpoint 串行、统一权限 profile、非交互项目信任、skill 文件系统权限及 Linux deny-glob、现有 Windows 沙箱路径修补。
3. 线程目录与 Provider：按线程有效配置/认证拥有者解析目录，子 agent 继承与显式覆盖、provider/关系落盘、冲突和原子配置事务、可编辑用户层 Provider 表单；保留精确目录 ID、自定义/服务默认 effort、Default/Plan 与密文边界。
4. MCP、skills、消息板：协商共享/懒启动、取消与租约隔离、持久网络决定、显示名与增量目录、最多八个并发插件加载、512 字符 starter prompt、本地持久消息板/通知/删除清理。
5. TUI：内嵌命令中心、未读阅览确认、异步侧栏与独立草稿/附件/滚动、Markdown 与回执、reasoning 恢复、粘贴/剪贴板/图片布局、标题异步保存、已返回且有效的安全解释与验证后的 fork 改写。

本地受管网络约束兼容 `[experimental_network]`、`[network_proxy]` 与 `[permissions.network]`。后两种写法需要支持本轮兼容修补的客户端；多客户端共享策略仍宜使用旧写法。混用顶层与嵌套写法（包括跨配置层混用）会明确报错，网络与文件系统约束保留独立优先级；这不启用托管配置获取。

配置加载器保留已经计算的项目信任决定，使显式 `exec --trust` 能保存并重新加载项目授权；明确不信任的项目仍拒绝该快捷路径。不通过放宽项目配置或 MCP 的默认读取权限解决授权问题。

本地适配保留 Git 2.34.1 的 worktree 列举和未认领目录清理。旧 Git 不支持 NUL 列举，且按行输出不能可靠区分路径内换行，因此回退到 Git 注册信息，并继续核验所属仓库、目录布局和双向链接；不降低路径验证要求，也不升级本机 Git。

## 来源台账

状态针对所选行为，不代表完整取入该提交。直接移植只做必要的 xdcode 身份适配；涉及本地目录、权限、历史或协议差异的部分按适配移植记录。原有来源记录不改写，完整 SHA 和限定范围追加于 `upstream.json`。

本地构建适配：Core 的编译期类型递归上限设为 `256`，与现有 App Server、TUI 和 exec 保持一致，避免所选异步历史迁移组合在 release 布局计算中超过默认上限。Rust 仍为 `1.99.1`，不改变运行时递归、权限或依赖范围；首次失败构建的证据保留，修补后使用新的构建 ID 和源码摘要。

| 批次 | 来源提交   | 状态     | 原始上游主题                                                                      | 证据    |
| ---- | ---------- | -------- | --------------------------------------------------------------------------------- | ------- |
| 1    | `020e0f77` | 已具备   | Preserve timestamp ties when paginating threads (#58886)                          | `E2`    |
| 1    | `0ed9d6fa` | 直接移植 | Handle `budget_reached` as a terminal session budget error (#59228)               | `E3`    |
| 1    | `2f02d846` | 适配移植 | Support update-boundary paging across forked thread histories (#59793)            | `E2`    |
| 1    | `4e2dfd22` | 适配移植 | Retry transient environment startup failures on the next turn (#57866)            | `E3`    |
| 1    | `575cc535` | 直接移植 | Fix app-server thread listener startup and shutdown races (#57787)                | `E1`    |
| 1    | `6562117a` | 直接移植 | Preserve process output and control replies under backpressure (#57569)           | `E3`    |
| 1    | `6ba3a846` | 已具备   | Restore spawned descendants when unarchiving a thread (#57635)                    | `E2`    |
| 1    | `7f4d53e9` | 直接移植 | Release app-server thread runtimes after shutdown (#57788)                        | `E1`    |
| 1    | `b7ca29d2` | 适配移植 | Recover inherited history from alternate rollout copies (#58959)                  | `E2`    |
| 1    | `ba0dfd38` | 适配移植 | Prefer indexed rollout paths when resolving ancestors (#58960)                    | `E2`    |
| 1    | `c92700d2` | 适配移植 | Backfill exec completion items through paginated history (#57800)                 | `E2`    |
| 1    | `df769889` | 直接移植 | Reduce SQLite write overhead for thread activity and history (#60006)             | `E2`    |
| 1    | `ff2346f3` | 直接移植 | Bound waits for the thread writer coordination lock (#57360)                      | `E2`    |
| 2    | `0b21a5b5` | 适配移植 | Respect filesystem permissions when discovering repository skills (#59280)        | `E4/E8` |
| 2    | `11ca3f75` | 适配移植 | Require explicit project trust in non-interactive runs (#60041)                   | `E4`    |
| 2    | `136d891a` | 适配移植 | Support network_proxy in managed requirements (#57286)                            | `E4`    |
| 2    | `dfcb5b59` | 适配移植 | Support managed network requirements under permissions.network (#58182)           | `E4`    |
| 2    | `2889d8e1` | 适配移植 | Normalize turn context permissions to a required profile (#59963)                 | `E4`    |
| 2    | `2a32e7c9` | 适配移植 | Expand Linux sandbox deny globs in process (#59711)                               | `E4`    |
| 2    | `4969675f` | 适配移植 | Resolve network approval prompts when their requests end (#58350)                 | `E4`    |
| 2    | `526f6099` | 适配移植 | Preserve filesystem authority when loading skills (#59279)                        | `E4/E8` |
| 2    | `80f469ce` | 适配移植 | Serialize manual network approval prompts per endpoint (#58351)                   | `E4`    |
| 2    | `9a7213ca` | 适配移植 | Resolve workspace permissions at the sandbox launch boundary (#59530)             | `E4`    |
| 2    | `a90c8084` | 适配移植 | Centralize Windows sandbox filesystem override resolution (#60042)                | `E4`    |
| 2    | `ad1dbba3` | 适配移植 | Scope file change approval cancellation to the requesting turn (#59910)           | `E4`    |
| 2    | `af2e52c8` | 适配移植 | Fail closed on unreadable glob search root metadata errors (#57186)               | `E4`    |
| 2    | `d92aae55` | 适配移植 | Centralize filesystem permission resolution for sandbox enforcement (#59009)      | `E4`    |
| 3    | `12dc86be` | 适配移植 | Expose model metadata from each thread's catalog and settings (#59958)            | `E5`    |
| 3    | `44ccf394` | 适配移植 | Preserve model catalogs when starting child agents (#60049)                       | `E5`    |
| 3    | `74bfe33e` | 适配移植 | Add provider configuration editing to the TUI (#59779)                            | `E5`    |
| 3    | `cf39544a` | 适配移植 | Resolve startup models using thread creation options (#59987)                     | `E5`    |
| 3    | `daa69c41` | 适配移植 | Allow selecting a configured provider in `model/list` (#59776)                    | `E5`    |
| 3    | `e115d5f9` | 直接移植 | Keep threads on their saved model provider when defaults change (#59503)          | `E5`    |
| 3    | `f1b94216` | 适配移植 | Add model provider browsing to the TUI configuration panel (#59541)               | `E5`    |
| 4    | `0712633d` | 适配移植 | Release message-board state before HTTP shutdown returns (#58933)                 | `E7`    |
| 4    | `0768988e` | 适配移植 | Resolve skill display names in read and installations tools (#59954)              | `E8`    |
| 4    | `0bc73bbe` | 适配移植 | Add opt-in protocol discovery for local stdio MCP servers (#57210)                | `E6`    |
| 4    | `119a547a` | 适配移植 | Prevent idle Windows MCP pipes from blocking unrelated I/O (#59726)               | `E6`    |
| 4    | `18c6dbe5` | 适配移植 | Add a standalone HTTP MCP server for the agent message board (#58904)             | `E7`    |
| 4    | `358e4645` | 适配移植 | Allow plugin starter prompts up to 512 characters (#60048)                        | `E8`    |
| 4    | `3cacafa0` | 适配移植 | Add incremental skill catalogs behind `skill_catalog_deltas` (#57262)             | `E8`    |
| 4    | `4437ff31` | 适配移植 | Extract agent message board hosting into a dedicated crate (#59473)               | `E7`    |
| 4    | `4a971492` | 适配移植 | Expose thread APIs for message board access (#59154)                              | `E7`    |
| 4    | `5e3496e0` | 适配移植 | Share eligible local stdio MCP connections across chats (#57211)                  | `E6`    |
| 4    | `632eee03` | 适配移植 | Add fixed read views for local message boards (#59993)                            | `E7`    |
| 4    | `6c4fa868` | 适配移植 | Add an in-memory launcher for the message-board MCP server (#58909)               | `E7`    |
| 4    | `7ccf78de` | 适配移植 | Preserve MCP sharing discovery when deferring cached stdio startup (#60037)       | `E6`    |
| 4    | `7d1492ff` | 适配移植 | Isolate MCP inputs and timeouts for callers sharing a connection (#57196)         | `E6`    |
| 4    | `7f60546e` | 适配移植 | Load up to eight plugins concurrently (#57819)                                    | `E6`    |
| 4    | `8fdf6472` | 适配移植 | Add per-chat leases for shared MCP connections (#57978)                           | `E6`    |
| 4    | `9c23d93d` | 适配移植 | Add opt-in MCP discovery for in-memory agent message boards (#58973)              | `E7`    |
| 4    | `9da07658` | 适配移植 | Move agent message-board backend construction into the extension (#58986)         | `E7`    |
| 4    | `a9e474c8` | 适配移植 | Add an MCP adapter for message-board tools (#58341)                               | `E7`    |
| 4    | `aa1399cb` | 适配移植 | Forward caller metadata on MCP resource reads (#59509)                            | `E6`    |
| 4    | `af9a98b0` | 适配移植 | Add opt-in lazy startup for cached stdio MCP servers (#60039)                     | `E6`    |
| 4    | `b7d2321e` | 适配移植 | Derive MCP network policy and preserve saved denials (#60046)                     | `E6`    |
| 4    | `bfdbf01b` | 适配移植 | Move agent message board host integration to app-server (#59162)                  | `E7`    |
| 4    | `d786ff91` | 适配移植 | Extract channel notification formatting into `PostPreview` (#59688)               | `E6`    |
| 4    | `f05aa41d` | 适配移植 | Route message-board notifications through agent mailboxes (#59139)                | `E7`    |
| 4    | `f7167ac8` | 适配移植 | Move agent message board cleanup into the app server (#59167)                     | `E7`    |
| 4    | `fb2b06a8` | 适配移植 | Surface MCP server startup errors when preparing cached tool calls (#60038)       | `E6`    |
| 5    | `025f6d71` | 适配移植 | Show unread results in the TUI command center (#58263)                            | `E9`    |
| 5    | `0389e80f` | 适配移植 | Refactor TUI clipboard and composer input handling (#60008)                       | `E10`   |
| 5    | `054128c9` | 适配移植 | Fix TUI assistant messages interrupted by tool execution (#59700)                 | `E10`   |
| 5    | `0ed34eb9` | 适配移植 | Ensure side-chat policy reaches the first model request (#60067)                  | `E9`    |
| 5    | `118aae2a` | 适配移植 | Explain safety policy blocks with turn-scoped recourse in the TUI (#57363)        | `E11`   |
| 5    | `1455fda7` | 适配移植 | Attach typed punctuation to completed skill mentions (#60180)                     | `E10`   |
| 5    | `194f2662` | 适配移植 | Add a sidebar chooser and floating workspace shortcut panels (#59739)             | `E9`    |
| 5    | `1cc3cf80` | 适配移植 | Skip non-regular files when detecting pasted image paths (#57774)                 | `E10`   |
| 5    | `26f73fba` | 适配移植 | Preserve literal/shell bang semantics in composer draft snapshots                 | `E10`   |
| 5    | `3cf4899c` | 适配移植 | Render delegated thread messages as Markdown in the TUI (#59789)                  | `E10`   |
| 5    | `44252e3f` | 适配移植 | Fix reasoning summary restoration in the TUI (#59698)                             | `E10`   |
| 5    | `4f6acbf0` | 适配移植 | Preserve transcript interaction when switching panes with arrow keys (#59472)     | `E9`    |
| 5    | `536ae056` | 适配移植 | Enable TUI task tools for embedded app-server sessions (#58240)                   | `E9`    |
| 5    | `57e64682` | 适配移植 | Persist local workspace side chats and add tab close controls (#60152)            | `E9`    |
| 5    | `5e07d1f6` | 适配移植 | Support OSC 52 clipboard forwarding through GNU screen (#58716)                   | `E10`   |
| 5    | `5fab2893` | 适配移植 | Enable resizing the sidebar chooser (#59824)                                      | `E9`    |
| 5    | `6b2a2757` | 适配移植 | Add shortcuts to focus the main composer and side conversation (#59208)           | `E9`    |
| 5    | `6c0db39b` | 适配移植 | Offer safer rewrites after safety blocks in the TUI (#57365)                      | `E11`   |
| 5    | `7689f21b` | 适配移植 | Enable the agent command center for embedded sessions (#58238)                    | `E9`    |
| 5    | `78b1dda5` | 适配移植 | Render typed prompts as Markdown in the TUI (#59983)                              | `E10`   |
| 5    | `7f1c0997` | 适配移植 | Expose system proxy support as an experimental setting (#58157)                   | `E9`    |
| 5    | `8044c686` | 适配移植 | Add keyboard and mouse navigation for sidebar tabs (#60181)                       | `E9`    |
| 5    | `814347ec` | 适配移植 | Improve sidebar chat selection and tab hover highlighting (#59928)                | `E9`    |
| 5    | `8a17396d` | 适配移植 | Fix live stream reflow at the allocated conversation width (#58927)               | `E9`    |
| 5    | `8a1d4253` | 适配移植 | Keep automatic thread title saves off the TUI event loop (#58970)                 | `E10`   |
| 5    | `8c811e8d` | 适配移植 | Support rendering TUI conversations within an inset area (#58718)                 | `E9`    |
| 5    | `9df200a0` | 适配移植 | Add sidebar tabs and nonblocking side-chat creation (#59767)                      | `E9`    |
| 5    | `a037e65e` | 适配移植 | Preserve client metadata through queued submissions (#59826)                      | `E10`   |
| 5    | `a142c90e` | 适配移植 | Handle TUI message submission receipts and preserve uncertain drafts (#59817)     | `E10`   |
| 5    | `ba057e43` | 适配移植 | Drain queued side-conversation events before switching focus (#59458)             | `E9`    |
| 5    | `bf877af4` | 适配移植 | Enable the agent command center in embedded mode (#58239)                         | `E9`    |
| 5    | `c2e4631a` | 适配移植 | Preserve unrelated side conversations when archiving or deleting threads (#58237) | `E9`    |
| 5    | `c8e73bf7` | 适配移植 | Use the native clipboard for WSL SSH sessions with X11 forwarding (#57504)        | `E10`   |
| 5    | `d63a3422` | 适配移植 | Support multiple side chats in the TUI sidebar (#59784)                           | `E9`    |
| 5    | `d7b77a5e` | 适配移植 | Show unread sessions and acknowledge viewed transcripts in the picker (#58264)    | `E9`    |
| 5    | `db227aed` | 适配移植 | Add opt-in side conversation panes to the TUI (#58992)                            | `E9`    |
| 5    | `db3c4441` | 适配移植 | Add directional conversation focus shortcuts in workspace mode (#59287)           | `E9`    |
| 5    | `e8e3197e` | 适配移植 | Make side conversation panes resizable and interactive (#59527)                   | `E9`    |
| 5    | `e977cb39` | 适配移植 | Preserve tool output versions and history order in rollout traces (#57311)        | `E10`   |
| 5    | `eab26037` | 适配移植 | Reuse connected backends in the in-app resume picker (#58280)                     | `E9`    |
| 5    | `eb15d3a0` | 适配移植 | Add a shortcut to hide the focused side conversation pane (#59130)                | `E9`    |
| 5    | `ed8eac40` | 适配移植 | Add keyboard controls for TUI pane resizing and zoom (#59595)                     | `E9`    |
| 5    | `f732dce4` | 适配移植 | Enable pane-local image previews in tmux (#59579)                                 | `E9`    |

## 证据索引

每个来源条目关联以下证据组。具体命令、每次尝试、通过数、失败及跳过项以交付目录的 `test-results.json` 和 `evidence-index.json` 为准；有证据组不等于该组全部通过。源码检查、行为测试、构建和真实运行分开记录。

| 编号 | 覆盖与行为测试入口                                                                                                                                                                               |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| E1   | App Server 生命周期、旧监听器关闭通知、连接关闭与 runtime 释放；`message_processor_thread_lifecycle_tests`、`connection_handling_stdio`。                                                        |
| E2   | SQLite、历史分页/并列、fork/祖先恢复/完成回填、归档子树；`xdcode-thread-store`、Core 历史/保留上下文、App Server fork/read/archive。                                                             |
| E3   | 输出背压与控制回复、临时环境重试、终止预算错误；exec-server 输出测试、Core retry/environment、API/TUI budget 错误。                                                                              |
| E4   | 人工网络审批、权限 profile/deny-glob、非交互信任；Core 网络审批、`xdcode-sandboxing`、exec trust、原生 Linux 运行。                                                                              |
| E5   | 原子配置与 provider 编辑、按线程目录、双 Responses 服务真实路由/认证/effort/模式和 `/new` 继承；config/model tests、Core model switching、App Server scoped provider、TUI provider/new-session。 |
| E6   | MCP 协议发现、缓存、共享/降级、请求上下文、租约和取消；rmcp 2026 stdio、`connection_pool`、`runtime_shared_connection_tests`、Core MCP。                                                         |
| E7   | 本地消息板持久化、通知、请求隔离和清理；message-board/client/host 测试、Core agent-message-board 场景。                                                                                          |
| E8   | skill 权限、显示名、目录增量与去重、并发加载和 starter prompt；`xdcode-skills-extension`、Core repository grants/catalog、插件加载测试。                                                         |
| E9   | `agents --no-daemon`、未读阅览确认、侧栏恢复/焦点/附件/关闭与归档、backend 所有权；TUI agents/key-chords/session/side-chat、原生 PTY。                                                           |
| E10  | Markdown、输入/回执/草稿、前导 `!`/粘贴/撤销、工具/历史/图片布局；TUI composer/chatwidget/history/transcript/receipts。                                                                          |
| E11  | 外部安全元数据的规范键和旧别名、无效数据保留原错误、验证分叉及设置后才发送；API SSE、TUI recourse metadata/boundary 和拒绝继承设置的实时用例。                                                   |
| E12  | 协议/schema/SDK 生成一致性、npm execve/fallback、交付脚本、Linux release 与隔离网络运行；Python/JS/TS 检查、构建回执和运行报告。                                                                 |

## 排除项

| 行为                                                                                     | 状态 | 原因                                                                                                        |
| ---------------------------------------------------------------------------------------- | ---- | ----------------------------------------------------------------------------------------------------------- |
| Cloud、托管登录、新 Gateway OAuth/device-code、遥测上传、自动升级与自动插件下载          | 排除 | 不符合个人本地部署边界；现有认证和本地能力不删除。                                                          |
| Guardian/reviewer 模型、Daybreak 接入及额外安全审查服务                                  | 排除 | 保留人工审批＋沙箱；安全解释只使用当前服务已返回的有效元数据。                                              |
| Vertex/Bedrock 新向导、独立远程执行器部署、Code Mode Only                                | 排除 | 不增加本轮不需要的部署或工作流。                                                                            |
| UI 插件宿主、扩展状态栏、多 backend 注册框架、host cycling 与新默认 provider 覆盖 `/new` | 排除 | 未接通生产入口或破坏已确认选择继承；不替上游完成实验架构。                                                  |
| 远程目录刷新后自动归一化 effort、Bazel/V8 源码构建与最低 Node 版本提升                   | 排除 | 保留个人 effort/服务默认与现有工具链。npm 原生 execve 仅在非 Windows 且运行时支持时使用，其他情况仍 spawn。 |

## 验证证据与交付

行为测试、协议生成、构建及隔离运行分别记录于 `target/xdcode-verification-main-c9b4267c/test-results.json` 和该目录的运行报告。测试编译失败、行为失败、既有无关失败及平台未验证项分别记录，不将模拟服务结果视为真实厂商在线验收。

开发包采用新的构建 ID `main-c9b4267c-20261108-r1`。构建脚本冻结源码摘要并如实标记 dirty，产物附构建回执与校验和；报告在忽略的交付目录生成，不改变冻结后的源码。本轮不创建分支、不提交、不发布、不安装，也不替换当前正在使用的 xdcode。macOS/Windows 未做原生验证，真实厂商在线验收未执行。
