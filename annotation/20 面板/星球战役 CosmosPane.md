---
title: 星球战役 CosmosPane
aliases:
  - CosmosPane
  - 星球战役
  - 星球面板
tags:
  - graphite/panel
up: "[[面板总览]]"
updated: 2026-11-20
---

> 上级：[[面板总览]] ｜ 首页：[[知识库首页]]

以星球为主轴整理 sector 归属，并提供格子、外观与星系三种预览。选中与草稿细节见 [[星球战役状态]]。

## 左树

`cosmosTree()` 输出 planet → sector 两级分组，以节点与从属关系表达归属。真正存在 planet 记录的分组可点击选中星球；没有对应记录的原版星球以 `vanilla` 分组呈现。搜索词同时过滤分组名与条目名，命中分组名时保留整组。

sector 行可拖到任意分组上。悬停时分组加 `cosmos-drop` 强调边，松开后调用 `reassignSector` 改 `planet` 字段。

> [!warning] 归属写回后必须 `await refreshCosmosFields()`。星球与 sector 的字段由 `records_detail` 重新物化，不刷新会继续显示旧 planet 值。比较星球名时要兼容 `${module}-${key}` 前缀，否则拖拽会被误判为“已在该星球”。

## 中间字段列

只读行由 `cosmosRows` 拼装：首行是显示名；sector 追加 planet 行，planet 追加 `sectorSize` 行；随后遍历 `fieldSpecs()`，跳过 name、planet、sectorSize，跳过名称含 mesh 或 atmosphere 的字段，空值不显示，对象值折叠为省略号。

## 右侧预览

页签状态在 `S.ui.cosmosTab`，取值为 0、1、2，`ArTabs` 的 key 统一转字符串比较。

- grid：按 `planetGrid` 的 `size × size` 渲染瓦片，容量为 `size` 平方。occupied 瓦片可点击选中对应 sector，选中 sector 的格子加 `seltile` 高亮；无 `sectorSize` 时提示无网格。
- appearance：用 `planetColors` 的 c2 到 c1 做径向渐变球体，颜色来自 planet 的 mesh 字段。
- system：纯 SVG 示意，太阳、两条轨道与一颗行星，只做视觉摘要。

## 新增与选中

底部动作区调用 `addCosmos('planet')` 或 `addCosmos('sector')`。在选中 planet 时新增 sector，store 会把新 sector 自动挂到该 planet 下。`cosmosSelection` 在没有 planet/sector 选中时回退到第一个有 `sectorSize` 的 planet。

所有写回走 store 的壳命令，面板不直接改记录或落盘，整体语义与原型保持同构。

## 空态

左树无分组、中间无选中、预览无选中都显示 `grid-empty` 文案，不渲染空画布。

## 相关笔记

- [[星球战役状态]]
- [[面板总览]]
- [[检查器 InspectorPane]]
