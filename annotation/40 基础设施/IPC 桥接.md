---
title: IPC 桥接
aliases: [IPC, 桥接, invoke 与 listen]
tags: [graphite/infra]
up: "[[基础设施总览]]"
updated: 2026-11-26
---
> 上级：[[基础设施总览]] ｜ 首页：[[知识库首页]]

# IPC 桥接

ipc.js 是前端唯一的跨界通道，导出 invoke、listen、errCode、errMsg、isNoProject。状态层的 call() 与所有命令函数都建立在它之上，组件不直接引用 @tauri-apps/api。

> **2026-11-26 就地更正**：命令面 **45 条**、事件 **6 条**（实测 `src-tauri/src/lib.rs`）。
> 原文「命令面 42 条」的账面漏记了 `cosmos_scene`，本批已按 `generate_handler!` 逐条核对：42 → 43（补记 cosmos_scene）→ 44（window_confirm_close）→ 45（window_close_hold）；
> `sprite_import` 是 2026-11-25 的第 42 条，只是原表没列。事件表补第 6 条 `window-close-requested`。

## 两类通道

- `invoke(cmd, args)`：请求-响应。包装 @tauri-apps/api/core 的 invoke，参数缺省为空对象。
- `listen(event, cb)`：事件订阅。包装 @tauri-apps/api/event，把 e.payload 解包后交给回调。浏览器（vite dev 无 Tauri 运行时）下 `listen` 会捕获同步异常并返回空 Promise，避免 `onMounted` 中断导致整页空白。

事件名与载荷形状由 Rust 侧定义，重命名前先核对壳内常量。

## 事件表（6 条）

| 事件 | 载荷 | 消费 |
| --- | --- | --- |
| `project-changed` | 无 | 全量刷新 + 选择刷新 |
| `selection-changed` | `RecordDto` 选择载荷 | `applySelection` |
| `validation-refreshed` | 无 | `refreshIssues` |
| `game-log` | 字符串数组 | 批量追加日志 |
| `import-progress` | `{stage, done, total}` | 状态栏进度条 |
| `window-close-requested` | 空（`()`） | 关窗守卫：hold → dirty 判定 → confirm/closeConfirm |

## 结构化错误

壳错误形如 {code, message}，由 bridge.rs 的 ShellError 产生。code 口径包括 no-project、not-ready、poisoned、panic、shell-exited、not-found、import-busy、encoding、fs-error、io、domain；unknown 只是兜底码。

- `encoding`（2026-11-20 起）：导入/打开时遇到**不是合法 UTF-8** 的内容/meta/bundle 文本文件。v1 口径是显式失败，不再用 U+FFFD 静默改写字节；二进制资产不受影响。
- `fs-error`：`fs_list_dir` 读目录失败（路径不存在、不是目录、权限）。
- `not-found`（关窗守卫）：重复调用 `window_confirm_close`／重复 `window_close_hold`／兜底已放行／无待确认请求／主窗口已不存在。前端对关窗命令的 `not-found` 静默忽略，其余错误才进日志与 toast。

- `errCode(e)` 取 code，缺失时返回 unknown。
- `errMsg(e)` 取 message，缺失时把错误转字符串。
- `isNoProject(e)` 判断是否为 no-project。

> [!info] no-project 不是故障
> 未打开项目时的 no-project 由调用方静默降级为空态，不写日志也不弹提示；其余错误才进入日志抽屉（2026-11-26 起还会弹去重 toast，见 [[Toast 与日志]]）。

> [!warning] 禁止额外网络通道
> src 内不得出现 fetch、XMLHttpRequest、WebSocket，也不得引入 @tauri-apps/plugin-*。跨界只允许 invoke 与 listen 两类通道。

> [!tip] 新增命令时
> 先确认 Rust 侧错误码归类，再由调用方决定静默还是记日志；不要让前端解析领域错误文本。

## 2026-11-20/21 新增命令（原文「命令面 42 条」）

| 命令 | 入参 | 返回 | 备注 |
| --- | --- | --- | --- |
| `fs_list_dir` | `path: string` | `{path, parent, entries:[{name,path,isDir,isMod,isArchive}]}` | 目录优先；`isMod`=含 mod.json/mod.hjson；`isArchive`=.zip/.jar；走 blocking 池 |
| `record_sprite` | `recordId: string` | `{dataUrl, region}` 或 `null` | `data:image/png;base64,…`；区域规则来自 mm_fmt::sprite |
| `project_convert_format` | `ext: "json"` 或 `"hjson"` | `number`（将转换条数） | 只改模型，落盘要再调 `project_save`；会广播 `project-changed` |
| `board_tidy_layout` | — | `BoardSnapshotDto` | 重排后直接回快照，无需再调 `board_snapshot` |
| `project_rebuild_links` | — | `number`（新增+删除的边数） | 会广播 `project-changed` |
| `cosmos_scene` | `recordId: string` | `CosmosSceneDto` | 星球战役三页签数据（区块网格/外观/星系）；非 planet/sector 回 `domain` 错误 |

## 2026-11-25/26 新增命令（43 → 45）

| 命令 | 入参 | 返回 | 备注 |
| --- | --- | --- | --- |
| `sprite_import`（第 42 条，原表漏列） | `recordId: string`, `sourcePath: string` | `SpriteDto` | 源 PNG 由用户经 `PathPicker`/`fs_list_dir` 选出；目标路径由 Rust 按 `sprites/<记录名>.png` 解析。成功后广播 `project-changed`，导入本身进撤销栈（`SpriteImportCommand`） |
| `window_confirm_close`（44） | — | `()` | 原生 X/Alt+F4 被拦后由前端确认；壳侧 `destroy` 主窗口，不再触发 `CloseRequested`。错误码 `not-found` |
| `window_close_hold`（45） | — | `()` | 摘除本代 3s 兜底计时、保留关窗请求；dirty 对话框因此有无限决策时间。旧前端不调它时兜底照旧放行。错误码 `not-found` |

> [!note] 嵌套 JSON 值的形状（2026-11-20 修正）
> `RecordDto.fields` / `StringTable.entries` 这类**顶层**二元组表仍按 `[[key, value], …]` 下发；
> 但**嵌套对象**（`mesh`、`research`、`objectives`）现在是真的 JSON 对象（手写 `SerializeMap`），
> 不再是二元组数组。此前 `mesh.color1` 一类读取会落空，已在本批修正。

## 相关笔记

- [[项目生命周期]] —— 生命周期命令的调用方
- [[基础设施总览]] —— 底座的整体边界
- [[前端分层守卫]] —— 扫描依赖与通道的规则
