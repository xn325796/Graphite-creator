//! 编辑命令族：Java `ProjectController` 的内部命令类（`FieldCommand` 一族）与
//! `DirtyFlags`／`EditState`／`AppCommand` 的等价移植。2026-11-17 从
//! `project_controller.rs` 拆出（原文件 4500+ 行）：命令只操作 [`EditState`]，
//! 与控制器本体无双向耦合，拆出后各自可独立阅读。行为与拆出前逐字一致。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mm_fmt::refs::{Revert, RewriteResult};
use mm_json::{Json, Object};
use mm_model::{AssetRef, Board, CanvasEdge, CanvasNode, ContentRecord, ModProject, StringTable};
use mm_schema::{FieldDef, KeyedMap, SchemaRegistry};

use super::ProjectController;
use crate::undo_manager::{Command, CommandError};

/// 命令的执行现场：Java 匿名命令类捕获的那三个东西。
///
/// `board` 用下标而不是引用——Java 的 `Board board` 是 `project.boards` 里某个元素的引用，
/// Rust 里 `ModProject` 独占 `Vec<Board>`，命令不能同时持有对它的两个可变借用。
/// Java `ProjectController.java:719-721` 特意说明命令要作用在**创建时**那块画板上（切画板不会把
/// 一次多节点的拖动拆开），这个「记住创建时那块画板」的语义由每个命令各自存 `board` 下标实现。
pub struct EditState<'a> {
    /// 对应 Java `ProjectController.project`（`:68`）。
    pub project: &'a mut ModProject,
    /// 对应 Java `ProjectController.board`（`:69`）：当前画板在 `project.boards` 中的下标。
    pub board: &'a mut Option<usize>,
    /// 对应 Java `ProjectController.registry`（`:62`）。
    pub registry: &'a SchemaRegistry,
}

impl<'a> EditState<'a> {
    /// 对应 Java 里命令直接用的 `board` 字段读取（如 `:676`、`:869`）。
    /// 画板被摘掉时返回 `None`，调用点与 Java 的 `IndexOutOfBounds`/`NPE` 分支一一对应。
    pub fn board_mut(&mut self) -> Option<&mut Board> {
        let index = (*self.board)?;
        self.project.boards.get_mut(index)
    }

    /// 同上，只读版（`:1229` 的 `board.nodes.removeIf` 之前先读 `board.edges`）。
    pub fn board(&self) -> Option<&Board> {
        let index = (*self.board)?;
        self.project.boards.get(index)
    }
}

/// 对应 Java `ProjectController.DirtyFlags`（`:111-180`，内部类）。
///
/// 「一条命令翻动了哪些『下次保存要写』的标记」：它碰过的记录脏标记、每个语言的 bundle 脏标记
/// （连同当时的原文）、加上画板与 meta 两个总标记。命令执行**前**记一次、执行**后**再记一次
/// （`:102-106`），于是 undo 能把项目恢复得像没改过一样干净（`:104-106`：撤销后仍留脏，会让下次
/// 保存重写一个没动过的文件、悄悄丢掉注释），redo 再把同一批标记装回去。
///
/// `:108-109` 的不变式原样保住：**标记绝不越过它守护的字节被清掉**——命令之后跑过一次保存，
/// 盘上的文件已经是这条命令的结果，那条记录／那个 bundle 仍得再生成一次。
///
/// Java 用 `List<ContentRecord>` + 对象同一性盯着记录；Rust 侧改成盯 id（见模块头第 2 点）。
/// 有一处必须如实说明的偏差：`addContent`（`:670-751`）在建好 `flags` 之后才把记录
/// `project.contents.add(record)`，所以那个 id 在「标记被捕获的那一刻」还不存在于项目里。
/// Java 靠引用不受影响，Rust 按 id 找回时可能查不到——查不到就是「记录已被摘出项目」，
/// 与 Java `project.contents.remove(record)` 之后的状态同义，跳过即可，不报错。
pub struct DirtyFlags {
    /// 对应 `records`（`:112`）：盯着哪些记录（Java 去重靠 `records.contains`，这里靠 id 去重）。
    pub(crate) records: Vec<String>,
    /// 对应 `rawSeen`（`:113`）：捕获时各记录的原文。
    pub(crate) raw_seen: Vec<Option<String>>,
    /// 对应 `bundleRawSeen`（`:114`）：捕获时各语言的 bundle 原文。
    /// `Arc<str>` 与 `StringTable.bundle_raw` 共享：未改动的语言只克隆引用，不复制文本。
    pub(crate) bundle_raw_seen: KeyedMap<Arc<str>>,
    /// 对应 `recordDirtyBefore` / `recordDirtyAfter`（`:115-116`）。
    pub(crate) record_dirty_before: Vec<bool>,
    pub(crate) record_dirty_after: Vec<bool>,
    /// 对应 `bundlesBefore` / `bundlesAfter`（`:117-118`）。
    pub(crate) bundles_before: KeyedMap<bool>,
    pub(crate) bundles_after: KeyedMap<bool>,
    /// 对应 `boardsDirtyBefore` / `boardsDirtyAfter`（`:119-120`）。
    pub(crate) boards_dirty_before: bool,
    pub(crate) boards_dirty_after: bool,
    /// 对应 `metaDirtyBefore` / `metaDirtyAfter`（`:121-122`）。
    pub(crate) meta_dirty_before: bool,
    pub(crate) meta_dirty_after: bool,
}

impl DirtyFlags {
    /// 对应 Java `DirtyFlags(ContentRecord... watched)`（`:124-136`）。
    ///
    /// Java 的可变参数在这里收成 `&[Option<&ContentRecord>]`：空参（如 `setString` 的调用点
    /// `:1765` `new DirtyFlags()`）就是空切片，单记录（如 `setPatch` 的 `:1786`
    /// `new DirtyFlags(record)`）就是一个元素；Java 允许的 `null` 记录由 `Option` 表达，
    /// 跳过它那一句对应 `:126` 的 `record != null`。
    pub fn new(project: &ModProject, watched: &[Option<&ContentRecord>]) -> Self {
        let mut records: Vec<String> = Vec::new();
        let mut raw_seen: Vec<Option<String>> = Vec::new();
        for record in watched.iter().flatten() {
            // :126 `if (record != null && !records.contains(record))`
            let id = match record.id.as_deref() {
                Some(id) => id,
                None => continue,
            };
            if records.iter().any(|seen| seen == id) {
                continue;
            }
            records.push(id.to_string());
            raw_seen.push(record.raw.clone());
        }
        // :131-135 逐个快照；`recordDirtyBefore = recordFlags()`（`:132`）用的正是上面刚收好的这批记录。
        let bundle_raw_seen = project.strings.bundle_raw.clone();
        let record_dirty_before = Self::record_flags(&records, project);
        let bundles_before = project.strings.bundle_dirty.clone();
        Self {
            records,
            raw_seen,
            bundle_raw_seen,
            record_dirty_before,
            record_dirty_after: Vec::new(),
            bundles_before,
            bundles_after: KeyedMap::new(),
            boards_dirty_before: project.boards_dirty,
            boards_dirty_after: false,
            meta_dirty_before: project.meta_dirty,
            meta_dirty_after: false,
        }
    }

    /// 对应 `captureAfter()`（`:138-144`）：记下命令留下了什么，命令第一次执行完调用一次
    /// （Java 的注释 `:138` 写明「call once, right after it first ran」）。
    pub fn capture_after(&mut self, project: &ModProject) {
        self.record_dirty_after = Self::record_flags(&self.records, project);
        self.bundles_after = project.strings.bundle_dirty.clone();
        self.boards_dirty_after = project.boards_dirty;
        self.meta_dirty_after = project.meta_dirty;
    }

    /// 对应 `restoreBefore()`（`:146-149`）：把每个被盯的标记放回命令执行前。
    pub fn restore_before(&self, project: &mut ModProject) {
        self.restore(
            project,
            &self.record_dirty_before,
            &self.bundles_before,
            self.boards_dirty_before,
            self.meta_dirty_before,
        );
    }

    /// 对应 `restoreAfter()`（`:151-154`）：把命令装上的标记再装回去，让 redo 的结果下次保存被写盘。
    pub fn restore_after(&self, project: &mut ModProject) {
        self.restore(
            project,
            &self.record_dirty_after,
            &self.bundles_after,
            self.boards_dirty_after,
            self.meta_dirty_after,
        );
    }

    /// 对应 `recordFlags()`（`:156-162`）：Java 直接 `records.get(i).dirty`；Rust 按 id 回查。
    /// 查不到 id 的那条给 `false`——记录已不在项目里，它不属于下次保存的范围（偏差说明见结构体注释）。
    fn record_flags(records: &[String], project: &ModProject) -> Vec<bool> {
        records
            .iter()
            .map(|id| project.content_by_id(id).map(|record| record.dirty).unwrap_or(false))
            .collect()
    }

    /// 对应 `restore(boolean[], Map, boolean, boolean)`（`:164-179`）。
    fn restore(
        &self,
        project: &mut ModProject,
        record_flags: &[bool],
        bundle_flags: &KeyedMap<bool>,
        boards_dirty: bool,
        meta_dirty: bool,
    ) {
        // :166-171 命令之后跑过一次保存：盘上的字节已经是这条命令的结果，所以即使记录之前是干净的，
        // 它也还得再生成一次——这就是 `|| !Objects.equals(record.raw, rawSeen.get(i))` 那半句。
        for (index, id) in self.records.iter().enumerate() {
            // 两个数组都由 `Self::record_flags(&self.records, _)` 按同一批记录生成，长度必然相等，
            // 因此这里与 Java 的 `recordFlags[i]` / `rawSeen.get(i)` 直接同形取值。
            let before = record_flags[index];
            let raw_was = self.raw_seen[index].clone();
            if let Some(record) = project.content_by_id_mut(id) {
                record.dirty = before || record.raw != raw_was;
            }
            // 查不到 id：记录已被摘出项目（Java 里那个引用还活着但不在 contents 里），无需回写。
        }
        // :172-176 语言逐个处理，遍历的是**当时**的 key 集合快照，对应 Java 的
        // `new ArrayList<>(project.strings.bundleDirty.keySet())`；`bundleDirty` 里允许放 `false`
        // （导入时 `ModImporter.java:289`、保存后 `ProjectIo.java:426` 都直接 put false），
        // 所以这里照搬 `put(locale, dirty)`，不做「false 就删键」的简化。
        let locales: Vec<String> = project.strings.bundle_dirty.keys().cloned().collect();
        for locale in locales {
            let before = bundle_flags.get(&locale).copied().unwrap_or(false);
            let raw_now = project.strings.bundle_raw.get(&locale).cloned();
            let raw_was = self.bundle_raw_seen.get(&locale).cloned();
            // Java 的 `getOrDefault(locale, false)` 与 `Objects.equals(null, null) == true` 一并保住：
            // 这个语言当时既没标记、两边又都没有原文，就是「干净」。
            let dirty = before || raw_now != raw_was;
            project.strings.bundle_dirty.put(locale, dirty);
        }
        project.boards_dirty = boards_dirty;
        project.meta_dirty = meta_dirty;
    }

}

/// 对应 Java `ProjectController.FieldCommand`（`:183-223`，静态内部类）：改一个字段值，
/// undo 与 redo 共用的实现。
pub struct FieldCommand {
    /// 对应 `record`（`:184`）。
    pub(crate) record_id: String,
    /// 对应 `field`（`:185`）。
    pub(crate) field: String,
    /// 对应 `oldValue`（`:186`）：`None` 就是 Java 的 `null`，意思是「这个字段该被移除」。
    pub(crate) old_value: Option<Json>,
    /// 对应 `newValue`（`:187`，非 final：连打合并时会被换掉）。
    pub(crate) new_value: Option<Json>,
    /// 对应 `flags`（`:188`）。
    pub(crate) flags: DirtyFlags,
}

impl FieldCommand {
    /// 对应构造器（`:190-197`）。
    pub fn new(
        record: &ContentRecord,
        field: &str,
        old_value: Option<Json>,
        new_value: Option<Json>,
        flags: DirtyFlags,
    ) -> Self {
        Self {
            // Java 传的是 `record` 引用，这里取它唯一的 id。
            record_id: record.id.clone().unwrap_or_default(),
            field: field.to_string(),
            old_value,
            new_value,
            flags,
        }
    }

    /// 对应 `sameTarget(FieldCommand other)`（`:199-201`）：`record == other.record &&
    /// field.equals(other.field)` → 同一 id 且同名字段。
    pub fn same_target(&self, other: &FieldCommand) -> bool {
        self.record_id == other.record_id && self.field == other.field
    }

    /// 对应 `apply(JsonElement value)`（`:203-210`）：`null` 就移除该字段，否则写入，最后翻脏。
    /// Java 的 `fields.put` 在这里是 `Object::insert`（同键保位换值，mm-json/value.rs:35，
    /// 对应 `JsonObject.add` 的「保留原位置」行为）。
    /// 按 id 查不到记录时整个 apply 跳过——那正是 Java 里这条引用已从 `project.contents`
    /// 摘掉、命令却又被 undo 的状态（`disconnect`（`:1206-1280`）与用例 4 就走这条路）。
    fn apply(state: &mut EditState<'_>, record_id: &str, field: &str, value: Option<&Json>) {
        if let Some(record) = state.project.content_by_id_mut(record_id) {
            match value {
                Some(value) => record.fields.insert(field.to_string(), value.clone()),
                None => {
                    record.fields.remove(field);
                }
            }
            record.dirty = true;
        }
    }
}

impl<'a> Command<EditState<'a>> for FieldCommand {
    /// 对应 `undo()`（`:212-216`）：先回滚字节，再把标记放回命令之前。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        Self::apply(state, &self.record_id, &self.field, self.old_value.as_ref());
        self.flags.restore_before(state.project);
        Ok(())
    }

    /// 对应 `redo()`（`:218-222`）：先装标记，再写字节（顺序与 Java 一致：标记要盖住这次写入）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project);
        Self::apply(state, &self.record_id, &self.field, self.new_value.as_ref());
        Ok(())
    }
}

/// 对应 Java `ProjectController.StringCommand`（`:226-261`，内部类）：改一条 bundle 文案，
/// 同一格子的快速连打会合并（合并判定在调用点 `:1770-1772`）。
pub struct StringCommand {
    /// 对应 `key`（`:227`）。
    pub(crate) key: String,
    /// 对应 `locale`（`:228`）。
    pub(crate) locale: String,
    /// 对应 `oldValue`（`:229`，`final`）：取自 `project.strings.get(key, locale)`（`:1761`），
    /// 该格子从没填过时 Java 得到 `null`。
    pub(crate) old_value: Option<String>,
    /// 对应 `newValue`（`:230`，非 final：连打合并时 `:1771` `top.newValue = value` 换掉它）。
    pub(crate) new_value: String,
    /// 对应 `flags`（`:231`）。
    pub(crate) flags: DirtyFlags,
}

impl StringCommand {
    /// 对应构造器（`:233-239`）。Java 的构造器不接记录，`flags` 由调用点用
    /// `new DirtyFlags()`（`:1765`，一个记录都不盯）建好传进来，这里照搬。
    pub fn new(
        key: &str,
        locale: &str,
        old_value: Option<String>,
        new_value: String,
        flags: DirtyFlags,
    ) -> Self {
        Self {
            key: key.to_string(),
            locale: locale.to_string(),
            old_value,
            new_value,
            flags,
        }
    }

    /// 对应 `sameTarget(StringCommand other)`（`:241-243`）：同 key 且同 locale 才算同一格子。
    pub fn same_target(&self, other: &StringCommand) -> bool {
        self.key == other.key && self.locale == other.locale
    }

    /// 对应 `apply(String value)`（`:245-248`）——Java 只有两行：写入这条文案，然后把该语言的
    /// bundle 标记为脏。`markBundleDirty` 只有「置 true」一个动作（`StringTable.java:34-36`），
    /// 清标记是保存路径 `ProjectIo.java:426` 的事，命令这边不存在「先清再标」这一步。
    ///
    /// 唯一的表示差异：Java 的 `values` 允许把值存成 `null`，Rust 的
    /// `KeyedMap<KeyedMap<String>>` 不允许。而写盘时 `renderBundle` 正好把值为 `null` 的条目过滤掉
    /// （`ProjectIo.java:447-451` 的 `.filter(... && entry.getValue().get(locale) != null)`），
    /// 所以「存 null」与「删掉这个 locale 条目」落到字节上是同一件事，这里按后者实现。
    pub(crate) fn apply(strings: &mut StringTable, key: &str, locale: &str, value: Option<&str>) {
        match value {
            Some(value) => strings.put(key, locale, value),
            None => {
                if let Some(by_locale) = strings.values.get_mut(key) {
                    by_locale.remove(locale);
                }
            }
        }
        strings.mark_bundle_dirty(locale);
    }
}

impl<'a> Command<EditState<'a>> for StringCommand {
    /// 对应 `undo()`（`:250-254`）：先把文案写回旧值，再把标记放回命令之前。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        Self::apply(
            &mut state.project.strings,
            &self.key,
            &self.locale,
            self.old_value.as_deref(),
        );
        self.flags.restore_before(state.project);
        Ok(())
    }

    /// 对应 `redo()`（`:256-260`）：标记先装回去，再写新值（顺序与 Java 一致）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project);
        Self::apply(
            &mut state.project.strings,
            &self.key,
            &self.locale,
            Some(self.new_value.as_str()),
        );
        Ok(())
    }
}

/// 对应 `addContent` 里那个匿名命令（`:723-743`）：新增一条记录，连带那张卡片与它的英文名案。
///
/// Java 捕获了 `added` / `placed` / `stringsKey` / `commandBoard` 四个局部量（`:716-721`），
/// 这里逐个落成字段。`record` 与 `node` 是**快照副本**而不是引用——Java 靠对象同一性把同一个
/// `ContentRecord` 在 `project.contents` 里进进出出，Rust 的所有权模型下命令只能自己保管一份：
///
/// - `undo` 按 id 把项目里那条记录摘掉（`:726` 的 `contents.remove(added)`），
/// - `redo` 把自己这份副本塞回去（`:736` 的 `contents.add(added)`）。
///
/// 这两条路在 Java 里作用于同一个对象、在 Rust 里作用于两个对象，为什么仍然等价：撤销栈严格
/// LIFO。任何后续编辑命令都排在 `add` 之上，所以它们只会在「记录已经回到 `contents` 里」时被
/// 执行或被撤销（`redo` 先重放 `add`，`undo` 先重放后续命令），于是 `add` 被撤销的那一刻，
/// 项目里那条记录必然已经回到创建时的状态，副本与原件同值。
pub struct AddCommand {
    /// 对应捕获的 `added`（`:716`）。
    pub(crate) record: ContentRecord,
    /// 对应捕获的 `placed`（`:717`）。
    pub(crate) node: CanvasNode,
    /// 对应捕获的 `stringsKey`（`:718`，即 `:718` 的 `stringKey(record, "name")`）。
    pub(crate) strings_key: String,
    /// 对应捕获的 `commandBoard`（`:721`）：这块画板在 `project.boards` 里的下标。
    pub(crate) board: usize,
    /// 对应 `flags`（`:696`）。
    pub(crate) flags: DirtyFlags,
}

impl<'a> Command<EditState<'a>> for AddCommand {
    /// 对应 `undo()`（`:725-731`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        // :726 `project.contents.remove(added);`
        let record_id = self.record.id.clone();
        state
            .project
            .contents
            .retain(|record| record.id != record_id);
        // :727 `project.strings.removeKey(stringsKey);`
        state.project.strings.remove_key(&self.strings_key);
        // :728-730 摘掉卡片，再摘掉所有连到它上面的连线。
        // Java 拿着 `commandBoard` 这个引用直接改，即使那块画板已从 `project.boards` 摘掉也照改；
        // 这里按下标取，取不到就是那块画板已不在项目里，跳过（同模块头第 2 点的说明）。
        if let Some(board) = state.project.boards.get_mut(self.board) {
            let node_id = self.node.id.clone();
            board.nodes.retain(|node| node.id != node_id);
            // :729-730 `edges.removeIf(edge -> placed.id.equals(edge.from) || placed.id.equals(edge.to))`
            board
                .edges
                .retain(|edge| edge.from != node_id && edge.to != node_id);
        }
        self.flags.restore_before(state.project); // :731
        Ok(())
    }

    /// 对应 `redo()`（`:734-742`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :735
        // :736 `project.contents.add(added);`——Java 无条件追加，重复追加也照加，这里同形。
        state.project.contents.push(self.record.clone());
        // :737 `project.strings.put(stringsKey, "en", added.name);`
        let name = self.record.name.clone();
        state.project.strings.put(&self.strings_key, "en", &name);
        state.project.strings.mark_bundle_dirty("en"); // :738
        // :739-741 卡片先查重复再追加（连线不恢复，`:743` 的注释写明是刻意的）。
        if let Some(board) = state.project.boards.get_mut(self.board) {
            let node_id = self.node.id.clone();
            if !board.nodes.iter().any(|node| node.id == node_id) {
                board.nodes.push(self.node.clone());
            }
        }
        Ok(())
    }
}

/// 对应 `rename` 里的匿名命令（`:805-825`）：改一条内容的名字，连带把整个工程的引用、
/// 受管 bundle 键和 ammoTypes 内联路径一起搬过去。
///
/// Java 捕获了 `rewrite` / `inlineEdits` / `record`（活对象）/ `previous` / `newName` / `flags`
/// 六个量，这里逐个落成字段；`record` 那个活对象换成一串 id（见模块头第 2 点）——
/// `undo` / `redo` 都按 id 回查项目里那条活记录再改它的 `name` / `dirty`。
///
/// `rewrite` 与 `inline_edits` 原样留着：[`RewriteResult::changes`] 里的 `Change.record` 是
/// **改动后的快照副本**（`mm_fmt::refs::rewrite` 的注释 :42-44 写明原因），所以
/// [`ProjectController::apply_rewrite`] 必须按 id 回查活记录再 `apply_tree`，
/// 而不能像 Java 那样直接对 `change.record()` 动手。
pub struct RenameCommand {
    /// 对应捕获的 `record`（`:805-825` 闭包捕获的方法参数）。
    pub(crate) record_id: String,
    /// 对应捕获的 `previous`（`:792`）。
    pub(crate) old_name: String,
    /// 对应捕获的 `newName`（`:764`）。
    pub(crate) new_name: String,
    /// 对应捕获的 `rewrite`（`:796-797`）。
    pub(crate) rewrite: RewriteResult,
    /// 对应捕获的 `inlineEdits`（`:800`）。
    pub(crate) inline_edits: Vec<InlinePathEdit>,
    /// 对应 `flags`（`:795`）。
    pub(crate) flags: DirtyFlags,
}

impl<'a> Command<EditState<'a>> for RenameCommand {
    /// 对应 `undo()`（`:807-813`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        ProjectController::apply_rewrite(state.project, &self.rewrite, true); // :808
        ProjectController::apply_inline_path_edits(state.project, &self.inline_edits, true); // :809
        // :810-811 `record.name = previous; record.dirty = true;`
        if let Some(record) = state.project.content_by_id_mut(&self.record_id) {
            record.name = self.old_name.clone();
            record.dirty = true;
        }
        // :812 `migrateBundleKeys(record, newName);`——把键从新名搬回旧名。
        // Java 用的是活对象，这里当场从项目里取当前状态；名字已经在 :810-811 改回旧名，
        // 于是 `record.fullName(internal)` 算出的正是「回退后」的键，与 Java 同值。
        let record = state.project.content_by_id(&self.record_id).cloned();
        if let Some(record) = record {
            ProjectController::migrate_bundle_keys(state.project, &record, &self.new_name);
        }
        self.flags.restore_before(state.project); // :813
        Ok(())
    }

    /// 对应 `redo()`（`:817-823`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :818
        if let Some(record) = state.project.content_by_id_mut(&self.record_id) {
            record.name = self.new_name.clone(); // :819
            record.dirty = true; // :820
        }
        let record = state.project.content_by_id(&self.record_id).cloned();
        if let Some(record) = record {
            ProjectController::migrate_bundle_keys(state.project, &record, &self.old_name); // :821
        }
        ProjectController::apply_rewrite(state.project, &self.rewrite, false); // :822
        ProjectController::apply_inline_path_edits(state.project, &self.inline_edits, false); // :823
        Ok(())
    }
}

/// 对应 Java `ProjectController.InlinePathEdit`（`:2065-2068`，record）：
/// 一条 `ammoTypes.<item>` 路径因为宿主改名而跟着变的内联记录。
///
/// Java 的 `record()` 分量是活对象，`applyInlinePathEdits`（`:2113-2118`）直接改它；
/// Rust 换成 `record_id`，回查后再改（同模块头第 2 点）。
pub struct InlinePathEdit {
    /// 对应 `record`（`:2066`）。
    pub(crate) record_id: String,
    /// 对应 `oldPath`（`:2066`）。
    pub(crate) old_path: String,
    /// 对应 `newPath`（`:2066`）。
    pub(crate) new_path: String,
    /// 对应 `oldName`（`:2067`）：路径末段改之前的显示名。
    pub(crate) old_name: String,
    /// 对应 `newName`（`:2067`）。
    pub(crate) new_name: String,
}

/// 对应 Java `ProjectController.RemovedInline`（`:2070-2073`，record）：
/// 因为 ammoTypes 键被删（或宿主被删）而一起摘掉的一条内联子弹，连同它的卡片与边。
///
/// Java 的 `record()` / `node()` / `edges()` 三个分量是活对象引用，撤销时直接塞回集合；
/// Rust 换成快照副本（模块头第 2 点），撤销时插入副本、按 id 判重。
/// `path` 与 `name` 是摘掉那一刻的 `inlinePath` / `name`（`:2151` 传的就是当时的值），
/// 撤销时原样写回（`:2202-2203`）。
#[derive(Clone)]
pub struct RemovedInline {
    /// 对应 `record`（`:2071`）。
    pub(crate) record: ContentRecord,
    /// 对应 `index`（`:2071`）：摘掉前它在 `project.contents` 里的下标，撤销时插回原位。
    pub(crate) index: usize,
    /// 对应 `path`（`:2071`）：`inlinePath` 当时的值。ammoTypes 那一族先过滤过 `null`
    /// （`:2135`），宿主那一族**没有**过滤（`:2187` 直接传 `inline.inlinePath`），故为 `Option`。
    pub(crate) path: Option<String>,
    /// 对应 `name`（`:2072`）。
    pub(crate) name: String,
    /// 对应 `node`（`:2072`）：`board == null` 或找不到卡片时就是 Java 的 `null`。
    pub(crate) node: Option<CanvasNode>,
    /// 对应 `edges`（`:2072`）：这张卡片当时挂着的全部边。
    pub(crate) edges: Vec<CanvasEdge>,
}

/// 删除命令里一条目标记录的现场快照，对应 Java `deleteContents` 在动手之前收好的那五个平行
/// 列表（`nodes` `:872`、`touching` `:873`、`ownerTrees` `:874`、`owners` `:875`、
/// `keyLocales` `:876`）加上 `targets` 本身——Rust 里命令要能脱离控制器独自重放，
/// 就把「按下标一一对应的平行列表」折成「每条目标一个结构体」。
pub struct DeleteTarget {
    /// 对应 `targets.get(i)`（`:865` 的活记录）：撤销时按 Java `:958` 原样 `add` 回去，
    /// 所以存的必须是摘走那一刻的副本。
    pub(crate) record: ContentRecord,
    /// 对应 `nodes.get(i)`（`:881`）。
    pub(crate) node: Option<CanvasNode>,
    /// 对应 `touching.get(i)`（`:882-886`）。
    pub(crate) touching: Vec<CanvasEdge>,
    /// 对应 `owners.get(i)`（`:887-888`）里那条宿主：Java 存引用，这里存它的 id。
    pub(crate) owner_id: Option<String>,
    /// 对应 `ownerTrees.get(i)`（`:889`）：宿主在**任何改动之前**的字段树。
    pub(crate) owner_tree: Option<Object>,
    /// 对应 `keyLocales.get(i)`（`:890`，即 `bundleLocales(record)`）。
    /// 用 `Vec` 而不是映射，为的是保住 Java `LinkedHashMap` 的插入顺序。
    pub(crate) key_locales: Vec<(String, KeyedMap<String>)>,
}

/// 对应 Java `deleteContents` 里的匿名命令（`:952-1021`）：一次删掉一批记录（框选多选后的
/// 批量删除），撤销把它们连同卡片、边、宿主字段树和 bundle 键一并放回。
pub struct DeleteCommand {
    /// 对应捕获的 `targets` 与那五个平行列表（`:947-950` 的 `captured*`）。
    pub(crate) targets: Vec<DeleteTarget>,
    /// 对应捕获的 `removedInline`（`:901-905`）。
    pub(crate) removed_inline: Vec<RemovedInline>,
    /// 对应捕获的 `rewrite`（`:898`）。
    pub(crate) rewrite: RewriteResult,
    /// 对应捕获的 `orderBefore`（`:869`）。
    pub(crate) order_before: Vec<String>,
    /// 对应捕获的 `orderAfter`（`:911`）。
    pub(crate) order_after: Vec<String>,
    /// 对应捕获的 `flags`（`:895`）。
    pub(crate) flags: DirtyFlags,
    /// 对应捕获的 `commandBoard`（`:950`）：命令创建时那块画板的下标。
    pub(crate) board: Option<usize>,
}

impl<'a> Command<EditState<'a>> for DeleteCommand {
    /// 对应 `undo()`（`:954-989`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        // Java 先 `applyRewrite(rewrite, true)`（`:955`）再往 `contents` 里放回目标（`:958`）：
        // 那一步能改到目标本身，靠的是 Java 手里的活引用。Rust 的 `apply_rewrite` 按 id 回查
        // 活记录，目标此刻还不在项目里就会被跳过，所以这里先把目标放回、再跑重写——
        // 两侧最终值相同：重写记录的 `before` 侧与快照都是「动手之前」那棵树，
        // 宿主那一侧（`:978-983`）同样写回采集于 `:889` 的改动前字段树，先后顺序不影响结果。
        for target in self.targets.iter().rev() {
            let record = target.record.clone(); // :958 `project.contents.add(record)`
            let id = record.id.clone().unwrap_or_default();
            if !state.project.contents.iter().any(|live| live.id.as_deref() == Some(id.as_str())) {
                state.project.contents.push(record);
            }
            // :959-965 `capturedKeyLocales.get(i).forEach((key, byLocale) -> {…})`
            for (key, by_locale) in &target.key_locales {
                state.project.strings.remove_key(key); // :960
                for locale in by_locale.keys().cloned().collect::<Vec<String>>() {
                    let value = by_locale.get(&locale).cloned().unwrap_or_default();
                    state.project.strings.put(key, &locale, &value); // :962
                    state.project.strings.mark_bundle_dirty(&locale); // :963
                }
            }
            // :966-977 卡片与它挂着的边放回。Java 的 `commandBoard` 是捕获时那块画板的引用，
            // 这里按下标回查；查不到就整段跳过（等价于 Java 里那块画板已从 `project.boards`
            // 摘走、只剩命令自己攥着的那个对象）。
            if let Some(board) = self.board
                && let Some(board) = state.project.boards.get_mut(board)
            {
                // :967-969 `node != null && !commandBoard.nodes.contains(node)`
                if let Some(node) = target.node.as_ref() {
                    let node_id = node.id.clone().unwrap_or_default();
                    if !board.nodes.iter().any(|live| live.id.as_deref() == Some(node_id.as_str()))
                    {
                        board.nodes.push(node.clone());
                    }
                }
                // :970-977 一条边的两端可能都在这一批里，所以每条边只放回一次。
                for edge in &target.touching {
                    let edge_id = edge.id.clone().unwrap_or_default();
                    if !board
                        .edges
                        .iter()
                        .any(|live| live.id.as_deref() == Some(edge_id.as_str()))
                    {
                        board.edges.push(edge.clone());
                    }
                }
            }
            // :978-983 宿主的字段树整棵放回。
            if let (Some(owner_id), Some(tree)) = (target.owner_id.as_deref(), target.owner_tree.as_ref())
                && let Some(owner) = state.project.content_by_id_mut(owner_id)
            {
                owner.apply_tree(tree.clone());
                owner.dirty = true;
            }
        }
        ProjectController::apply_rewrite(state.project, &self.rewrite, true); // :955（见上方说明）
        ProjectController::apply_removed_inline(state.project, self.board, &self.removed_inline, true); // :985
        // :986-987 保存顺序整表换回删除前。
        state.project.meta.content_order.clear();
        state.project.meta.content_order.extend(self.order_before.iter().cloned());
        self.flags.restore_before(state.project); // :988
        Ok(())
    }

    /// 对应 `redo()`（`:992-1020`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :993
        ProjectController::apply_rewrite(state.project, &self.rewrite, false); // :994
        for target in &self.targets {
            let record_id = target.record.id.clone().unwrap_or_default();
            // :998-1007 内联内容只存在于宿主体内，所以要从宿主的树里把那个对象再摘一次。
            // 注意这里读的是 `target.record` 这份**快照**上的 `inlineOwnerId` / `inlinePath`，
            // 与 Java 读活对象上的同两个字段等价——删除不会改动这两个分量。
            if target.record.is_inline() {
                let owner_id = target.record.inline_owner_id.clone();
                let path = target.record.inline_path.clone();
                if let (Some(owner_id), Some(path)) = (owner_id, path) {
                    ProjectController::remove_inline_from_owner(state.project, &owner_id, &path);
                }
            }
            // :1008-1009 先标脏再删键，删的正是刚标记的那些语言。
            let keys: Vec<String> = target.key_locales.iter().map(|(key, _)| key.clone()).collect();
            for key in &keys {
                ProjectController::mark_key_locales_dirty(state.project, key);
            }
            for key in &keys {
                state.project.strings.remove_key(key);
            }
            // :1010 `project.contents.remove(record)`
            state
                .project
                .contents
                .retain(|live| live.id.as_deref() != Some(record_id.as_str()));
            // :1011-1014 摘卡片，再摘它挂着的那些边（Java 的 `recordEdges::contains`
            // 是引用相等，这里换成边 id 相等）。
            if let Some(board) = self.board {
                let edge_ids: Vec<String> = target
                    .touching
                    .iter()
                    .map(|edge| edge.id.clone().unwrap_or_default())
                    .collect();
                if let Some(board_ref) = state.project.boards.get_mut(board) {
                    board_ref
                        .nodes
                        .retain(|live| live.content_id.as_deref() != Some(record_id.as_str()));
                    board_ref.edges.retain(|live| {
                        !edge_ids.iter().any(|id| Some(id) == live.id.as_ref())
                    });
                }
            }
        }
        ProjectController::apply_removed_inline(state.project, self.board, &self.removed_inline, false); // :1016
        // :1017-1018 保存顺序换成删除后那份。
        state.project.meta.content_order.clear();
        state.project.meta.content_order.extend(self.order_after.iter().cloned());
        state.project.boards_dirty = true; // :1019
        Ok(())
    }
}

/// 对应 Java `connect` 里的匿名命令（`:1092-1110`）：画一条引用线——把源名字写进目标记录的字段，
/// 并在画板上留下那条边。
pub struct ConnectCommand {
    /// 对应捕获的 `to`（`:1072`）：Java 攥的是活记录，这里存它的 id（模块头第 2 点）。
    pub(crate) to_id: String,
    /// 对应捕获的 `field`（`:1061`）。
    pub(crate) field: FieldDef,
    /// 对应捕获的 `revert`（`:1073`）：`ReferenceOps.apply` 返回的那份撤销信息。
    pub(crate) revert: Revert,
    /// 对应捕获的 `sourceName`（`:1067`）。
    pub(crate) source_name: String,
    /// 对应捕获的 `appliedSource`（`:1090`）：就是 `sourceTypeName`（`:1071`）当时那份值，
    /// redo 拿它重放同一次写入。
    pub(crate) applied_source: Option<String>,
    /// 对应捕获的 `edge`（`:1081-1087`）：这条命令新建的那张边。
    pub(crate) edge: CanvasEdge,
    /// 对应捕获的 `flags`（`:1072`）：只盯 `to` 这一条记录。
    pub(crate) flags: DirtyFlags,
    /// 对应捕获的 `commandBoard`（`:1089`）。
    pub(crate) board: Option<usize>,
}

impl<'a> Command<EditState<'a>> for ConnectCommand {
    /// 对应 `undo()`（`:1094-1099`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        // :1095-1096 先按撤销信息回退字段，再翻脏（Java 的这两行都作用于同一个活记录 `to`）。
        if let Some(record) = state.project.content_by_id_mut(&self.to_id) {
            mm_fmt::refs::revert(record, &self.field, &self.revert); // :1095
            record.dirty = true; // :1096
        }
        // :1097 `commandBoard.edges.remove(edge)`：Java 按引用摘，这里按边 id 摘。
        if let Some(board) = self.board {
            let edge_id = self.edge.id.clone().unwrap_or_default();
            if let Some(board) = state.project.boards.get_mut(board) {
                board
                    .edges
                    .retain(|live| live.id.as_deref() != Some(edge_id.as_str()));
            }
        }
        self.flags.restore_before(state.project); // :1098
        Ok(())
    }

    /// 对应 `redo()`（`:1101-1109`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1103
        // :1104-1105 重放这次写入；Java 把 `apply` 的返回值丢掉了（撤销信息只在 undo 用一次），
        // 所以这里同样不看返回是 `Some` 还是 `None`。
        if let Some(record) = state.project.content_by_id_mut(&self.to_id) {
            let _ = mm_fmt::refs::apply_with_type(
                record,
                &self.field,
                &self.source_name,
                self.applied_source.as_deref(),
            ); // :1104
            record.dirty = true; // :1105
        }
        // :1106-1108 边不在了才补回去（`!contains` 的判据换成 id）。
        if let Some(board) = self.board {
            let edge_id = self.edge.id.clone().unwrap_or_default();
            if let Some(board) = state.project.boards.get_mut(board)
                && !board
                    .edges
                    .iter()
                    .any(|live| live.id.as_deref() == Some(edge_id.as_str()))
            {
                board.edges.push(self.edge.clone());
            }
        }
        Ok(())
    }
}

/// 对应 Java `disconnect` 主分支里的匿名命令（`:1255-1277`）：摘掉一条边，并把它代表的那份字段值
/// 一起撤掉。
///
/// `field` 可能是 `None`——Java `:1245` 的 `referenceField` 解析不出来时返回 `null`，
/// 于是 :1247 的 `removed` 恒为 `false`，命令里那两个 `if (field != null)` 分支也整段跳过，
/// 只剩下「摘边」这一件事（边仍然要能撤销）。
pub struct DisconnectCommand {
    /// 对应捕获的 `to`（`:1212`）：同样按 id 存。
    pub(crate) to_id: String,
    /// 对应捕获的 `field`（`:1245`）。
    pub(crate) field: Option<FieldDef>,
    /// 对应捕获的 `sourceName`（`:1242`）。
    pub(crate) source_name: String,
    /// 对应捕获的 `sourceTypeName`（`:1244`）。
    pub(crate) source_type_name: Option<String>,
    /// 对应捕获的 `edge`（`:1206` 的入参）。
    pub(crate) edge: CanvasEdge,
    /// 对应捕获的 `flags`（`:1246`）：只盯 `to`。
    pub(crate) flags: DirtyFlags,
    /// 对应捕获的 `commandBoard`（`:1253`）。
    pub(crate) board: Option<usize>,
}

impl<'a> Command<EditState<'a>> for DisconnectCommand {
    /// 对应 `undo()`（`:1257-1266`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        // :1258-1261 字段还在（或本来就没被撤掉）才重放那次连接。
        if let Some(field) = self.field.as_ref()
            && let Some(record) = state.project.content_by_id_mut(&self.to_id)
        {
            let _ = mm_fmt::refs::apply_with_type(
                record,
                field,
                &self.source_name,
                self.source_type_name.as_deref(),
            ); // :1259
            record.dirty = true; // :1260
        }
        // :1262-1264 边不在了才补回去。
        if let Some(board) = self.board {
            let edge_id = self.edge.id.clone().unwrap_or_default();
            if let Some(board) = state.project.boards.get_mut(board)
                && !board
                    .edges
                    .iter()
                    .any(|live| live.id.as_deref() == Some(edge_id.as_str()))
            {
                board.edges.push(self.edge.clone());
            }
        }
        self.flags.restore_before(state.project); // :1265
        Ok(())
    }

    /// 对应 `redo()`（`:1268-1276`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1270
        // :1271-1274 `ReferenceOps.remove` 的返回值在 redo 里被丢掉（Java 同样没接）。
        if let Some(field) = self.field.as_ref()
            && let Some(record) = state.project.content_by_id_mut(&self.to_id)
        {
            let _ = mm_fmt::refs::remove(record, field, &self.source_name); // :1272
            record.dirty = true; // :1273
        }
        // :1275 摘边：Java 不判存在，`List.remove` 自己容错。
        if let Some(board) = self.board {
            let edge_id = self.edge.id.clone().unwrap_or_default();
            if let Some(board) = state.project.boards.get_mut(board) {
                board
                    .edges
                    .retain(|live| live.id.as_deref() != Some(edge_id.as_str()));
            }
        }
        Ok(())
    }
}

/// 对应 Java `disconnect` 里「目标记录已经没了」那条分支的匿名命令（`:1224-1238`）：只摘边。
///
/// Java `:1214-1216` 的注释把这件事讲得很直白——指向已删除记录的边是画板上的陈旧授权数据，
/// 删掉它仍然是用户做的一次编辑，所以必须像其它编辑一样可撤销；在此之前这一处裸删是唯一一个
/// Ctrl+Z 撤不回来的删除动作（对应测试 #4）。
pub struct DetachEdgeCommand {
    /// 对应捕获的 `edge`（`:1206` 的入参）。
    pub(crate) edge: CanvasEdge,
    /// 对应捕获的 `flags`（`:1218`）：Java 这里是 `new DirtyFlags()`，**一条记录都不盯**，
    /// 于是 `restore*` 只回放假想中的画板/meta 两个总标记。
    pub(crate) flags: DirtyFlags,
    /// 对应捕获的 `commandBoard`（`:1217`）。
    pub(crate) board: Option<usize>,
}

impl<'a> Command<EditState<'a>> for DetachEdgeCommand {
    /// 对应 `undo()`（`:1226-1231`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        // :1227-1229 边不在了才补回去。注意 Java **没有**在这里补 `boardsDirty`，
        // 而是靠 :1230 的 `restoreBefore` 把它放回命令创建前的值。
        if let Some(board) = self.board {
            let edge_id = self.edge.id.clone().unwrap_or_default();
            if let Some(board) = state.project.boards.get_mut(board)
                && !board
                    .edges
                    .iter()
                    .any(|live| live.id.as_deref() == Some(edge_id.as_str()))
            {
                board.edges.push(self.edge.clone());
            }
        }
        self.flags.restore_before(state.project); // :1230
        Ok(())
    }

    /// 对应 `redo()`（`:1233-1237`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1235
        if let Some(board) = self.board {
            let edge_id = self.edge.id.clone().unwrap_or_default();
            if let Some(board) = state.project.boards.get_mut(board) {
                board
                    .edges
                    .retain(|live| live.id.as_deref() != Some(edge_id.as_str())); // :1236
            }
        }
        Ok(())
    }
}

/// 对应 `commitMoves` 里的匿名命令（`:1351-1378`）：把一次**已经落地**的卡片拖动记进撤销栈
/// （Java `:1339` 的注释写明坐标是拖动本身先应用好的，这里只是补一条可撤销的记录）。
///
/// 必须登记的脚手架差异：Java 的两个入参用的是两种 map——`Map<String, Point2D> before` 与
/// `Map<String, double[]> after`（`:1340`），而 `undo()` 的第一步（`:1367-1368`）就是把 `before`
/// 逐条 `new double[] {point.getX(), point.getY()}` 转成后者的形状，再交给内部 `apply`。
/// 两种值类型只是调用方便的便利、语义完全相同，所以 Rust 侧统一成 `KeyedMap<[f64; 2]>`，
/// 那个转换循环随之塌缩成「undo 直接拿 before 用」。
pub struct CommitMovesCommand {
    /// 对应捕获的 `before`（`:1340` 的入参，`:1368` 读）。
    pub(crate) before: KeyedMap<[f64; 2]>,
    /// 对应捕获的 `after`（`:1340` 的入参，`:1376` 读）。
    pub(crate) after: KeyedMap<[f64; 2]>,
    /// 对应捕获的 `flags`（`:1345`）：Java 是 `new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
    /// 对应捕获的 `commandBoard`（`:1350`）。
    pub(crate) board: Option<usize>,
}

impl CommitMovesCommand {
    /// 对应匿名类里那个私有 `apply(Map<String, double[]> positions)`（`:1352-1363`）：
    /// 按节点 id 把坐标写回命令创建时那块画板。逐条遍历节点这件事与外部的
    /// `applyPositions`（`:1988-1999`）是同一份循环，故复用 [`ProjectController::apply_positions`]。
    fn apply(&self, state: &mut EditState<'_>, positions: &KeyedMap<[f64; 2]>) {
        // :1353-1355 `if (commandBoard == null) return;`
        let Some(board) = self.board else {
            return;
        };
        // `:1356-1362` 那圈 `for (CanvasNode node : commandBoard.nodes)`；按下标取不到板子
        // 就是那块板已不在项目里（模块头第 2 点），与 Java 拿着已摘除的引用改是同一种状态。
        if let Some(board) = state.project.boards.get_mut(board) {
            ProjectController::apply_positions(board, positions);
        }
    }
}

impl<'a> Command<EditState<'a>> for CommitMovesCommand {
    /// 对应 `undo()`（`:1366-1371`）：先回到拖动前的坐标，再把脏标记放回命令创建前。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.apply(state, &self.before); // :1367-1369（`:1367` 的转换循环已塌缩，见结构体文档）
        self.flags.restore_before(state.project); // :1370
        Ok(())
    }

    /// 对应 `redo()`（`:1374-1377`）：**先**放回脏标记再落坐标，顺序照 Java，不调换。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1375
        self.apply(state, &self.after); // :1376
        Ok(())
    }
}

/// reroute 一族命令的公共定位：在全项目的画板里按 id 找回那条边。
///
/// 为什么是「全项目」而不是「命令捕获的那块板」：Java 的三个匿名命令类（`:1408`、`:1440`、
/// `:1478`）捕获的都是 `CanvasEdge` **活对象**引用，改的就是那个对象本身，与它此刻挂在哪块板
/// 上无关，而且 `insertReroute` 那段里根本没有 `Board commandBoard = board;` 这样的捕获
/// （对比 `commitMoves` 的 `:1350`）。Rust 没有对象同一性（模块头第 2 点），边 id 全局唯一，
/// 按 id 找就是那个活对象最贴近的等价物；按下标存板反而会引入「画板列表增删导致下标漂移」
/// 这个 Java 不存在的失败模式。找不到边 = 边已被摘出项目，Java 那次改动落在没人再看的对象上，
/// 两边同样是「看不见」，故静默跳过。
pub(crate) fn reroute_edge<'b>(project: &'b mut ModProject, edge_id: &str) -> Option<&'b mut CanvasEdge> {
    for board in project.boards.iter_mut() {
        for edge in board.edges.iter_mut() {
            if edge.id.as_deref() == Some(edge_id) {
                return Some(edge);
            }
        }
    }
    None
}

/// 对应 `insertReroute` 的匿名命令（`:1408-1424`）：往一条连线的 reroute 点列**末尾**补一个点。
pub struct InsertRerouteCommand {
    /// 对应捕获的 `edge`（`:1397` 的入参，`:1411`/`:1421` 读）：见 [`reroute_edge`] 的偏差登记。
    pub(crate) edge_id: String,
    /// 捕获的 `index`（`:1402`）：`edge.reroutes.size()`，即**插入前**的长度。
    pub(crate) index: usize,
    /// 捕获的 `point`（`:1403`）：redo 要重新 append 的那个点。
    pub(crate) point: [f64; 2],
    /// 对应捕获的 `flags`（`:1404`）：`new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
}

impl<'a> Command<EditState<'a>> for InsertRerouteCommand {
    /// 对应 `undo()`（`:1410-1415`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        if let Some(edge) = reroute_edge(state.project, &self.edge_id) {
            // :1411-1413 `if (index < edge.reroutes.size()) edge.reroutes.remove(index);`
            // 用 `<` 而不是 `<=`：点已经不在了（比如撤销栈被别处动过）就不硬摘。
            if self.index < edge.reroutes.len() {
                edge.reroutes.remove(self.index);
            }
        }
        self.flags.restore_before(state.project); // :1414
        Ok(())
    }

    /// 对应 `redo()`（`:1418-1423`）：**先**放回脏标记再补点，顺序照 Java。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1419
        if let Some(edge) = reroute_edge(state.project, &self.edge_id) {
            // :1420-1422 守卫是 `index <= size`，而动作是 `add(point)`（**追加**，不是 `add(index, …)`）。
            // 原样保住：中间若又插过别的点，redo 会补到末尾而不是回到 index 处——Java 就是这个行为。
            if self.index <= edge.reroutes.len() {
                edge.reroutes.push(self.point);
            }
        }
        Ok(())
    }
}

/// 对应 `removeReroute` 的匿名命令（`:1440-1456`）：摘掉一个 reroute 点。
pub struct RemoveRerouteCommand {
    /// 对应捕获的 `edge`（`:1429` 的入参，`:1444`/`:1453` 读）：见 [`reroute_edge`] 的偏差登记。
    pub(crate) edge_id: String,
    /// 捕获的 `index`（`:1429` 的入参，`:1443`/`:1452` 读）：被摘那个点原来的下标。
    /// Java 的 `index < 0` 守卫（`:1432`）由 `usize` 类型承担。
    pub(crate) index: usize,
    /// 捕获的 `point`（`:1435`）：`edge.reroutes.get(index)`，undo 要插回去的那个点。
    pub(crate) point: [f64; 2],
    /// 对应捕获的 `flags`（`:1436`）：`new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
}

impl<'a> Command<EditState<'a>> for RemoveRerouteCommand {
    /// 对应 `undo()`（`:1442-1447`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        if let Some(edge) = reroute_edge(state.project, &self.edge_id) {
            // :1443-1445 `if (index <= size) edge.reroutes.add(index, point);`——用 `<=`，
            // 因为 `add(index, …)` 在 `index == size` 时就是追加，与 Java 的 List 语义一致。
            if self.index <= edge.reroutes.len() {
                edge.reroutes.insert(self.index, self.point);
            }
        }
        self.flags.restore_before(state.project); // :1446
        Ok(())
    }

    /// 对应 `redo()`（`:1450-1455`）：**先**放回脏标记再摘点。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1451
        if let Some(edge) = reroute_edge(state.project, &self.edge_id)
            && self.index < edge.reroutes.len()
        {
            edge.reroutes.remove(self.index); // :1452-1454
        }
        Ok(())
    }
}

/// 对应 `commitRerouteMove` 的匿名命令（`:1478-1494`）：把一次**已经落地**的 reroute 点拖动
/// 记进撤销栈（Java `:1461-1463` 的注释写明拖动本身已就地改过活点，这里只是补一条可撤销的记录，
/// 与 [`CommitMovesCommand`] 同构）。
pub struct CommitRerouteMoveCommand {
    /// 对应捕获的 `edge`（`:1465` 的入参，`:1481`/`:1490` 读）：见 [`reroute_edge`] 的偏差登记。
    pub(crate) edge_id: String,
    /// 捕获的 `index`（`:1465` 的入参，`:1480`/`:1489` 读）。Java 的 `index < 0` 守卫（`:1468`）
    /// 由 `usize` 类型承担。
    pub(crate) index: usize,
    /// 捕获的 `before`（`:1465` 的入参，`:1482` 读）。
    pub(crate) before: [f64; 2],
    /// 捕获的 `after`（`:1465` 的入参，`:1491` 读）。
    pub(crate) after: [f64; 2],
    /// 对应捕获的 `flags`（`:1475`）：`new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
}

impl<'a> Command<EditState<'a>> for CommitRerouteMoveCommand {
    /// 对应 `undo()`（`:1480-1485`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        if let Some(edge) = reroute_edge(state.project, &self.edge_id) {
            // :1481-1483 `if (index < size) edge.reroutes.set(index, before);`
            if self.index < edge.reroutes.len() {
                edge.reroutes[self.index] = self.before;
            }
        }
        self.flags.restore_before(state.project); // :1484
        Ok(())
    }

    /// 对应 `redo()`（`:1488-1493`）：**先**放回脏标记再落坐标。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1489
        if let Some(edge) = reroute_edge(state.project, &self.edge_id)
            && self.index < edge.reroutes.len()
        {
            edge.reroutes[self.index] = self.after; // :1490-1492
        }
        Ok(())
    }
}

/// 对应 `addBoard` 里的匿名命令（`:1860-1877`）：新增一块画板，并把它设为当前画板。
///
/// 需要登记的脚手架差异：Java 捕获的是 `created` 这个**活对象**引用（`:1850`）加 `previous`
/// 引用（`:1857`）。Rust 没有对象同一性（模块头第 2 点），所以这里存一份创建时的 `Board` 副本
/// （摘掉后 redo 要能把它重新入列），`previous` 换成下标——与 [`AddCommand`] 存 `record` 副本（`:716`）
/// 是同一个取舍，也共享 [`AddCommand`] 文档里那条「撤销栈严格 LIFO，所以副本与原件同值」的论证。
pub struct AddBoardCommand {
    /// 对应捕获的 `created`（`:1850`）：这块板自己的那份副本，`:1872-1874` 的
    /// `if (!project.boards.contains(created)) project.boards.add(created)` 要靠它重新入列。
    pub(crate) board: Board,
    /// 对应捕获的 `previous`（`:1857`）：Java 存的是当时的 `board` 引用，`null` 换成 `None`。
    pub(crate) previous: Option<usize>,
    /// 对应捕获的 `flags`（`:1854`）：`new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
}

impl<'a> Command<EditState<'a>> for AddBoardCommand {
    /// 对应 `undo()`（`:1862-1867`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        // :1863 `project.boards.remove(created);`——Java 按引用摘，这里按 id 摘。
        let id = self.board.id.clone();
        if let Some(position) = state.project.boards.iter().position(|board| board.id == id) {
            state.project.boards.remove(position);
        }
        // :1864-1865 一字不差的三元式：有 previous 就回 previous，否则「板列表空 → null，
        // 不然第 0 块」。
        *state.board = self.previous.or({
            if state.project.boards.is_empty() {
                None
            } else {
                Some(0)
            }
        });
        self.flags.restore_before(state.project); // :1866
        Ok(())
    }

    /// 对应 `redo()`（`:1870-1876`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1871
        let id = self.board.id.clone();
        // :1872-1874 板不在列表里才补回去（`contains(created)` 换成按 id 找）。
        if !state.project.boards.iter().any(|board| board.id == id) {
            state.project.boards.push(self.board.clone());
        }
        // :1875 `board = created;`——把「那个引用」换成「那个 id 现在的下标」。
        *state.board = state.project.boards.iter().position(|board| board.id == id);
        Ok(())
    }
}

/// 对应 `tidyLayout` 里的匿名命令（`:1903-1915`）：整块画板重排的两端坐标。
///
/// Java `:1894` 的注释讲得明白——一次彻底的重排太有破坏性，不能不可撤销，所以两端都快照。
pub struct TidyLayoutCommand {
    /// 对应捕获的 `commandBoard`（`:1902`）。
    pub(crate) board: Option<usize>,
    /// 对应捕获的 `before`（`:1895` 的 `snapshotPositions()`）。
    pub(crate) before: KeyedMap<[f64; 2]>,
    /// 对应捕获的 `after`（`:1897` 的 `snapshotPositions()`）。
    pub(crate) after: KeyedMap<[f64; 2]>,
    /// 对应捕获的 `flags`（`:1899`）。
    pub(crate) flags: DirtyFlags,
}

impl<'a> Command<EditState<'a>> for TidyLayoutCommand {
    /// 对应 `undo()`（`:1905-1908`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        // :1906 `applyPositions(commandBoard, before)`：Java 里 `commandBoard == null` 由
        // `applyPositions` 自己的 `target == null` 守卫兜住（`:1989-1991`），Rust 侧这个守卫
        // 由下面这层 `if let Some(...)` 承担（见 [`ProjectController::apply_positions`] 的注释）。
        if let Some(board) = self.board
            && let Some(board) = state.project.boards.get_mut(board)
        {
            ProjectController::apply_positions(board, &self.before); // :1906
        }
        // Java 这里**没有**显式设 boardsDirty：全靠下面这行把它放回命令创建前的值。
        self.flags.restore_before(state.project); // :1907
        Ok(())
    }

    /// 对应 `redo()`（`:1911-1914`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1912
        if let Some(board) = self.board
            && let Some(board) = state.project.boards.get_mut(board)
        {
            ProjectController::apply_positions(board, &self.after); // :1913
        }
        Ok(())
    }
}

/// 对应 `rebuildLinks` 里的匿名命令（`:1956-1970`）：整表换掉画板的连线。
///
/// Java 的 `commandBoard.edges.clear(); commandBoard.edges.addAll(before);`（`:1960-1961`）
/// 是在同一个 `List` 实例上原地换内容；Rust 侧 `Vec<CanvasEdge>` 是画板的独占字段，
/// 没有任何地方持有那张表的别名，所以整表赋值（`board.edges = self.before.clone()`）
/// 与 clear + addAll 的效果完全一致，这点在两个方法里各注一次。
pub struct RebuildLinksCommand {
    /// 对应捕获的 `before`（`:1943` 的 `new ArrayList<>(board.edges)`）。
    pub(crate) before: Vec<CanvasEdge>,
    /// 对应捕获的 `after`（`:1950` 的 `rebuild.edges()`）。
    pub(crate) after: Vec<CanvasEdge>,
    /// 对应捕获的 `flags`（`:1951`）：`new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
    /// 对应捕获的 `commandBoard`（`:1949`）。
    pub(crate) board: Option<usize>,
}

impl<'a> Command<EditState<'a>> for RebuildLinksCommand {
    /// 对应 `undo()`（`:1958-1962`）：**先** `restoreBefore` 再换表，顺序照 Java。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_before(state.project); // :1959
        if let Some(board) = self.board
            && let Some(board) = state.project.boards.get_mut(board)
        {
            board.edges = self.before.clone(); // :1960-1961 的 clear + addAll
        }
        Ok(())
    }

    /// 对应 `redo()`（`:1965-1969`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1966
        if let Some(board) = self.board
            && let Some(board) = state.project.boards.get_mut(board)
        {
            board.edges = self.after.clone(); // :1967-1968 的 clear + addAll
        }
        Ok(())
    }
}

/// 对应 `setPatch` 里的匿名命令（`:1790-1804`）：翻一条记录的 patch 标记。
///
/// Java 捕获的是 `record` 这个**活引用**；Rust 侧按 id 回查（模块头第 2 点）。id 查不到就等于
/// 这条记录已被摘出项目——Java 那半边仍会往一个不在 `contents` 里的对象上写字段，
/// 而那种状态在这里没有任何可观测差别（保存与校验都只看 `contents`）。
pub struct PatchCommand {
    /// 对应捕获的 `record`（`:1780` 入参，`:1793`、`:1801` 读写）。
    pub(crate) record_id: String,
    /// 对应捕获的 `previous`（`:1785`）。
    pub(crate) previous: bool,
    /// 对应捕获的 `patch`（`:1780` 入参，`:1801` 读）。
    pub(crate) patch: bool,
    /// 对应捕获的 `flags`（`:1786` 的 `new DirtyFlags(record)`）：只盯这一条记录。
    pub(crate) flags: DirtyFlags,
}

impl PatchCommand {
    pub(crate) fn new(record_id: &str, previous: bool, patch: bool, flags: DirtyFlags) -> Self {
        Self {
            record_id: record_id.to_string(),
            previous,
            patch,
            flags,
        }
    }

    /// 对应 `:1793-1794` 与 `:1801-1802` 里重复的那两行：写标记、再**无条件**翻脏。
    /// Java 的 `undo` 也要 `record.dirty = true`——撤销一个 patch 同样是一次要落盘的改动。
    fn apply(&self, state: &mut EditState<'_>, value: bool) {
        if let Some(record) = state.project.content_by_id_mut(&self.record_id) {
            record.patch = value;
            record.dirty = true;
        }
    }
}

impl<'a> Command<EditState<'a>> for PatchCommand {
    /// 对应 `undo()`（`:1792-1796`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.apply(state, self.previous); // :1793-1794
        self.flags.restore_before(state.project); // :1795
        Ok(())
    }

    /// 对应 `redo()`（`:1799-1803`）：**先**放回脏标记再写标记，顺序照 Java。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1800
        self.apply(state, self.patch); // :1801-1802
        Ok(())
    }
}

/// 对应 `applyRawTree` 里的匿名命令（`:1826-1840`）：整棵字段树换掉。
///
/// Java `:1814-1815` 的注释讲明了为什么要两端快照：编辑器替换的是**整棵树**，而树里的嵌套值
/// 与结构化编辑器共享、被它们原地改。Rust 侧 `Json` 整棵树按值克隆、不存在共享子树，
/// 所以「两端快照」这件事仍然要做（不然 undo 拿到的就是被后续编辑改过的树），
/// 但 `deepCopy()` 全部塌缩成 `Clone`。
pub struct RawTreeCommand {
    /// 对应捕获的 `record`（`:1809` 入参，`:1829`、`:1837` 写）。
    pub(crate) record_id: String,
    /// 对应捕获的 `before`（`:1816` 的 `record.tree().deepCopy()`）。
    pub(crate) before: Object,
    /// 对应捕获的 `after`（`:1825` 的 `record.tree().deepCopy()`）。
    pub(crate) after: Object,
    /// 对应捕获的 `flags`（`:1820` 的 `new DirtyFlags(record)`）。
    pub(crate) flags: DirtyFlags,
}

impl RawTreeCommand {
    pub(crate) fn new(record_id: &str, before: Object, after: Object, flags: DirtyFlags) -> Self {
        Self {
            record_id: record_id.to_string(),
            before,
            after,
            flags,
        }
    }

    /// 对应 `:1829-1830` 与 `:1837-1838` 那两对相同的语句：换树、翻脏。
    /// `deepCopy()`（`:1829`、`:1837`）在这里就是 `clone()`——交给记录的是一份独立副本，
    /// 命令自己留着的那两端快照因此不会被记录之后的编辑带动。
    fn apply(&self, state: &mut EditState<'_>, tree: &Object) {
        if let Some(record) = state.project.content_by_id_mut(&self.record_id) {
            record.apply_tree(tree.clone()); // :1829 / :1837
            record.dirty = true; // :1830 / :1838
        }
    }
}

impl<'a> Command<EditState<'a>> for RawTreeCommand {
    /// 对应 `undo()`（`:1828-1832`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.apply(state, &self.before);
        self.flags.restore_before(state.project); // :1831
        Ok(())
    }

    /// 对应 `redo()`（`:1835-1839`）。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1836
        self.apply(state, &self.after);
        Ok(())
    }
}

/// 对应 `rebindCarrier` 里的匿名命令（`:1606-1620`）：换一张载体卡片绑定的记录 / 直接引用名。
///
/// 必须登记的脚手架差异：Java 的匿名类捕获 `node` **活引用**（`:1592` 从当时的当前画板里找到的
/// 那个对象），既不记 `board` 也不记节点下标——引用本身就隐含了「它在哪块板上」。
/// Rust 交不出这种引用，于是记 `(board 下标, 节点 id)` 二元组：下标就是 Java「节点所属的那块板」
/// 的等价物，切画板不会把这条命令的作用点搬走（与 `:719-721` 特意交代过的 `addContent` 同理）。
pub struct RebindCarrierCommand {
    /// 对应命令创建时的当前画板（Java 由 `node` 引用隐式携带，见结构体文档）。
    pub(crate) board: Option<usize>,
    /// 对应 `node.id`（`:1602-1603` 通过 `node` 读不到 id，这里靠它定位）。
    pub(crate) node_id: String,
    /// 对应捕获的 `oldContentId`（`:1596`）。
    pub(crate) old_content_id: Option<String>,
    /// 对应捕获的 `oldRefName`（`:1597`）。
    pub(crate) old_ref_name: Option<String>,
    /// 对应捕获的 `newContentId`（`:1590` 入参，`:1617` 读）。
    pub(crate) new_content_id: Option<String>,
    /// 对应捕获的 `newRefName`（`:1590` 入参，`:1618` 读）。
    pub(crate) new_ref_name: Option<String>,
    /// 对应捕获的 `flags`（`:1601`）：`new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
}

impl RebindCarrierCommand {
    pub(crate) fn new(
        board: Option<usize>,
        node_id: &str,
        old_content_id: Option<String>,
        old_ref_name: Option<String>,
        new_content_id: Option<String>,
        new_ref_name: Option<String>,
        flags: DirtyFlags,
    ) -> Self {
        Self {
            board,
            node_id: node_id.to_string(),
            old_content_id,
            old_ref_name,
            new_content_id,
            new_ref_name,
            flags,
        }
    }

    /// 对应 `:1602-1603` 与 `:1609-1610`、`:1617-1618` 里重复的同一对赋值。
    fn apply(&self, state: &mut EditState<'_>, content_id: &Option<String>, ref_name: &Option<String>) {
        // :1602 之前 Java 已经确定 `node != null`；这里取不到板子或节点，就是那块板 / 那张卡片
        // 已不在项目里（模块头第 2 点），与 Java 拿着已摘除的对象写字段是同一种状态。
        let Some(board) = self.board else { return };
        let Some(board) = state.project.boards.get_mut(board) else {
            return;
        };
        if let Some(node) = board
            .nodes
            .iter_mut()
            .find(|node| node.id.as_deref() == Some(self.node_id.as_str()))
        {
            node.content_id = content_id.clone();
            node.ref_name = ref_name.clone();
        }
    }
}

impl<'a> Command<EditState<'a>> for RebindCarrierCommand {
    /// 对应 `undo()`（`:1608-1612`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.apply(state, &self.old_content_id, &self.old_ref_name); // :1609-1610
        self.flags.restore_before(state.project); // :1611
        Ok(())
    }

    /// 对应 `redo()`（`:1615-1619`）：**先**放回脏标记再改绑定，顺序照 Java。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1616
        self.apply(state, &self.new_content_id, &self.new_ref_name); // :1617-1618
        Ok(())
    }
}

/// 对应 `detachNodes` 里的匿名命令（`:1661-1689`）：摘掉若干载体卡片连同它们的连线，
/// **不动**卡片指向的记录（Java `:1625-1626` 的注释：记录留在资产工作区里）。
///
/// 需要登记的脚手架差异：Java 捕获 `List.copyOf(removed)` 里的 `CanvasNode` **活对象**引用，
/// `contains` / `remove` 全凭引用相等（`:1666`、`:1670`、`:1683`）。Rust 侧存的是这些节点的
/// 副本，`contains` 换成按 `CanvasNode::id` 找、`remove` 换成按下标摘——id 唯一性就是
/// Java 依赖的那个性质（模块头第 2 点）。`capturedTouching` 里那批边同理按 `CanvasEdge::id` 判。
pub struct DetachNodesCommand {
    /// 对应捕获的 `capturedNodes`（`:1648`）。
    pub(crate) captured_nodes: Vec<CanvasNode>,
    /// 对应捕获的 `capturedTouching`（`:1649`）：与 `capturedNodes` 一一对应，
    /// 第 `i` 项是第 `i` 张卡片在摘除前连着的那些边。
    pub(crate) captured_touching: Vec<Vec<CanvasEdge>>,
    /// 对应捕获的 `flags`（`:1651`）：`new DirtyFlags()`，一条记录都不盯。
    pub(crate) flags: DirtyFlags,
    /// 对应捕获的 `commandBoard`（`:1650`）。
    pub(crate) board: Option<usize>,
}

impl DetachNodesCommand {
    pub(crate) fn new(
        board: Option<usize>,
        captured_nodes: Vec<CanvasNode>,
        captured_touching: Vec<Vec<CanvasEdge>>,
        flags: DirtyFlags,
    ) -> Self {
        Self {
            captured_nodes,
            captured_touching,
            flags,
            board,
        }
    }

    /// 把命令创建时那块板借出来；取不到就是它已不在项目里（模块头第 2 点）。
    fn board_mut<'b>(&self, state: &'b mut EditState<'_>) -> Option<&'b mut Board> {
        let index = self.board?;
        state.project.boards.get_mut(index)
    }

    /// 两个 id 是否同一个。Java 比的是**引用**，而 `CanvasNode.id` 在 Java 里恒不为 null，
    /// 所以 `node.id.equals(edge.from)` 在 `edge.from` 为 null 时必然是 false。Rust 两侧都是
    /// `Option<String>`，直接 `==` 会让「两边都缺 id」误判成同一个，故显式要求非空。
    pub(crate) fn same_id(left: &Option<String>, right: &Option<String>) -> bool {
        left.is_some() && left == right
    }

    pub(crate) fn drop_edges_of(board: &mut Board, node_id: &Option<String>) {
        board.edges.retain(|edge| {
            !(Self::same_id(node_id, &edge.from) || Self::same_id(node_id, &edge.to))
        });
    }
}

impl<'a> Command<EditState<'a>> for DetachNodesCommand {
    /// 对应 `undo()`（`:1663-1677`）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        if let Some(board) = self.board_mut(state) {
            // :1664-1674 逐张卡片把节点补回去，再补回它当初连着的那些边；
            // 两处 `if (!...contains(...))` 的幂等守卫原样保住（同一条边可能连着两张被摘的卡片，
            // 不加守卫就会被加两遍）。
            for (node, edges) in self.captured_nodes.iter().zip(self.captured_touching.iter()) {
                if !board.nodes.iter().any(|seen| seen.id == node.id) {
                    board.nodes.push(node.clone()); // :1667
                }
                for edge in edges {
                    if !board.edges.iter().any(|seen| seen.id == edge.id) {
                        board.edges.push(edge.clone()); // :1671
                    }
                }
            }
        }
        state.project.boards_dirty = true; // :1675
        self.flags.restore_before(state.project); // :1676
        Ok(())
    }

    /// 对应 `redo()`（`:1680-1688`）：**先**放回脏标记再摘卡片，顺序照 Java。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.flags.restore_after(state.project); // :1681
        if let Some(board) = self.board_mut(state) {
            for node in &self.captured_nodes {
                // :1683 `commandBoard.nodes.remove(node)`——按 id 摘第一次出现的那张。
                if let Some(position) =
                    board.nodes.iter().position(|seen| seen.id == node.id)
                {
                    board.nodes.remove(position);
                }
                Self::drop_edges_of(board, &node.id); // :1684-1685
            }
        }
        state.project.boards_dirty = true; // :1687
        Ok(())
    }
}

/// 贴图导入命令（2026-11-25 新增，**授权的新设计**：Java modmaker 没有应用内贴图导入，
/// 本命令无 Java 对应物，登记理由见 `src-tauri/src/sprite_asset.rs` 模块头）。
///
/// 与其余命令族的关键差异：它改的是**磁盘字节 + 资产账本**，不是内存模型，所以
/// undo/redo 可失败（[`CommandError`]）：磁盘写失败 → `Io`，记录被带外删除 → `NotFound`，
/// 项目无磁盘根 / 目标越界 → `Domain`。失败时 [`crate::undo_manager::UndoManager`] 把条目
/// 放回栈顶，历史不被破坏、可重试；因此**先写磁盘、后改账本**——磁盘失败时内存分毫未动。
///
/// 已知语义（契约登记，刻意不做）：
/// - undo 恢复的是「导入前快照」，**不做外部变更检测**——导入后用户又在外部改了该 PNG，
///   undo 仍按条目里的旧字节恢复（与字段命令按快照回滚是同一种语义）。
/// - 字节整段持有在内存里：Mindustry 贴图是小 PNG（典型 < 100 KB），历史有界
///   （`UndoManager` 的 `LIMIT` = 200 条）封住了最坏占用；不设单文件硬上限，
///   若将来出现超大贴图顾虑，再考虑把字节溢出到临时文件（当前不做，YAGNI）。
/// - 不带 [`DirtyFlags`]：资产账本与贴图字节不归脏标记守护（导入路径本身也不翻脏标记，
///   贴图直接落盘）；校验缓存作废与广播由控制器 undo/redo 的 `fire_project_changed`
///   与壳的 `after_mutation` 既有口径承担，不在命令里另造一份。
pub struct SpriteImportCommand {
    /// 导入目标的记录 id：undo/redo 前按它回查，记录被带外删除时报 `NotFound`。
    pub(crate) record_id: String,
    /// 目标相对路径（`sprites/<记录名>.png`，导入时按读侧口径解析并快照）。
    pub(crate) relative: String,
    /// 导入前目标文件的完整字节；`None` = 首次导入（当时没有旧文件），undo 时删除文件。
    pub(crate) old_bytes: Option<Vec<u8>>,
    /// 导入写入的完整字节，redo 重放用。
    pub(crate) new_bytes: Vec<u8>,
    /// 导入前账本里同路径条目的快照；`None` = 当时账本没有这条，undo 时摘掉。
    pub(crate) asset_before: Option<AssetRef>,
}

impl SpriteImportCommand {
    /// 由导入内核（`sprite_asset::import_sprite`）在写入前收齐两端快照后构造。
    pub fn new(
        record_id: &str,
        relative: &str,
        old_bytes: Option<Vec<u8>>,
        new_bytes: Vec<u8>,
        asset_before: Option<AssetRef>,
    ) -> Self {
        Self {
            record_id: record_id.to_string(),
            relative: relative.to_string(),
            old_bytes,
            new_bytes,
            asset_before,
        }
    }

    /// 记录必须还在项目里：被带外删除时报 `NotFound`（条目留栈可重试），
    /// 不静默往一条已不存在的记录名下写文件。
    fn require_record(&self, state: &EditState<'_>) -> Result<(), CommandError> {
        if state.project.content_by_id(&self.record_id).is_none() {
            return Err(CommandError::NotFound(self.record_id.clone()));
        }
        Ok(())
    }

    /// 目标绝对路径：与导入内核同一份防护（`safe_resolve` 归一化后必须仍在项目根内）。
    fn target(&self, state: &EditState<'_>) -> Result<PathBuf, CommandError> {
        let root = state.project.root.as_deref().ok_or_else(|| {
            CommandError::Domain("项目尚未落盘，无法回滚贴图文件".to_string())
        })?;
        mm_io::project_io::safe_resolve(root, &self.relative).ok_or_else(|| {
            CommandError::Domain(format!("贴图目标路径越出项目根：{}", self.relative))
        })
    }

    /// 账本回滚到导入前：有旧条目装回旧条目（保位），没有则摘掉本次登记。
    fn restore_ledger(&self, project: &mut ModProject) {
        match &self.asset_before {
            Some(before) => {
                match project.assets.iter_mut().find(|asset| asset.path == self.relative) {
                    Some(entry) => *entry = before.clone(),
                    None => project.assets.push(before.clone()),
                }
            }
            None => project.assets.retain(|asset| asset.path != self.relative),
        }
    }

    /// 账本重放到导入后：与导入内核第 5 步同形（同路径只更新字节数，否则登记）。
    fn replay_ledger(&self, project: &mut ModProject) {
        let size = self.new_bytes.len() as i64;
        match project.assets.iter_mut().find(|asset| asset.path == self.relative) {
            Some(entry) => entry.size = size,
            None => project.assets.push(AssetRef::new(&self.relative, size, "sprite")),
        }
    }
}

/// 原子写一份字节到目标（与导入内核第 4 步同一份 mm-io AtomicFiles 口径）：
/// 先写同目录临时文件再换入，失败清掉临时文件——一次失败的回滚不留半截贴图。
fn write_sprite_bytes(target: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(CommandError::Io)?;
    }
    let temp = mm_io::files::temp_for(target);
    let landed = std::fs::write(&temp, bytes)
        .and_then(|()| mm_io::files::move_into_place(&temp, target));
    if let Err(err) = landed {
        mm_io::files::delete_quietly(&temp);
        return Err(CommandError::Io(err));
    }
    Ok(())
}

impl<'a> Command<EditState<'a>> for SpriteImportCommand {
    /// 恢复导入前快照：旧字节原子写回（首次导入则删除文件），再回滚账本。
    /// 文件已不在（外部删过）时删除视为达成目标状态，不报错；其余磁盘失败报 `Io`。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.require_record(state)?;
        let target = self.target(state)?;
        match &self.old_bytes {
            Some(bytes) => write_sprite_bytes(&target, bytes)?,
            None => match std::fs::remove_file(&target) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(CommandError::Io(err)),
            },
        }
        self.restore_ledger(state.project);
        Ok(())
    }

    /// 重放导入：新字节原子写回，再重放账本。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        self.require_record(state)?;
        let target = self.target(state)?;
        write_sprite_bytes(&target, &self.new_bytes)?;
        self.replay_ledger(state.project);
        Ok(())
    }
}

/// 对应 Java `ProjectController.Command`（`:96-98`）：一条可撤销的用户动作，Java 那边它只是
/// `UndoManager.Command` 的一个空子接口（好在类型上区分开「本控制器发布的命令」）。
///
/// Java 那边它的实现全是匿名类；「命令即数据」的建模下，这里用一个枚举把
/// `ProjectController` 里出现过的每一种匿名命令各列一个变体，每个变体的负载就是那个匿名类
/// 捕获的字段。`undo` / `redo` 逐一对应各 `:N-M` 里那两个方法。
pub enum AppCommand {
    /// 对应 `FieldCommand`（`:183-223`）。
    Field(FieldCommand),
    /// 对应 `StringCommand`（`:226-261`）。
    String(StringCommand),
    /// 对应 `addContent` 里的匿名命令（`:723-743`）。
    Add(AddCommand),
    /// 对应 `rename` 里的匿名命令（`:805-825`）。
    Rename(RenameCommand),
    /// 对应 `deleteContents` 里的匿名命令（`:952-1021`）。
    Delete(DeleteCommand),
    /// 对应 `connect` 里的匿名命令（`:1092-1110`）。
    Connect(ConnectCommand),
    /// 对应 `disconnect` 主分支里的匿名命令（`:1255-1277`）。
    Disconnect(DisconnectCommand),
    /// 对应 `disconnect` 孤儿边分支里的匿名命令（`:1224-1238`）。
    DetachEdge(DetachEdgeCommand),
    /// 对应 `commitMoves` 里的匿名命令（`:1351-1378`）。
    CommitMoves(CommitMovesCommand),
    /// 对应 `insertReroute` 里的匿名命令（`:1408-1424`）。
    InsertReroute(InsertRerouteCommand),
    /// 对应 `removeReroute` 里的匿名命令（`:1440-1456`）。
    RemoveReroute(RemoveRerouteCommand),
    /// 对应 `commitRerouteMove` 里的匿名命令（`:1478-1494`）。
    CommitRerouteMove(CommitRerouteMoveCommand),
    /// 对应 `addBoard` 里的匿名命令（`:1860-1877`）。
    AddBoard(AddBoardCommand),
    /// 对应 `tidyLayout` 里的匿名命令（`:1903-1915`）。
    TidyLayout(TidyLayoutCommand),
    /// 对应 `rebuildLinks` 里的匿名命令（`:1956-1970`）。
    RebuildLinks(RebuildLinksCommand),
    /// 对应 `setPatch` 里的匿名命令（`:1790-1804`）。
    Patch(PatchCommand),
    /// 对应 `applyRawTree` 里的匿名命令（`:1826-1840`）。
    RawTree(RawTreeCommand),
    /// 对应 `rebindCarrier` 里的匿名命令（`:1606-1620`）。
    RebindCarrier(RebindCarrierCommand),
    /// 对应 `detachNodes` 里的匿名命令（`:1661-1689`）。
    DetachNodes(DetachNodesCommand),
    /// 贴图导入（2026-11-25 新增，授权的新设计，无 Java 对应物）：见 [`SpriteImportCommand`]。
    SpriteImport(SpriteImportCommand),
}

impl<'a> Command<EditState<'a>> for AppCommand {
    /// 分派到各变体的 `undo()`。内存命令恒 `Ok`；贴图命令可失败（见 [`CommandError`]）。
    fn undo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        match self {
            AppCommand::Field(command) => command.undo(state),
            AppCommand::String(command) => command.undo(state),
            AppCommand::Add(command) => command.undo(state),
            AppCommand::Rename(command) => command.undo(state),
            AppCommand::Delete(command) => command.undo(state),
            AppCommand::Connect(command) => command.undo(state),
            AppCommand::Disconnect(command) => command.undo(state),
            AppCommand::DetachEdge(command) => command.undo(state),
            AppCommand::CommitMoves(command) => command.undo(state),
            AppCommand::InsertReroute(command) => command.undo(state),
            AppCommand::RemoveReroute(command) => command.undo(state),
            AppCommand::CommitRerouteMove(command) => command.undo(state),
            AppCommand::AddBoard(command) => command.undo(state),
            AppCommand::TidyLayout(command) => command.undo(state),
            AppCommand::RebuildLinks(command) => command.undo(state),
            AppCommand::Patch(command) => command.undo(state),
            AppCommand::RawTree(command) => command.undo(state),
            AppCommand::RebindCarrier(command) => command.undo(state),
            AppCommand::DetachNodes(command) => command.undo(state),
            AppCommand::SpriteImport(command) => command.undo(state),
        }
    }

    /// 分派到各变体的 `redo()`。
    fn redo(&mut self, state: &mut EditState<'a>) -> Result<(), CommandError> {
        match self {
            AppCommand::Field(command) => command.redo(state),
            AppCommand::String(command) => command.redo(state),
            AppCommand::Add(command) => command.redo(state),
            AppCommand::Rename(command) => command.redo(state),
            AppCommand::Delete(command) => command.redo(state),
            AppCommand::Connect(command) => command.redo(state),
            AppCommand::Disconnect(command) => command.redo(state),
            AppCommand::DetachEdge(command) => command.redo(state),
            AppCommand::CommitMoves(command) => command.redo(state),
            AppCommand::InsertReroute(command) => command.redo(state),
            AppCommand::RemoveReroute(command) => command.redo(state),
            AppCommand::CommitRerouteMove(command) => command.redo(state),
            AppCommand::AddBoard(command) => command.redo(state),
            AppCommand::TidyLayout(command) => command.redo(state),
            AppCommand::RebuildLinks(command) => command.redo(state),
            AppCommand::Patch(command) => command.redo(state),
            AppCommand::RawTree(command) => command.redo(state),
            AppCommand::RebindCarrier(command) => command.redo(state),
            AppCommand::DetachNodes(command) => command.redo(state),
            AppCommand::SpriteImport(command) => command.redo(state),
        }
    }
}
