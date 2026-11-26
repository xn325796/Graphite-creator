---
title: Toast 与日志
aliases: [Toast, 日志抽屉, 提示层]
tags: [graphite/infra]
up: "[[基础设施总览]]"
updated: 2026-11-20
---
> 上级：[[基础设施总览]] ｜ 首页：[[知识库首页]]

# Toast 与日志

用户反馈分成两层：toast.js 负责用户动作的即时轻提示，store 的 S.logs 与日志抽屉负责完整流水与错误码。两者职责不重叠。

## Toast

- 导出 reactive 数组 `toasts` 与 `toast(message, kind='info', ms=3600)`、`dismissToast(id)`。
- 每条记录含自增 id、message 与 kind；默认 kind 为 info，失败类用 warn。
- 最多同时保留 3 条，超出淘汰最旧；定时器 3.6 秒后按对象身份移除。
- `dismissToast` 只负责手动关闭，定时回调自行判空，不依赖撤销。

## 日志

`log(msg)` 在 project.js 内，向 `S.logs` 追加 `{time, msg}`，时间取 HH:MM:SS，超过 500 行时移除最早一条。日志抽屉由 `ui.logOpen` 控制展开。

## 分工

- toast：已保存、已复制、重命名失败、校验完成等一句话反馈。
- 日志：命令错误码、导入摘要、构建摘要、校验前 10 条、游戏启动目录等可追溯信息。

> **2026-11-26 增补**：命令失败的可见性统一在 `store/project.js` 的 `call()` 包装里——除 `no-project` 外，
> 失败先写日志再弹错误 toast（`toast.cmdFailed`，6 秒）。同一失败按 **`命令名|code|message`** 三元组
> **3 秒去重**（`ERROR_TOAST_DEDUP_MS`），不同命令的相同文案互不吞并；`no-project` 保持静默降级。
> 字段提交被拒（`setField` 返回 false）会 toast + 草稿回滚并标红，不再静默。

> [!info] 三层反馈
> 输入非法另有字段标红（invalids），不要把所有失败都塞进 toast，否则关键诊断会被 3 条上限挤掉。

> [!warning] toast 不是日志缓存
> 3 条上限是产品口径，不是队列能力；需要留存的诊断信息必须写入 S.logs。

> [!tip] 错误提示
> 壳错误的 code 与 message 应成对写入日志，toast 只给用户看得懂的一句话。

## 相关笔记

- [[基础设施总览]] —— 底座的整体边界
- [[IPC 桥接]] —— 错误码的来源
- [[项目生命周期]] —— 保存、构建、启动的反馈
