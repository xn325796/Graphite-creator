---
title: 页面壳 Home
aliases:
  - 页面壳 Home
  - Home 页面壳
  - home.vue
tags:
  - graphite/view
up: "[[视图层总览]]"
updated: 2026-11-26
---
> 上级：[[视图层总览]] ｜ 首页：[[知识库首页]]

# 页面壳 Home

`src/views/home.vue` 是唯一页面组件，负责工作区切换之外的一切“壳”：页签、主区容器、状态栏、日志抽屉、命令面板、快捷键停靠层、全局对话框、导入进度、120s 自动保存与**全局键盘监听**。它不做字段级业务。

> **2026-11-26 就地更正**：原文写的是「六个页签、一个 ArModal、五种弹窗、关闭只置遮罩」。现状：
> 页签仍六项但 `units` 已是真实面板（不再是占位）；弹窗种类为 `about` / `meta` / `closeConfirm` / `buildDone`（导入进度移入状态栏）；
> 关闭不再置 `S.ui.closed` 遮罩，而是「壳拦截 → 前端 hold → closeConfirm → window_confirm_close」。
> 新增 `ArCommandPalette` 与 F1 快捷键停靠层，全局 `keydown` 也挂在本文件。
> 依据：`src/views/home.vue` 的模板与 `onGlobalKey`、`src/store/project.js` 的 `onCloseRequested`。

## 结构

- 页签：`wsTabs` 由 `tr('ws.*')` 生成六项（assets / canvas / units / cosmos / tech / script），script 页恒为 disabled；左右方向键在页签间移动焦点。
- 主区：assets 与 units 都是 `workspace-split` 两栏（列表 + `InspectorPane`），中间有可拖动的 `pane-resizer`；cosmos/tech/canvas 各渲染单面板；script 落到 `empty-state`。
- 状态栏：左侧脏点、项目名、记录数、路径、导入进度（含取消）；右侧 E/W 徽标（点击跳到校验页）、保存状态与时间、日志开关。
- 日志抽屉：`v-show="S.ui.logOpen"` 读 `S.logs`，store 侧上限 500 条；支持复制与清空。
- 覆盖层：`ArCommandPalette`（Ctrl+K / Ctrl+Shift+P）、`keys-dock`（F1，可搜索快捷键与手势）、`ArModal`、`ArToasts`。

> [!info] 页签懒加载
> `setWorkspace(w)` 先写 `S.ui.workspace`，再按目标页签补数据：cosmos 走 `selectDefaultCosmos()+refreshCosmosFields()`，tech 走 `refreshTechFields()`，canvas 走 `refreshBoard()`。删掉这几行，首次进入就是空白。原文的 `units` 占位说明已过时。

## 弹窗与导入

只有一个 `ArModal`，由 `S.ui.modal.kind` 决定种类：`about`、`meta`、`closeConfirm`、`buildDone`。`closeConfirm` 是 dirty 关窗对话框，footer 三键：取消 / 放弃并关闭 / 保存并关闭；`busy` 期间禁止再次提交与点遮罩关闭。导入进度不再用弹窗，而是状态栏里的 `ArProgress` + 阶段文案，来自 `import-progress` 事件（`done >= total` 时清空）。

## 生命周期

`onMounted` 依次执行 `initEvents()`、`openDemo()`，挂 `window` 的 `keydown`（`onGlobalKey`），并启动 120s 定时器：仅当 `S.dirty && S.project` 时 `save('auto')`；`onUnmounted` 必须清定时器并移除监听。另有 `watch(() => S.selectedId, clearDrafts)`，切换记录即丢草稿，防止写到新记录上。

## 全局快捷键

`onGlobalKey` 先让命令面板、快捷键层、模态优先：命令面板打开时直接返回；F1 开关快捷键层；`modalState.open > 0` 时不再处理编辑类快捷键。之后才是 F10 聚焦菜单、F5 校验、F6 启动、Ctrl+K/Shift+P 命令面板、Ctrl+S/Z/Y/F/B/N/O/`\`、Ctrl+1…5 切页签。**输入框内（`inField`）不接管 Ctrl+Z/Ctrl+Y**，交给浏览器原生撤销。

## 规避点

- 关窗必须走 `requestClose()` 或原生 X：两者都经壳的 `CloseRequested` 拦截，前端不得自行 `destroy`；取消关窗不调任何命令。
- `S.dirty` 是唯一未保存信号，别另设局部标记。
- `S.ui.closed` 已是无人消费的历史字段，不要再围绕它写逻辑。
- 页面内只允许这一处全局 `keydown`；局部监听（画布 Delete）各自负责卸载清理。

## 相关笔记

- [[菜单栏 MenuBar]]
- [[布局与固定画布]]
- [[视图层总览]]
- [[命令条 CommandBar]]
