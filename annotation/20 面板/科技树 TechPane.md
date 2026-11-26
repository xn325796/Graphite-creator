---
title: 科技树 TechPane
aliases:
  - TechPane
  - 科技树
  - 研究树
tags:
  - graphite/panel
up: "[[面板总览]]"
updated: 2026-11-20
---

> 上级：[[面板总览]] ｜ 首页：[[知识库首页]]

把记录上的 research 关系渲染成可平移缩放的森林，支持拖拽端口改写父级。运行期状态由 [[科技树状态]] 维护。

## 数据来源

画布数据来自 `techForest()`，即 store 中 `S.tech` 的快照，由壳命令 `tech_forest` 生成。左侧栏分为根列表与未入树记录，`filteredRoots` 在根标签或其子项标签命中搜索词时保留整棵树。

`research` 是唯一数据源：树形、父级、根名、星球、前置与目标全部从该字段派生，不从其他字段或本地缓存二次推导。

> [!warning] `readResearch(r)` 返回副本。拖拽改父级时要在副本上就地修改再 `setResearch`，不得直接改 `S.tech` 缓存，否则下一次刷新会被覆盖。

## 布局算法

叶子节点宽 `TECH_NODE = 60`，同级间距 `TECH_GAP = 20`。`measureBox` 自底向上累计子树宽度，`placeBox` 把父节点居中到首末子节点之间。根节点按子项数量一半分成 `dir = 1` 与 `dir = -1`，分别向下、向上展开，保证树不重叠。

边用 SVG 正交折线：先垂直、再水平、再垂直。节点坐标只由算法计算，缩放与平移不改世界坐标。

## 平移与缩放

左键或中键在画布按下可拖动平移，滚轮以光标为锚缩放，范围 0.15–2.5。工具栏提供加减与 `fitTech` 自适应，自适应用世界包围盒加 120px 留白计算。

## 端口拖拽

节点右侧 `tech-port` 拖出橡皮筋线。松开时若命中目标节点的 60×60 区域，且目标与来源不是同一记录，就把来源的父级改为目标 `key` 并提交。命中失败或目标无 `key` 时直接放弃。

## 右侧编辑器

选中节点后 `pickNode` 同时 `select` 记录并刷新编辑器。编辑器有 parent/root 两种 `mode`：parent 模式选择父级；root 模式填写根名与 `requiresUnlock`。前置条件与目标行按数组增删，目标类型来自 `OBJECTIVE_TYPES`，目标键由 `objectiveTargetKey` 决定。

## 与 Rust 对齐

前端只做展示与交互，树结构、问题标记与候选父级由 Rust 侧 `TechTreeIndex` 与 `TechTreeRules` 的移植实现给出。`writeResearch` 会把“只有父级的裸字符串”写回为短形式，其余情况写成对象。

## 相关笔记

- [[科技树状态]]
- [[面板总览]]
- [[字段草稿 fieldDrafts]]
