---
title: 共享样式 kit.css
aliases:
  - kit.css
  - 共享样式
  - 全局样式基线
tags:
  - graphite/style
up: "[[样式与组件总览]]"
updated: 2026-11-20
---

> 上级：[[样式与组件总览]] ｜ 首页：[[知识库首页]]

`src/shared/kit.css` 由应用入口全局引入，承载跨面板复用的按钮基线、字段行、空态与侧栏骨架，是 [[Argon 控件库]] 之外的布局补充层。

## 全局类清单

- `button` 基线：清除浏览器默认外观，透明底、继承字体与颜色，hover 提亮 1.25、active 压暗 0.75，过渡 100ms。
- `.grid-empty`：13px 灰色居中空态，四周 24px 留白，宽度撑满。
- `.res-selected`：资源列表与星球 sector 共用的选中态，`#2F5152` 底加 `#1DAF98` 边。
- `.tong`：检查器与科技树编辑器的大标题，白字 24px。
- `.insp-group`：分组小标题，强调色 12px，上方留 8px。
- 字段行：`.fld-row` 两端对齐、最小高 28；`.fld-label` 竖排；`.fld-zh` 13px 正文；`.fld-n` 10px 字段名。
- Argon 行：`.fld-ar-row` 横排、最小高 30；`.fld-ar-label` 固定 64px 标签列；`.fld-static` 只读值省略号；`.ar-field` 与 `.ar-select` 在行内自适应宽度。
- 窄字段：`.fld-row` 内 `.ar-field` 与 `.ar-select` 固定 128，输入壳压到 24 高、字号 11。
- 侧栏骨架：`.cosmos-rail-head` 64px 头部、`.cosmos-search` 搜索区、`.cosmos-actions` 纵向动作区，供科技树与星球战役共用。

## 使用边界

kit.css 没有组件级 scope，选择器是应用私有命名，全局只加载一次。面板内不要再定义同名类，否则会与全局版本冲突；局部差异用更具体的选择器或在 scoped 样式里补布局。

> [!warning] `button` 选择器是全局的。新增按钮组件或裸 `<button>` 必须显式给尺寸与底色，否则会继承透明外观与全局提亮规则。

> [!tip] 想扩展字段行时优先增加 kit.css 既有类，不要在每个面板复制一套私有样式；只有单面板独有的结构才留在 scoped CSS。

## 与令牌和控件的关系

颜色与圆角取 [[设计令牌]]，交互控件取 [[Argon 控件库]]。kit.css 只负责间距、对齐、空态与选中态这类跨面板一致的视觉契约，不覆盖控件内部结构。

## 相关笔记

- [[Argon 控件库]]
- [[设计令牌]]
- [[样式与组件总览]]
- [[面板总览]]
