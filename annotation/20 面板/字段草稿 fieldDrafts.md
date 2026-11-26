---
title: 字段草稿 fieldDrafts
aliases:
  - fieldDrafts
  - 字段草稿
  - 草稿状态
tags:
  - graphite/panel
up: "[[面板总览]]"
updated: 2026-11-26
---

> 上级：[[面板总览]] ｜ 首页：[[知识库首页]]

把输入框的即时输入接到 store 的落盘命令上，是 [[检查器 InspectorPane]] 与 [[资源列表 AssetsListPane]] 共用的编辑缓冲。物化与选中时序见 [[状态层总览]]。

> **2026-11-26 就地更正**：原文「`onInput` 乐观提交、每击键一条 undo」已废止。z1 起击键只写草稿，
> 提交发生在 blur / enter / 显式 Apply；`enter + blur` 双触发由 `lastCommit` 判重，改名由 `pendingNames` 判重。
> 依据：`src/shared/fieldDrafts.js`。

## 模块级状态

`drafts` 是模块级 reactive 对象，`invalids` 是模块级 reactive Set；另有 `lastCommit` 与 `pendingNames` 两个普通 Map 做去重。它们都是全局单份、跨记录共享，不随组件卸载而清空。

键由 `draftKey(r, f)` 生成，格式为 `${r.id}:${field}`。所有读写草稿的入口都必须用同一个键，否则标红与取值会对不上。

## 惰性初始化

首次调用 `draft(r, f)` 时才从 `r.fields[f]` 取值：空值归一为空串，对象序列化为 JSON 文本，其余转字符串。草稿本身不做类型转换，数字框保留用户原文，类型判断与合法性检查交给 `setField` 后的壳返回值。

## 提交路径

- `onInput(r, f, ev)`：**只写草稿**，不发命令。逐击键提交会让撤销栈被一次编辑刷爆。
- `commitDraft(r, f)`：blur / enter / Apply 的统一入口。值未变则不提交；`lastCommit` 记住已提交值，`enter` 紧接 `blur` 只产生一条 undo。壳返回 `false` 时标红并把草稿回滚到提交前值；`true` 清除标记；`null` 保持现状。
- `applyDraft(r, f)`：直接转调 `commitDraft`，供 raw、struct、stack 等需要显式 Apply 的控件。
- `toggleBool(r, f)`：布尔开关取反后立即提交。
- `onRefChange(r, f, v)`：引用字段空值转 `undefined` 再立即提交。
- `commitName(r, ev)`：本地校验失败时记录日志、toast 并回滚输入框；成功则 `clearDrafts()` 并把新名记进 `pendingNames`，`enter+blur` 双触发不再重复改名。

> [!warning] `invalids` 必须与 `drafts` 同步清理。只删草稿不删标记，会让下一次同键输入误标红；`clearDrafts` 同时清 `lastCommit` 与 `pendingNames`，否则换记录后同名值会被误判为「已提交」。

## raw 编辑

`toggleEditRaw` 进入编辑态时把 `r.raw` 抄进 `S.ui.rawDraft`，避免直接编辑 store 数据。`doApplyRaw` 在应用成功后先 `await settleSelection()`，再退出编辑态并清空草稿；失败时保留编辑态与草稿。失败不回退输入。

## 清空时机

选中记录变化（`home.vue` 监听 `S.selectedId`）会立即 `clearDrafts()`。撤销与重做由 `lib/actions.js` 的 `doUndo/doRedo` 先 `settleSelection()` 再清草稿，并额外 `invalidateImportedSprites()`（贴图导入在撤销栈里，缩略图缓存必须作废）。记录切换、重命名与 undo 都被视为新上下文，未提交的编辑会被丢弃，这是有意取舍。

## 等待物化落定

选择物化是异步的：`selRecord` 在 fields 到位前不暴露。若提前清草稿或初始化，会把空窗口的空串当成新草稿，数据到达后不再重建，输入框永久显示旧值或空值。

> [!tip] 新增“应用后刷新选择”的流程时，顺序固定为 `settleSelection()` 之后再 `clearDrafts()`。

## 相关笔记

- [[检查器 InspectorPane]]
- [[资源列表 AssetsListPane]]
- [[状态层总览]]
