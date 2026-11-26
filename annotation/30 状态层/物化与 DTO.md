---
title: 物化与 DTO
aliases: [物化, Materialize, DTO 映射]
tags: [graphite/state]
up: "[[状态层总览]]"
updated: 2026-11-26
---
> 上级：[[状态层总览]] ｜ 首页：[[知识库首页]]

# 物化与 DTO

物化指把 Rust 壳返回的 DTO 转成前端可直接渲染的普通对象。所有映射集中在 project.js 的 materialize 系列函数与 typedFields，组件不接触原始 DTO 的字段名。

> **2026-11-26 就地更正**：原文「`spriteUrl()` 当前恒返回 null，精灵尚未接入」已过时——缩略图走
> `record_sprite` + 前端缓存 + `S.ui.showSprites` 开关；记录 DTO 还多了有效类与来源两格。依据：`src/store/project.js`、`src-tauri/src/dto.rs`。

## 壳命令与落点

- `project_summary` → `S.project` 项目壳（含 meta、locales、contentCount）。
- `project_contents` → `S.project.records` 记录列表。
- `project_strings` → `S.project.strings`，按 locale 对折成 {zh, en}。
- `history_state` → `undoState` 与 `dirty`。
- `record_select` → `selRecord` / `selSchema` / `selClassName` / `selClassSource`。

## record 的形状

每条记录固定为 id、type、name、displayName、patch、inline、dirty、fields。`contentType` 映射为 `type`；颜色来自 DTO 的 color，去掉前导 #，纯白归一为 undefined；`fields` 初始只放 color，其余字段在选中后由 `typedFields` 补齐。

`typedFields` 按 FieldSpecDto 的 control 还原类型：bool 取 checked，number 取 text 转数字（空串为 undefined），其余按字符串或 JSON 文本保存。

**有效类两格（2026-11-26 R-02）**：`RecordDto.className` 现在是「声明 > 脚本推断 > 类型默认」的有效类，`classSource` 是 `"declared"` / `"script"` / `"default"` 之一。它是**视图口径**，不是磁盘上的 `type`：保存路径完全不知道推断结果，`record_set_field{field:"type"}` 仍是唯一写入口。壳未打开项目时脚本这一级不存在，回落与 Java 两级解析同形。

> [!info] 颜色与精灵
> 列表色块使用 schema.js 的 `TYPE_COLOR`（按内容类型），记录自身颜色来自 DTO。`spriteUrl(r)` 读前端 `sprites` 缓存：命中直接返回 dataURL，未命中则入队（`requestSprite`，只取当前页）并先返回 null；`S.ui.showSprites` 为假时恒为 null，列表回退色块。

> [!warning] 不要制造空窗
> `materializeSummary` 先写项目壳，并刻意沿用上一次的 records/strings，等 contents 与 strings 到达再覆盖；否则刷新瞬间列表会闪空。

## vanilla 与 id

vanilla 标记不随 `project_contents` 下发，由 `isVanilla(type, name)` 查 `vanillaNames` 判断；科技树节点带 `vanilla` 字段，星球分组对缺少对应记录的星球也标 vanilla。记录 id 全部由 Rust 生成，前端不随机造 id；重命名只在本地做格式与同类型重名校验，随后交给壳命令。

## 相关笔记

- [[状态层总览]] —— S 的整体形状
- [[事件刷新链路]] —— 物化由哪些事件触发
- [[科技树状态]] —— research 字段的另一套物化
- [[星球战役状态]] —— planet/sector 字段的批量补齐
