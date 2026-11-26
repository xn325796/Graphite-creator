---
title: 方块画布 CanvasPane
aliases:
  - CanvasPane
  - 方块画布
tags:
  - graphite/panel
up: "[[面板总览]]"
updated: 2026-11-26
---

> 上级：[[面板总览]] ｜ 首页：[[知识库首页]]

方块构建的节点画布，负责拖动卡片、连线端口、增删中转点与批量删除。画布数据由 [[方块构建状态]] 提供，几何计算全部来自 [[画布几何 canvasGeometry]]。

> **2026-11-26 就地更正**：工具栏新增了页签、缩放百分比、Fit 与删除按钮；背景点阵由 `S.ui.grid` 控制
> （`.cv-canvas.plain` 去掉 `background-image`）。**吸附（`S.ui.snap`）仍未接线**：只在工具栏与菜单里改状态并写日志，
> 画布拖动不做任何吸附。依据：`CanvasPane.vue` 模板与 `canvas.css` 的 `.cv-canvas.plain`。

## 视口

组件内用局部 `cv` 保存 `panX`、`panY`、`zoom`。首次进入某个工程与板卡时，从 `S.board.viewport` 初始化；没有视口则回到 40/40/1。切换板卡或刷新后按 `工程名:board` 键判断是否需要重置。

背景拖动会清空节点与边选中并平移画布；滚轮以光标为锚缩放，范围 0.2–2.5。

板卡页签切换时调用 `refreshBoard`，Fit 按节点包围盒自适应，缩放按钮与百分比只影响局部视口，不写回 store。

## 卡片与多选

`cvNodes` 把 `board.nodes` 与 `board.cards` 按下标配对并过滤无卡片的节点，尺寸用 `cardSize` 计算。点击卡片时：

- 无修饰键：若未选中则替换为单选。
- Shift、Ctrl 或 Meta：切换该节点在选区中的存在。
- 拖动位移超过 0.5 才提交，经 `canvasMove` 传入每项的 before 与 after。

卡片头部背景取 `card.color`，内容行按 `CONTENT.HEADER + index * CONTENT.ROW` 定位。

## 端口与连线

内容卡右侧有一个输出口，输入口按 `inputFields` 在对应行左侧生成。从输出口拖出时画贝塞尔橡皮筋，松开时以 `max(10, 10 / zoom)` 的容差吸附最近的输入口，命中且不是同一节点就 `canvasConnect`。

> [!warning] 端口位置必须走 `portAnchor` 与 `portWorld`，尺寸常量只在 `lib/canvasGeometry.js` 定义：内容卡宽 192、头行各 32、端口半径 4，补给卡 260×64。在 `canvas.css` 里只准写外观，不要再算一份几何。

## 边

边由 `portWorld` 求起终点，再交给 `edgeGeometry` 按 `board.edgeStyle` 生成路径，默认 bezier，也支持 orthogonal 与 straight。单击边选中，双击插入中转点，拖动圆点调用 `canvasMoveReroute`，双击圆点调用 `canvasRemoveReroute`。箭头角度与标签位置同样由几何模块返回。

## 删除快捷键

Delete 与 Backspace 监听挂在 window，只在画布挂载期间生效。若事件源是 input、textarea 或 select 则跳过，避免误删正在编辑的字段。有边选中先断边，否则删除全部选中节点；工具栏的删除按钮与快捷键走同一个 `cvDeleteSelection()`，底层是 `canvasDeleteNodes` → `record_delete`。

> [!tip] 卸载时必须移除键盘监听，否则切到其他工作区后快捷键仍会触发。

## 相关笔记

- [[方块构建状态]]
- [[画布几何 canvasGeometry]]
- [[面板总览]]
