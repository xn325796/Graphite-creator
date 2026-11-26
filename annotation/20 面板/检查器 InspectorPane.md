---
title: 检查器 InspectorPane
aliases:
  - InspectorPane
  - 检查器
tags:
  - graphite/panel
up: "[[面板总览]]"
updated: 2026-11-26
---

> 上级：[[面板总览]] ｜ 首页：[[知识库首页]]

右侧详情面板，展示 [[资源列表 AssetsListPane]] 中选中记录的结构化字段，所有修改经 [[字段草稿 fieldDrafts]] 提交。

> **2026-11-26 就地更正**：本页原文描述的是「输入即提交」的旧草稿口径。z1 起：除布尔与引用下拉外，
> 文本类字段只在 **blur/enter**（或显式 Apply）时提交，一次编辑只产生一条 undo；类名行改为三态只读/可编辑混合；
> 预览页新增「导入贴图」入口。依据：`src/shared/fieldDrafts.js`、`src/components/panels/InspectorPane.vue`。

## 四个页签

页签状态存在 `S.ui.inspectorTab`：`props` 属性、`preview` 预览、`validate` 校验、`text` 文本表。切换回 `props` 时会把当前 `sel.raw` 抄进 `S.ui.rawDraft`。

## 类名行（有效类三态）

类名来自后端下发的 `S.selClassName` + `S.selClassSource`（`declared` / `script` / `default`，解析规则见 `mm-app/src/class_resolution.rs`）：

- `declared`：内容 JSON 里写了 `type`，类名只读，提示「在内容里声明」。
- `script`：类由 `scripts/**/*.js` 的 `extend(…)` 推断，类名只读，旁边挂「由脚本推断」强调徽标（`ArBadge variant="accent"`）。
- `default`：既没声明、脚本也没推断出来，按类型默认类加载；显示警示徽标 + 「指定类」铅笔按钮。点开后输入类名并应用 → `setClass()` 写 `record_set_field{field:'type'}`；**留空应用 = 清除声明**，回落到默认类。

换选中记录会收起编辑行，避免把上一条的草稿带过来。

## 属性页

字段列表来自 `fieldSpecs()`（只渲染 `shown` 的字段），按 `control` 分派控件：

- `bool` 用 ArSwitch，走 `toggleBool` 直接提交。
- `color` 用 ArInput 加色块（`swatchFor`），输入写草稿，blur/enter 提交。
- `content` 用 ArSelect，选项来自 `candidatesFor`，首项为「—」，选择即 `onRefChange` 提交。
- `raw`、`struct`、`stack`、`stackArray`、`enumeration`、`textArray`、`sprite` 先写草稿，点 Apply 才提交。
- 其余控件输入只写草稿，blur/enter 走 `commitDraft`；被壳拒绝（`false`）时输入框标红并回滚到提交前值。

改名走 `commitName`，回车或失焦触发；`enter + blur` 双触发按 `pendingNames` 判重，本地校验失败时记录日志、toast 并回滚输入框，成功则清空全部草稿。

> [!warning] 草稿键是 `${r.id}:${field}`。改名成功后必须 `clearDrafts()`，否则旧草稿会覆盖新记录的字段。raw 编辑受 `S.ui.editRaw` 控制，提交失败不回退。

## 预览与校验

预览页显示精灵图或记录色块、显示名、类型/ID；非内联记录有「导入贴图」按钮（经 `PathPicker` 选源 PNG，调 `sprite_import`，成功后缩略图缓存就地更新并重拉校验）；下方最多五个数值型字段，以及记录色、类型色、面板底色三枚色卡。

校验页顶部显示 error/warn 汇总，校验中显示状态徽标。点击问题行走 `jumpToIssue`：切到 assets 工作区，再 `setType` 与 `select`，让选中项与问题定位一致。问题文本由 `issueText` 本地化。

## 文本页

`strings` 表按 key 排序，过滤词存在 `S.ui.stringsFilter`。zh 与 en 两列入空时标红，顶部徽标统计缺失条目数。编辑在 `change` 时直接走 `setString`。

## 分页

当 `pageCount() > 1` 时显示分页条。页数不超过 7 时全显；超过时取首页、末页与当前页前后各一页。与资源列表共用 `S.ui.page`。

> [!tip] 无选中记录时标题显示空态文案，属性页与预览页只留占位说明。选中变化时 `home.vue` 会清空草稿；新增“应用后清草稿”的流程要先等待物化落定，详见 [[字段草稿 fieldDrafts]]。

## 相关笔记

- [[面板总览]]
- [[资源列表 AssetsListPane]]
- [[字段草稿 fieldDrafts]]
