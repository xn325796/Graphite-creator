---
title: 国际化 messages
aliases: [messages, 国际化, 文案表]
tags: [graphite/infra]
up: "[[基础设施总览]]"
updated: 2026-11-20
---
> 上级：[[基础设施总览]] ｜ 首页：[[知识库首页]]

# 国际化 messages

messages.js 维护一张 key 到中英两列的文案表 M，以及取词函数 t。store 再包一层 tr，把当前语言固定为 S.ui.lang。

## 文案表

- 每个条目是 `[中文, English]` 两列。
- 分组前缀覆盖菜单 menu.*、命令 cmd.*、状态 status.*、对话框 dlg.*、日志 log.*、校验 issues.*、提示 toast.*。
- 键缺失时回退返回 key 本身，便于发现漏配。

`t(key, lang, args)` 按 lang 取列（非 en 一律取中文），并用 replaceAll 把 `{name}` 之类占位符替换为参数值。

`tr(key, args)` 直接读取 `S.ui.lang`，组件与 store 内部统一用它取词。

## 语言切换

切换语言只改 `S.ui.lang`，随后整壳依赖响应式重渲染；菜单、命令按钮、状态栏、检查器标签与日志模板都会随之更新。日志中已写入的历史文本不会追溯翻译。

> [!info] 文案来源集中
> 菜单栏 MenuBar 的标签来自 `tr('menu.' + 名称)` 与命令键，状态栏用 status.*，对话框用 dlg.*；组件不硬编码中英文。

> [!warning] 占位符必须成对
> 参数名要与模板里的 `{k}` 完全一致，否则整段文本原样保留花括号。

> [!tip] 新增文案
> 同时补齐中英两列；只有中文会让英文界面露出键名，回退逻辑不会报错。

## 相关笔记

- [[菜单栏 MenuBar]] —— 菜单与命令文案的消费方
- [[基础设施总览]] —— 底座的整体边界
- [[Toast 与日志]] —— toast 与日志模板的取词
