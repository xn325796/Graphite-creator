---
title: Argon 控件库
aliases:
  - Argon
  - Ar 控件
  - UI 控件库
tags:
  - graphite/style
up: "[[样式与组件总览]]"
updated: 2026-11-20
---

> 上级：[[样式与组件总览]] ｜ 首页：[[知识库首页]]

`src/components/ui` 下的受控展示层，统一 `Ar` 前缀。只接收 props 与 model 并抛事件，业务状态留在 store；颜色全部取 [[设计令牌]]。

## 控件清单

- ArButton：`variant` 取 primary、soft、ghost、outline、danger，默认 soft；`size` 取 xs、sm、md，高 22、26、30；另有 `icon` 变正方形、`block` 撑满、`disabled` 降透明度。未声明 emits，click 透传到根按钮。
- ArInput：`defineModel` 字符串；`label` 为浮动标签，聚焦或有值时上浮；`size` 取 sm、md；`mono`、`invalid`、`readonly`、`disabled`、`type`。抛 input、blur、enter，并暴露 `focus()`。
- ArSelect：`options` 为 `{ value, label }` 数组，字符串 model；`size` 取 sm、md；支持 `invalid`、`disabled`，原生 select 加自绘箭头。
- ArSwitch：布尔 model；`size` 取 sm、md；`label`、`disabled`。开启态轨道用 `--ar-accent-bg`，滑块用 `--ar-accent`。
- ArTabs：`tabs` 为 `{ key, label, disabled }` 数组，model 是 key 字符串；`size` 取 sm、md；`block` 均分。点击直接改写 model。
- ArBadge：`variant` 取 accent、warn、err、neutral，固定 16px 高、9px 字，只做颜色区分。
- ArProgress：`value` 必须是 0..1，内部裁剪并四舍五入成百分比；`label` 显示标题，`small` 收细轨道。
- ArTooltip：`text` 必填，`placement` 取 top、bottom；hover 或 focus-within 时淡入，纯 CSS 无延迟配置。
- ArDropdown：`items` 为 `{ label, shortcut, disabled, checked, sep }`；`align` 取 left、right，`minWidth` 默认 210；`trigger` 插槽带 `{ open, toggle }`，选中抛 `choose`，点外部或 Esc 关闭。
- ArModal：`v-model:open`，`title`、`width` 默认 420，`footer` 具名插槽；Esc、遮罩、关闭按钮均 emit `close`。
- ArToasts：全局单例，直接读 `lib/toast.js`，按 info、warn、err 给圆点染色，点击即关闭，需手动挂载一次。

## 尺寸覆写

面板里的紧凑字段行不修改组件默认值，只在调用方用 `:deep` 覆盖：`:deep(.ar-field)` 控宽、`:deep(.ar-field-sm .ar-field-shell)` 压高度、`:deep(.ar-select)` 控宽高、`:deep(.ar-switch-label)` 调字号。深层结构如 `:deep(.ar-field .ar-field-shell)` 也可用，因为 scoped 只作用于根节点。

> [!warning] 交互组件都是 `defineModel`。父级应传 `:model-value` 加 `@update:model-value`，不要把 store 状态直接 `v-model` 绑进来，否则绕过 store 的 setter 与校验。

## 使用约定

`ArProgress` 只接受 0..1，传 0..100 会一直满格。`ArTabs` 的 key 一律按字符串比较，数字页签必须 `String()` 转换。`ArModal` 与 `ArToasts` 使用 `position: absolute`，挂载容器必须有定位上下文。

> [!tip] 控件外观差异只能通过 `size` 与 `variant` 表达；局部微调留在调用方，避免动默认值影响全局。

## 相关笔记

- [[设计令牌]]
- [[共享样式 kit.css]]
- [[样式与组件总览]]
