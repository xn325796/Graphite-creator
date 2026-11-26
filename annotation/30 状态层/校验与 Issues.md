---
title: 校验与 Issues
aliases: [校验, Issues, Validation]
tags: [graphite/state]
up: "[[状态层总览]]"
updated: 2026-11-26
---
> 上级：[[状态层总览]] ｜ 首页：[[知识库首页]]

# 校验与 Issues

校验的唯一事实源在 Rust。前端不重算规则，只负责取回结果、归一等级、计数并展示。

> **2026-11-26 就地更正**：原文「五段规则：Meta → MinGameVersion → Record → Reference → Strings」是 Java 五规则的旧口径。
> 现状是 **17 条规则**，其中 `scriptClass` 是 Java 之外新增的一条（R-02）。原文顺序保留为历史，以下为实测顺序。
> 依据：`crates/mm-validate/src/validator.rs::default_checks`。

## 规则顺序（17 条）

`meta` → `minGameVersion` → `encoding` → `records` → **`scriptClass`** → `references` → `contentOrder` → `strings` → `teams` → `boards` → `bulletReach` → `drillTiers` → `environmentSprites` → `crafterOutputs` → `recipeCapacity` → `sounds` → `planets`

`UnknownCheck` 的已知 id 列表与这个顺序逐字一致（`validator.rs` 的回归测试直接断言 `known.len() == 17` 与整串文本）。

### `scriptClass`（2026-11-26 新增）

方块既没有声明 `type`，脚本里也没有 `extend(<Class>, '<名>')` 命中时，报 **warning**：游戏会按 `Block` 加载，文件能过，但作者写的子类字段会被静默忽略。它只补「有默认类、真实类却在脚本里」这一档，与 `records` 不重复报：显式 `type` 交给 `records`，脚本命中则 `mm-app` 已按推断类显示字段集。静态扫描给不出变量名（`extend(GenericCrafter, name, …)`）时同样只报 warning——「工具推断不出」不等于「游戏加载错」。

## 与 Java 的两处授权偏离

- `references`：内联对象里数组的裸字符串不再当内容引用（模块头第 11 条）。真实样本 `destroyBullet.hitEffect.effects: ["massiveExplosion"]` 是原版 Fx 名，旧口径误报 error 并卡死 CLI 校验；`requirements: [{item, amount}]`、`consumes: {liquids: [...]}` 的具名键检查不放松。
- `minGameVersion`：导入/读取缺字段时的回退值由 Java 的 `"146"` 改为 `"160"`（2026-11-25 用户授权，`mm-fmt/src/codec/meta.rs`）。

## 结果归一

`refreshIssues` 调用 `board_snapshot`（boardId 为 null），把返回的 issues 交给 `mapIssues`：

- `level` 只保留 error 与 warn，非 error 一律记为 warn。
- 保留 key、args、contentId 与 message。
- `message` 直接使用 Rust 渲染好的文本，前端不再按 key 拼装。

`validate.js` 中的 `MSG` 与 `validateProject` 是旧 mock 路径的回退，Tauri 链路不调用；`issueText` 也优先消费 Rust 的 message。

> [!info] 计数来自 S.issues
> 状态栏与检查器用 `errorCount()`、`warnCount()` 直接过滤 `S.issues`；单条记录的 Issues 用 `issuesFor(r)` 按 contentId 匹配。

> [!warning] 不要复活本地校验
> 重新引入前端校验会与 Rust 分叉，也会把领域词带进 src，触发 layering 检查。

## 主动校验

`validateNow()` 在无项目时直接返回；有项目时设置 `ui.validating`，调用 `refreshIssues`，把总数与前 10 条写入日志，最后切到检查器的校验页签。它不会在本地重新计算。贴图导入成功后也会主动 `refreshIssues()`，让状态栏徽标立即一致。

## 相关笔记

- [[项目生命周期]] —— 打开、导入后主动刷新 Issues
- [[事件刷新链路]] —— validation-refreshed 事件
- [[状态层总览]] —— S.issues 的位置
