---
title: 命令条 CommandBar
aliases:
  - 命令条 CommandBar
  - CommandBar
  - 命令条
tags:
  - graphite/view
up: "[[视图层总览]]"
updated: 2026-11-26
---
> 上级：[[视图层总览]] ｜ 首页：[[知识库首页]]

# 命令条 CommandBar

命令条是 `MenuBar.vue` 模板里的 `.toolbar` 段（`role="toolbar"`），高度 `--h-toolbar`（44，窄窗可增高）。它不单独成组件：与标题栏共享同一个 scoped 组件状态。

> **2026-11-26 就地更正**：控件顺序与行为已改。原文的「校验图标按钮、撤销、搜索、打开目录、安装启动、停止、网格、吸附、校验文案、构建」
> 现在是：搜索 → 撤销/重做 → 安装并启动/停止 → 网格/吸附 → 校验/构建；「打开目录」按钮已移除（`run.openDir` 命令仍在菜单里且恒禁用）。
> 依据：`MenuBar.vue` 的 `.toolbar` 模板与 `lib/actions.js` 的 `commands`。

## 控件顺序

从左到右：

- 搜索：`ArInput`（`data-global-search`）绑定 `S.ui.search`，回车 `searchNext()`；`Ctrl+F` 只聚焦，不执行。
- 撤销 / 重做：`doUndo()` / `doRedo()`，顺序固定 `undo()/redo() → settleSelection() → clearDrafts() → invalidateImportedSprites()`；禁用态取 `canUndo()` / `canRedo()`。
- 安装并启动 / 停止：`run.launch` / `run.stop`；可用性由 `S.project` 与 `S.ui.gameRunning` 控制。
- 网格 / 吸附：`ArSwitch` 直接写 `S.ui.grid` / `S.ui.snap`，并在回调里补一条开关日志。
  - 网格已接线：`CanvasPane` 的 `.cv-canvas` 在 `!S.ui.grid` 时加 `.plain`，隐藏点阵背景。
  - 吸附**尚未接线**：全仓除开关与日志外没有消费者，留待专项；不要以为打开它就会吸。
- 校验：`doValidate()` → `validateNow()`，按是否有 error 决定 toast 变体。
- 构建：`startBuild()`，先走 `PathPicker` 选目标与目录/zip 格式，再拍构建计划并写盘。

> [!info] 图标来源
> 全部是内联 SVG，尺寸在 `menubar.css` / 组件内联样式里固定，不引外部图标库。

## 与快捷键的关系

命令条上的动作都在 `lib/actions.js` 的 `commands` 表里有同 id 条目，快捷键与菜单只是同一命令的另一种触发方式：Ctrl+S 保存，Ctrl+Z/Y 撤销与重做，Ctrl+F 聚焦搜索，Ctrl+B 构建，F5 校验，F6 安装并启动。按钮与快捷键调用同一批动作，不要各写一份逻辑。

## 相关笔记

- [[菜单栏 MenuBar]]
- [[页面壳 Home]]
- [[布局与固定画布]]
