---
title: 菜单栏 MenuBar
aliases:
  - 菜单栏 MenuBar
  - MenuBar
  - 菜单栏
tags:
  - graphite/view
up: "[[视图层总览]]"
updated: 2026-11-26
---
> 上级：[[视图层总览]] ｜ 首页：[[知识库首页]]

# 菜单栏 MenuBar

`components/layout/MenuBar.vue` 一个组件同时渲染标题栏 `titlebar`、菜单栏 `menubar` 与工具栏 `toolbar`，合计占主壳 `--h-titlebar`(36) + `--h-toolbar`(≥44)。**2026-11-26 更正**：它不再是全局快捷键注册点——全局键盘监听在 `home.vue`，菜单/命令面板/快捷键共用 `lib/actions.js` 的命令表。

> **2026-11-26 就地更正（原文口径保留在下文）**：z1 之后菜单从七个变八个（新增 `run`），菜单项不再由本组件手写，
> 而是 `MENU_LAYOUT` 的 id 列表指向 `lib/actions.js` 的 `commands`；窗口按钮改为真实的 Tauri 窗口控制
> （`lib/window.js`），关闭入口统一走 `requestClose()` → 壳的 `CloseRequested` 拦截。
> 依据：`src/lib/actions.js` 的 `MENU_LAYOUT` / `commands`、`src/lib/window.js`、`MenuBar.vue::requestClose`。

## 菜单数据

`MENU_LAYOUT` 是 `{ id, label, items }` 数组，顺序固定八项：`project`、`edit`、`view`、`run`、`format`、`language`、`window`、`help`。`items` 里是命令 id 或 `'|'` 分隔线，`menuItems(menu)` 到 `commands` 表里取标题、快捷键、勾选态、禁用态与提示。原文的三元组 `menuDefs` 与「三处必须同步」已被命令表取代：现在新增动作只需在 `commands` 里加一条（可选 `menu` 字段归组），再在 `MENU_LAYOUT` 的 `items` 里列 id。

> [!warning] 命令表是单一来源
> 菜单、命令面板（`ArCommandPalette`）、快捷键帮助（F1）与全局快捷键都从 `commands` 读取。不要在 `MenuBar.vue` 里另写一份动作或快捷键字符串。

## 语言、格式与主题

`switchLang(l)` 只写 `S.ui.lang` 并记一条日志；`menus` 每次渲染都调 `tr()`，切语言立即生效、无需重挂组件。文案表见 [[国际化 messages]]。格式转换（`format.toJson` / `format.toHjson`）调用 `project_convert_format`，只改模型，落盘要再保存。`view.themeDark/Light/System` 调 `lib/theme.js` 的 `setTheme`，只切 `<html data-theme>`。

## 窗口行为

最小化、最大化/还原、关闭都是真实窗口操作：`lib/window.js` 包装 `getCurrentWindow()`，浏览器（vite dev）下降级为 fullscreen/no-op。标题栏 `data-tauri-drag-region` 负责拖动与双击最大化，四边热区由 `WindowResizer` 负责缩放。

关闭与退出统一走 `requestClose()` → `closeWindow()`：Tauri 的 `CloseRequested` 被壳拦下并广播 `window-close-requested`，dirty 时弹 `closeConfirm`，确认后由 `window_confirm_close` 真正关窗。原文的「先 `confirm`、只把 `S.ui.closed` 置真、不杀进程」已废弃（`S.ui.closed` 现在只剩一个未使用的字段）。

## 快捷键

**2026-11-26 更正**：监听挂在 `home.vue` 的 `window`（`onGlobalKey`），`MenuBar` 内不再注册。组合键认 `Ctrl` 或 `Meta`：S 保存、Z 撤销、Y 重做、F 聚焦搜索、B 构建、K / Shift+P 命令面板、N 新建、O 打开、`\` 极简模式、1–5 切页签；单键 F1 快捷键帮助、F5 校验、F6 安装并启动、F10 聚焦菜单栏。输入框内的 Ctrl+Z/Ctrl+Y 由 `inField` 守卫放行给浏览器原生撤销；有模态打开时全局快捷键让位（`modalState.open > 0`）。撤销与重做固定 `undo() → settleSelection() → clearDrafts() → invalidateImportedSprites()`。

> [!tip] 快捷键归属
> 除 `home.vue`（全局）与画布（Delete/Backspace 局部）外，任何地方都不要再监听 `keydown`，否则 Ctrl+S 会保存两次。按钮入口见 [[命令条 CommandBar]]。

## 相关笔记

- [[页面壳 Home]]
- [[命令条 CommandBar]]
- [[国际化 messages]]
- [[视图层总览]]
