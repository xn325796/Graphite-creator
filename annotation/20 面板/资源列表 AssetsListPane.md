---
title: 资源列表 AssetsListPane
aliases:
  - AssetsListPane
  - 资源列表
  - 资产列表
tags:
  - graphite/panel
up: "[[面板总览]]"
updated: 2026-11-26
---

> 上级：[[面板总览]] ｜ 首页：[[知识库首页]]

左侧资源浏览面板，负责按类型筛选记录、切换网格/列表视图，并把选中项交给 [[检查器 InspectorPane]] 编辑。左右两栏之间有可拖动的分隔条（`usePanelWidth`，双击复位）。

> **2026-11-26 就地更正**：原文的「卡片固定 100×100、7 列 × 5 行、`padding-right: 19px` 补偿」已不成立。
> z1 之后网格是 `repeat(auto-fill, minmax(104px, 1fr))` / 行高 104，列数随宽度变化；`padding-right` 补偿
> 随 `mian.svg` 底图一起取消（画布改为纯色点阵）。每页仍 35 条（`PER_PAGE`），但不再对应固定的 7×5 视觉网格。
> 依据：`src/components/panels/assets.css`、`src/store/project.js::PER_PAGE`。

## 类型导航

`TYPES` 固定七类：item、liquid、block、unit、status、weather、team。每行显示类型色点、本地化名称与数量徽标，选中行高亮。点击类型只改 `S.ui.type` 并把页码归零；上下方向键/Home/End 在类型间移动。

## 筛选与视图

- 筛选框写 `S.ui.filter`，匹配 `name` 与本地化显示名，排序按 `name`。
- 网格/列表切换写 `S.ui.view`。
- `setType`、`setFilter`、`setView` 三个动作都会把 `S.ui.page` 重置为 0，避免停留在越界页。

## 网格视图

`pageRecords()` 每页 35 条。卡片最小 104×104，`auto-fill` 自动排列表；优先显示 `spriteUrl`（只预取当前页），没有精灵图时用 `recordColor`（`fields.color` 去 `#` 回拼，非法值回落 `TYPE_COLOR`）。左上角是原版徽标，右上角是问题红点；卡片与列表行都支持右键菜单（重命名/删除）。

## 列表视图

固定名称、ID、状态三列。状态列依次堆叠原版、未保存、问题徽标，问题徽标按是否存在 error 在 `err` 与 `warn` 之间切换。

## 新增与删除

底部两个按钮：新增要求工程已载入，`@click="addRecord()"`；删除要求存在选中记录且其类型等于当前类型，`@click="deleteSelected()"`。两类操作的真实写盘由 store 发起的壳命令完成。

> 2026-11-26 教训：`@click` 必须写空括号 `addRecord()`。写成 `@click="addRecord"` 会把 `MouseEvent` 当作第一个实参（`type`）传进命令，表现为「新增后类型错乱/命令报错」。`deleteSelected(type)` 同理吃第一个参数。

## 空态与选中

无记录时显示 `grid-empty` 空态文案。网格卡与列表行共用 `.selected` 选中态（`.asset-card.selected` / `.asset-row.selected`，取令牌 `--accent-soft` / `--accent-line`），进入编辑的通道是 [[字段草稿 fieldDrafts]]。

## 相关笔记

- [[面板总览]]
- [[检查器 InspectorPane]]
- [[字段草稿 fieldDrafts]]
