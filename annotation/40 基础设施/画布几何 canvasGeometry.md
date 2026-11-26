---
title: 画布几何 canvasGeometry
aliases: [canvasGeometry, 画布几何, 边路由数学]
tags: [graphite/infra]
up: "[[基础设施总览]]"
updated: 2026-11-20
---
> 上级：[[基础设施总览]] ｜ 首页：[[知识库首页]]

# 画布几何 canvasGeometry

canvasGeometry.js 集中方块构建画布的尺寸、端口与边路由数学，是前端实测绘制与命中测试的唯一来源。它与 Rust 的 mm-layout::edge_path_math 逐函数对等。

## 尺寸常量

- `CONTENT`：宽 192、头 32、行高 32、端口半径 4。
- `CARRIER`：固定 260×64。
- `cardSize(card)`：补给卡用固定尺寸，内容卡高度为 32 加行数乘 32。

## 端口锚点

- 内容卡输出口在右边中部，坐标为宽减半径、头高一半。
- 字段输入口在左边对应行中线，且只有 inputFields 包含的字段才有口。
- 补给卡只有一个右侧输出口。
- 找不到口返回 null；`portWorld` 回落为卡缺失取原点、口缺失时 y 取 75、x 按输出/输入取 192 或 0。

## 边路由

- `bezier`：每段是两端水平切线的 S 曲线，交汇于中点 x。
- `straight`：直连并按中转点折线。
- `orthogonal`：在中点 x 处垂直转折。
- `viaPoints` 过滤畸形中转点，重复点自动去重。

`edgeGeometry` 返回 `{d, arrow, label, tip}`：箭头角由最后一段方向得出，箭尖落在终点前 7px，标签在两端中点左上偏移。

> [!info] 黄金参考
> 本模块与 Java EdgePathMath、Rust edge_path_math 是逐函数对等关系，改任一侧都必须同步另一侧并跑两侧单测。

> [!warning] bezier 箭头的特殊之处
> bezier 的箭头角只依赖最后一个中转点的 x，不含 y；改动曲线形状时留意箭头是否仍水平进入。

> [!tip] 命中测试
> `edgeGeometry` 接收原始 reroutes 并自行过滤；命中结果的下标基于过滤后列表，调用方不要混用原始下标。

## 相关笔记

- [[方块画布 CanvasPane]] —— 唯一渲染消费方
- [[方块构建状态]] —— 快照提供的节点与卡片
- [[基础设施总览]] —— 底座的整体边界
