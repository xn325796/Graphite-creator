//! `ModImporter.java` 的等价移植：把 [`ModPackage`]（模组在磁盘上的原始样子）变成可编辑的
//! [`ModProject`]。
//!
//! 对应 Java `dev.modmaker.core.io.ModImporter`。内容记录保留原始文本、游戏只能以内联对象表达的
//! 嵌套内容（bullet）独立成节点、bundle 拆成字符串表加未识别的额外键、其余文件登记为资源、导入的
//! 内容排到一块画板上并推出引用连线。
//!
//! 什么都不丢：无法识别的文件、未知字段、自定义 bundle 键全部存活，这正是「未改动的导入可以逐字节
//! 导出」的前提。
//!
//! 与 Java 的**必要偏离**（除第一条外均不改变可观察行为）：
//! - Java `importPackage(ModPackage pkg)` 收的是引用，`importStrings` 会把「同一语言槽的第二个
//!   bundle」从 `pkg.bundleFiles` 移进 `pkg.assetFiles`，调用方（`ProjectActions.readImport` 随后
//!   把同一个 pkg 交给 `ProjectIo.unpack`）看得见这个改动。Rust 的 `ModPackage` 是拥有型值，故签名
//!   显式改成 `&mut ModPackage`。
//! - Java 的 `Result` record 与 `std::result::Result` 撞名，这里叫 [`ImportedProject`]。
//! - Java `ImportProgress.checkCancelled()` 抛不受检异常绕过所有签名；Rust 没有对应物，取消经
//!   `Result<_, ImportError>` 显式传播（见 [`crate::import::ImportCancelled`] 的说明）。
//! - `project.meta = pkg.meta` 在 Java 是引用赋值；[`mm_model::ModMeta`] 不是 `Copy`，这里 `clone`。
//!   等价前提已核对：Java 侧导入后没有任何地方再写 `pkg.meta`（只有 `ModPackageReader` 在导入前赋值，
//!   `ProjectIo.unpack` 只读 `pkg.metaFile`/`pkg.metaText`），故共享与克隆无可观察差异。
//! - `buildBoard` 分成两次借用（见该方法注释），因为 Rust 不能同时持有 `&ModProject` 和
//!   `project.boards` 里某个 `Board` 的 `&mut`。

use std::sync::Arc;

use mm_fmt::codec::{locale_of, parse_key, PropertiesFile};
use mm_fmt::ids::uid;
use mm_json::{child, child_index, leaf_name, Json, Object};
use mm_model::{AssetRef, Board, ContentRecord, ModProject};
use mm_schema::{name_rules, ContentType, KeyedMap, SchemaRegistry};

use crate::canvas::{auto_layout, link_graph};
use crate::import::{ImportError, ImportProgress, ImportReport, NoProgress, Stage};
use crate::package::mod_package::{ModPackage, SourceContent};

/// Java `ModImporter.BUILD_STAGES`：`import_package_with` 依次跑完的具名趟数。
const BUILD_STAGES: i32 = 5;

/// Java `ModImporter.Result`：导入产物（项目 + 报告）。
#[derive(Debug)]
pub struct ImportedProject {
    /// Java `Result.project()`。
    pub project: ModProject,
    /// Java `Result.report()`。
    pub report: ImportReport,
}

/// Java `ModImporter`：把一个原始包变成一个可编辑项目。
pub struct ModImporter<'a> {
    registry: &'a SchemaRegistry,
}

/// Java `buildingTick`：构建的一趟跑完了——先响应取消，再宣告这一趟。
///
/// 顺序照搬 Java：`check_cancelled()` 在前、`progressed()` 在后，颠倒会让「取消发生在宣告之后」
/// 这种可观察差异出现。
fn building_tick(progress: &mut dyn ImportProgress, pass: i32) -> Result<(), ImportError> {
    progress.check_cancelled()?;
    progress.progressed(Stage::Building, pass, BUILD_STAGES);
    Ok(())
}

/// Java `AbstractCollection.toString()` 的确切形态（`[a, b]`，空集合 `[]`）。
///
/// mm-schema 的 `accepted_folders()` 返回 `Vec<String>`，Rust 既没有 `Display` 实现、`{:?}` 也会
/// 给每个元素加引号，与 Java 打印 `Set<String>` 的样子都不一致，故告警文案一律经此还原。
fn java_collection_to_string(items: &[String]) -> String {
    let joined = items
        .iter()
        .map(|item| item.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    format!("[{joined}]")
}

impl<'a> ModImporter<'a> {
    /// Java `new ModImporter(SchemaRegistry registry)`。
    pub fn new(registry: &'a SchemaRegistry) -> Self {
        Self { registry }
    }

    /// Java `importPackage(ModPackage pkg)`：无进度回调（`ImportProgress.NONE` → [`NoProgress`]）。
    pub fn import_package(&self, pkg: &mut ModPackage) -> Result<ImportedProject, ImportError> {
        self.import_package_with(pkg, &mut NoProgress)
    }

    /// Java `importPackage(ModPackage pkg, ImportProgress progress)`。
    pub fn import_package_with(
        &self,
        pkg: &mut ModPackage,
        progress: &mut dyn ImportProgress,
    ) -> Result<ImportedProject, ImportError> {
        let mut project = ModProject::default();
        let mut report = ImportReport { source: pkg.source_label.clone(), ..Default::default() };
        // Java 是引用赋值：`project.meta = pkg.meta`。`ModMeta` 非 `Copy`，故 clone；导入完成后
        // Java 侧再无人写 `pkg.meta`，两者无可观察差异（见模块头的说明）。
        project.meta = pkg.meta.clone();
        // 承接元数据来源，使一个未改动的项目在保存时重写原始字节（连注释一起），
        // 而不是重写一份解析后的元数据。
        project.meta_file = pkg.meta_file.clone();
        project.meta_text = pkg.meta_text.clone();
        project.meta_dirty = false;
        project.meta_parse_failed = !pkg.meta_error.is_empty();
        project.imported_from = pkg.source_label.clone();

        let internal = project.internal_name();
        if pkg.meta_file.is_empty() {
            report.warn("no mod.json / mod.hjson found - metadata starts empty");
        } else if !pkg.meta_error.is_empty() {
            report.warn(&format!(
                "{} could not be parsed and was kept verbatim: {}",
                pkg.meta_file, pkg.meta_error
            ));
        } else if internal.is_empty() {
            report.warn(&format!(
                "mod name is empty in {} - content prefixes cannot be resolved",
                pkg.meta_file
            ));
        }
        if pkg.from_zip && !pkg.root_prefix.is_empty() {
            report.warn(&format!(
                "zip contained a single wrapping folder '{}' - unwrapped the same way the game does",
                pkg.root_prefix
            ));
        }

        progress.stage_started(Stage::Building, BUILD_STAGES);
        self.import_contents(pkg, &mut project, &mut report, &internal);
        building_tick(progress, 1)?;
        self.import_inline(&mut project, &mut report);
        building_tick(progress, 2)?;
        // 确定性顺序让同一份包的排版、授权文件表和 diff 在各次导入之间保持稳定，
        // 不受文件系统遍历顺序影响。
        project.contents.sort_by(ContentRecord::deterministic_cmp);
        Self::import_strings(pkg, &mut project, &mut report);
        building_tick(progress, 3)?;
        Self::import_assets(pkg, &mut project, &mut report);
        building_tick(progress, 4)?;
        self.build_board(&mut project, &mut report);
        building_tick(progress, 5)?;

        report.skipped.extend(pkg.skipped.iter().cloned());
        Ok(ImportedProject { project, report })
    }

    // --- content --------------------------------------------------------------------------------

    /// Java `importContents`。
    ///
    /// Java 形参 `internal` 在本方法体内未被使用（只由 `importPackage` 用于空名字告警），
    /// 这里以下划线前缀保留同样的形状。
    fn import_contents(
        &self,
        pkg: &ModPackage,
        project: &mut ModProject,
        report: &mut ImportReport,
        _internal: &str,
    ) {
        for source in &pkg.contents {
            let mut record = ContentRecord {
                id: Some(uid("c")),
                ctype: source.ctype,
                name: source.name.clone(),
                ext: source.ext.clone(),
                raw: Some(source.text.clone()),
                raw_path: Some(source.path.clone()),
                ..Default::default()
            };
            for (key, value) in source.fields.iter() {
                record.fields.insert(key.clone(), value.clone());
            }

            if let Some(error) = &source.parse_error {
                record.parse_failed = true;
                report
                    .parse_failures
                    .put(source.path.clone(), error.clone());
                report.warn(&format!(
                    "could not parse {} (kept verbatim): {}",
                    source.path, error
                ));
            }
            if self.is_patch(source, report) {
                record.patch = true;
                report.patch_count += 1;
            }

            if let Some(folder) = Self::folder_of(&source.path) {
                let accepted = source.ctype.accepted_folders();
                if !accepted.contains(&folder) {
                    report.warn(&format!(
                        "content folder '{}' is not the name the game looks for {}; it will not load on a case-sensitive filesystem",
                        folder,
                        java_collection_to_string(&accepted)
                    ));
                }
            }
            if !name_rules::is_valid_content_name(&record.name) {
                // Java 里 `contentNameProblem` 可能返回 null，字符串拼接会打印 "null"；
                // 前置条件（名字非法）保证这里恒非 null，`unwrap_or("null")` 只为还原 Java 的打印形态。
                let problem = name_rules::content_name_problem(&record.name)
                    .unwrap_or_else(|| "null".to_string());
                report.warn(&format!("{}: {}", record.name, problem));
            }
            if record.class_name().is_none() && source.ctype.default_class().is_empty() {
                report.warn(&format!(
                    "{} declares no \"type\"; the game requires one for {}",
                    source.path, source.ctype
                ));
            }
            report.content_count += 1;
            project.contents.push(record);
        }
    }

    /// Java `isPatch`：复刻 ContentParser 的补丁识别——文件名命中已有内容时修改该内容而非新建。
    ///
    /// 这里只能知道原版名：另一个模组的内容需要那个模组被加载。方块是特例：声明 `"type"`
    /// 会使其成为一个新方块，即便文件名冲突（ContentParser.java:583-592）。
    fn is_patch(&self, source: &SourceContent, report: &mut ImportReport) -> bool {
        if !self.registry.is_vanilla_name(source.ctype, &source.name) {
            return false;
        }
        if source.ctype == ContentType::Block && source.fields.contains("type") {
            report.warn(&format!(
                "content/blocks/{}.json declares a type over the vanilla block '{}'; the game treats it as a NEW block (omit \"type\" to patch it)",
                source.name, source.name
            ));
            return false;
        }
        true
    }

    /// Java `folderOf`：内容文件所在的那一级目录，取不到返回 `None`（Java 返回 `null`）。
    ///
    /// `indexOf('/') + 1` 的边界照搬：路径里没有 `/` 时 start 为 0，再找第二个 `/` 得 -1 返回 null。
    fn folder_of(content_path: &str) -> Option<String> {
        let start = content_path.find('/').map_or(0, |index| index + 1);
        let end = content_path[start..].find('/');
        end.map(|relative| content_path[start..start + relative].to_string())
    }

    // --- inline content -------------------------------------------------------------------------

    /// Java `importInline`：炮塔 `ammoTypes` 里定义的 bullet 不是一个文件——游戏没有
    /// `content/bullets` 解析器。这类对象成为指回宿主的独立记录，于是可以被当作节点编辑，
    /// 而宿主文件仍是它们被写回的唯一位置。
    ///
    /// Java 用 `new ArrayList<>(project.contents)` 取快照避免并发修改；Rust 里循环体只读
    /// `project.contents`、写入的是独立的 `inline`，收集完再追加，与「遍历快照」等价。
    fn import_inline(&self, project: &mut ModProject, report: &mut ImportReport) {
        let mut inline: Vec<ContentRecord> = Vec::new();
        for owner in &project.contents {
            if owner.is_inline() || owner.fields.is_empty() {
                continue;
            }
            // Java 把宿主字段灌进一个 `JsonObject` 视图再遍历 `entrySet`；Rust 的
            // `ContentRecord.fields` 本身就是有序的 `Object`，视图是恒等操作，直接遍历。
            for (key, value) in owner.fields.iter() {
                self.walk_inline(owner, Some(value), key, &mut inline, &mut *report, 0);
            }
        }
        let count = inline.len() as i32;
        project.contents.extend(inline);
        report.inline_count = count;
    }

    /// Java `walkInline`：`path` 是内联节点在宿主文件里的位置；`node` 为 `None` 对应 Java 的
    /// `null`（同样直接返回）。Java 形参 `report` 在方法体内未被使用，只是沿递归传递，
    /// 这里以下划线前缀保留形状。
    fn walk_inline(
        &self,
        owner: &ContentRecord,
        node: Option<&Json>,
        path: &str,
        out: &mut Vec<ContentRecord>,
        _report: &mut ImportReport,
        depth: i32,
    ) {
        let Some(node) = node else {
            return;
        };
        if node.is_null() || depth > 6 {
            return;
        }
        if let Json::Obj(object) = node {
            let inline_type = self.inline_type_of(object);
            if let Some(inline_type) = inline_type {
                let mut record = ContentRecord {
                    id: Some(uid("i")),
                    ctype: inline_type,
                    name: leaf_name(path),
                    inline_owner_id: owner.id.clone(),
                    inline_path: Some(path.to_string()),
                    ..Default::default()
                };
                for (key, value) in object.iter() {
                    record.fields.insert(key.clone(), value.clone());
                }
                record.dirty = false;
                out.push(record);
                return;
            }
            for (key, value) in object.iter() {
                self.walk_inline(
                    owner,
                    Some(value),
                    &child(path, key),
                    out,
                    _report,
                    depth + 1,
                );
            }
        } else if let Json::Arr(array) = node {
            for (index, element) in array.iter().enumerate() {
                self.walk_inline(
                    owner,
                    Some(element),
                    &child_index(path, index),
                    out,
                    _report,
                    depth + 1,
                );
            }
        }
    }

    /// Java `inlineTypeOf`：嵌套对象的 `"type"` 值对应的内容类型，前提是该类型没有文件解析器。
    fn inline_type_of(&self, object: &Object) -> Option<ContentType> {
        let type_field = object.get("type")?;
        // Gson 的 `isJsonPrimitive()` 只对字符串/数字/布尔为真（`null` 为假）。
        if !matches!(type_field, Json::Str(_) | Json::Num(_) | Json::Bool(_)) {
            return None;
        }
        let class_name = type_field.get_as_string();
        let class_name = match class_name.rfind('.') {
            Some(dot) => class_name[dot + 1..].to_string(),
            None => class_name,
        };
        let schema = self.registry.schema(&class_name)?;
        let ctype = schema.ctype()?;
        if ctype.file_backed() {
            return None;
        }
        Some(ctype)
    }

    // --- strings --------------------------------------------------------------------------------

    /// Java `importStrings`（Java 是实例方法，但不读实例状态，这里收成关联函数以免 `unused_self`）。
    fn import_strings(pkg: &mut ModPackage, project: &mut ModProject, report: &mut ImportReport) {
        // Java `LinkedHashSet`：去重且保插入序。
        let mut locales: Vec<String> = Vec::new();
        // 本工具把基础 bundle 记作 "en"（对它而言 bundle.properties 与 bundle_en.properties
        // 都是回退语言），于是两个文件可能解析到同一个语言槽。按语言排序并偏好基础文件，
        // 使哪个文件胜出变得确定——Files.walk 的顺序并不确定——而落败者在下面被原样保留为资源，
        // 而不是被丢弃。
        let mut bundles: Vec<(String, String)> = pkg
            .bundle_files
            .iter()
            .map(|(path, text)| (path.clone(), text.clone()))
            .collect();
        bundles.sort_by(|a, b| {
            // 第一档照搬 Java：`localeSlot` 对空语言返回 "en"，故这一档实际上永不命中，
            // 但判定顺序保持原样。
            let rank = |path: &str| if Self::locale_slot(path).is_empty() { 0 } else { 1 };
            rank(&a.0)
                .cmp(&rank(&b.0))
                .then_with(|| Self::locale_slot(&a.0).cmp(&Self::locale_slot(&b.0)))
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut claimed: Vec<String> = Vec::new();
        let mut duplicates: Vec<String> = Vec::new();
        for (path, text) in &bundles {
            let file_name = &path[path.rfind('/').map_or(0, |index| index + 1)..];
            let locale = Self::locale_slot(path);
            if claimed.contains(&locale) {
                duplicates.push(path.clone());
                report.warn(&format!(
                    "{path} is a second bundle for the same locale as another file ({file_name} resolves to the same bundle); it is kept verbatim as an asset and cannot be edited in the string table"
                ));
                continue;
            }
            claimed.push(locale.clone());
            locales.push(locale.clone());
            project.strings.bundle_files.put(locale.clone(), file_name.to_string());

            let properties = PropertiesFile::parse(text);
            for problem in &properties.problems {
                report.warn(&format!("{path}: {problem}"));
            }
            for (key, value) in properties.values.iter() {
                if parse_key(key).is_some() {
                    project.strings.put(key, &locale, value);
                    report.string_count += 1;
                } else {
                    project
                        .strings
                        .extra
                        .entry_or_insert_with(locale.clone(), KeyedMap::new)
                        .put(key.clone(), value.clone());
                }
            }
            // 保留原始文本，使一个未改动的 bundle 被逐字节写回。
            project.strings.bundle_raw.put(locale.clone(), Arc::from(text.clone()));
            // Java 在此处再次 `bundleFiles.put(locale, fileName)`（与上面那次等价），照搬。
            project.strings.bundle_files.put(locale.clone(), file_name.to_string());
            project.strings.bundle_dirty.put(locale.clone(), false);
            report.bundle_count += 1;
        }
        // 不允许任何东西被静默丢弃：多出来的文件变成资源，于是它在原路径上逐字节撑过往返与构建。
        for path in duplicates {
            // Java 里这里会抛 NullPointerException；`duplicates` 全部来自 `bundle_files` 的键，
            // 且两趟之间无人改动该表，故必然存在。
            let text = pkg
                .bundle_files
                .remove(&path)
                .expect("duplicate 路径必然仍在 bundle_files 里");
            pkg.asset_files.put(path, text.as_bytes().to_vec());
        }
        let mut ordered: Vec<String> = vec!["en".to_string()];
        let mut rest: Vec<String> = locales.into_iter().filter(|l| l != "en").collect();
        rest.sort();
        ordered.extend(rest);
        project.locales = ordered;
    }

    /// Java `localeSlot`：bundle 文件在本工具字符串表里占用的语言槽。
    /// `bundle.properties` 是基础 bundle，游戏把它当回退，本工具称之为 "en"。
    fn locale_slot(path: &str) -> String {
        let file_name = &path[path.rfind('/').map_or(0, |index| index + 1)..];
        let locale = locale_of(file_name);
        if locale.is_empty() {
            "en".to_string()
        } else {
            locale
        }
    }

    // --- assets ---------------------------------------------------------------------------------

    /// Java `importAssets`（Java 是实例方法，但不读实例状态，这里收成关联函数）。
    fn import_assets(pkg: &ModPackage, project: &mut ModProject, report: &mut ImportReport) {
        for (path, bytes) in pkg.asset_files.iter() {
            let kind = asset_kind(path);
            project
                .assets
                .push(AssetRef::new(path, bytes.len() as i64, &kind));
            report.asset_count += 1;
            if kind.starts_with("sprite") {
                report.sprite_count += 1;
                // Mods.java:168-169 只打包扩展名恰好是 "png" 的文件——检查是大小写敏感的，
                // 所以 "Sprite.PNG" 会被静默地不打包。先在本机把路径转小写会让这条告警永不为真，
                // 因此这里必须用大小写敏感的 `ends_with`。
                if !path.ends_with(".png") {
                    report.warn(&format!(
                        "{path} is in a sprite folder but its extension is not lowercase \".png\" - the game will not pack it"
                    ));
                }
            }
        }
    }

    // --- board ----------------------------------------------------------------------------------

    /// Java `buildBoard`。
    ///
    /// Java 先把画板放进 `project.boards` 再 `AutoLayout.apply` / `LinkGraph.derive`（同一实例）；
    /// Rust 不能同时借出 `&ModProject` 与其中一块画板的 `&mut Board`，而 `apply`/`derive` 都只读
    /// `project.contents`（不读 `project.boards`），故两阶段：先在画板外完成节点与连线，再入表，
    /// 结果与 Java 一致。
    fn build_board(&self, project: &mut ModProject, report: &mut ImportReport) {
        let mut board = Board {
            id: Some(uid("b")),
            name: "Main".to_string(),
            file: "main.canvas".to_string(),
            ..Default::default()
        };

        auto_layout::apply(project, &mut board);
        // 引用连线来自画板「重建连线」动作用的同一段代码，于是导入与重建对「一条连线是什么、
        // 指向哪个方向」永不会分歧。
        let edges = link_graph::derive(project, &board, self.registry);
        report.edge_count = edges.len() as i32;
        board.edges.extend(edges);
        project.boards.push(board);
    }
}

/// Java `ModImporter.assetKind`（包内可见静态方法）。
pub(crate) fn asset_kind(path: &str) -> String {
    if path.starts_with("sprites/") {
        return "sprite".to_string();
    }
    if path.starts_with("sprites-override/") {
        return "spriteOverride".to_string();
    }
    if path.starts_with("scripts/") {
        return "script".to_string();
    }
    if path.starts_with("sounds/") {
        return "sound".to_string();
    }
    if path.starts_with("music/") {
        return "music".to_string();
    }
    if path.starts_with("maps/") {
        return "map".to_string();
    }
    if path.starts_with("schematics/") {
        return "schematic".to_string();
    }
    if path == "icon.png" || path == "preview.png" {
        return "icon".to_string();
    }
    "extra".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportCancelled;
    use std::cell::{Cell, RefCell};
    use std::path::Path;

    /// 按 `tests/link_graph.rs` 的方式从真实 schema 目录装载注册表；目录缺失时返回 `None`。
    fn load_registry() -> Option<SchemaRegistry> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Mindustry-modmaker/schemas");
        if !dir.join("fields.json").exists() {
            return None;
        }
        SchemaRegistry::load(&dir).ok()
    }

    fn json_object(pairs: &[(&str, Json)]) -> Object {
        let mut object = Object::new();
        for (key, value) in pairs {
            object.insert((*key).to_string(), value.clone());
        }
        object
    }

    fn source(path: &str, name: &str, ctype: ContentType, fields: Object) -> SourceContent {
        SourceContent {
            path: path.to_string(),
            name: name.to_string(),
            ext: "json".to_string(),
            text: "{}".to_string(),
            ctype,
            fields,
            parse_error: None,
        }
    }

    fn package() -> ModPackage {
        let mut pkg = ModPackage::default();
        pkg.meta.name = "Test Mod".to_string();
        pkg.meta_file = "mod.json".to_string();
        pkg.meta_text = "{\"name\":\"Test Mod\"}".to_string();
        pkg.source_label = "test.jar".to_string();
        pkg
    }

    /// 记录 progress 回调轨迹，并可按第 N 次 `check_cancelled` 触发取消。
    /// `check_cancelled` 只借 `&self`，轨迹与计数用 `RefCell`/`Cell` 承接。
    #[derive(Default)]
    struct Recorder {
        trace: RefCell<Vec<String>>,
        checks: Cell<usize>,
        cancel_after: Cell<Option<usize>>,
    }

    impl Recorder {
        fn events(&self) -> Vec<String> {
            self.trace.borrow().clone()
        }
    }

    impl ImportProgress for Recorder {
        fn stage_started(&mut self, stage: Stage, total: i32) {
            self.trace
                .borrow_mut()
                .push(format!("stage_started({:?},{})", stage, total));
        }

        fn progressed(&mut self, stage: Stage, done: i32, total: i32) {
            self.trace
                .borrow_mut()
                .push(format!("progressed({:?},{},{})", stage, done, total));
        }

        fn is_cancelled(&self) -> bool {
            match self.cancel_after.get() {
                Some(limit) => self.checks.get() >= limit,
                None => false,
            }
        }

        fn check_cancelled(&self) -> Result<(), ImportCancelled> {
            self.checks.set(self.checks.get() + 1);
            self.trace.borrow_mut().push("check".to_string());
            if self.is_cancelled() {
                Err(ImportCancelled)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn meta_is_carried_and_flags_are_set() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        pkg.meta.description = "keeps provenance".to_string();
        pkg.meta_text = "{ \"name\": \"Test Mod\" }".to_string();
        let importer = ModImporter::new(&registry);
        let imported = importer.import_package(&mut pkg).expect("import ok");
        assert_eq!(imported.project.meta.name, "Test Mod");
        assert_eq!(imported.project.meta.description, "keeps provenance");
        assert_eq!(imported.project.meta_file, "mod.json");
        assert_eq!(imported.project.meta_text, "{ \"name\": \"Test Mod\" }");
        assert!(!imported.project.meta_dirty);
        assert!(!imported.project.meta_parse_failed);
        assert_eq!(imported.project.imported_from, "test.jar");
        assert_eq!(imported.report.source, "test.jar");
        assert!(imported.report.warnings.is_empty());
        // 没有任何 bundle 时，语言表仍然只有 Java 固定在第一位的回退槽。
        assert_eq!(imported.project.locales, vec!["en".to_string()]);

        pkg.meta_error = "boom".to_string();
        let imported = importer.import_package(&mut pkg).expect("import ok");
        assert!(imported.project.meta_parse_failed);
        assert!(!imported.project.meta_dirty);
        assert_eq!(
            imported.report.warnings,
            vec!["mod.json could not be parsed and was kept verbatim: boom"]
        );
    }

    #[test]
    fn the_three_early_meta_warnings_are_mutually_exclusive() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let importer = ModImporter::new(&registry);

        let mut pkg = package();
        pkg.meta_file.clear();
        pkg.meta_text.clear();
        let imported = importer.import_package(&mut pkg).expect("import ok");
        assert_eq!(
            imported.report.warnings,
            vec!["no mod.json / mod.hjson found - metadata starts empty"]
        );

        let mut pkg = package();
        pkg.meta_error = "unexpected token".to_string();
        pkg.meta.name = String::new();
        let imported = importer.import_package(&mut pkg).expect("import ok");
        assert_eq!(
            imported.report.warnings,
            vec!["mod.json could not be parsed and was kept verbatim: unexpected token"]
        );

        let mut pkg = package();
        pkg.meta.name.clear();
        let imported = importer.import_package(&mut pkg).expect("import ok");
        assert_eq!(
            imported.report.warnings,
            vec!["mod name is empty in mod.json - content prefixes cannot be resolved"]
        );

        let mut pkg = package();
        pkg.from_zip = true;
        pkg.root_prefix = "my-mod/".to_string();
        let imported = importer.import_package(&mut pkg).expect("import ok");
        assert_eq!(
            imported.report.warnings,
            vec![
                "zip contained a single wrapping folder 'my-mod/' - unwrapped the same way the game does"
            ]
        );
    }

    #[test]
    fn folder_of_mirrors_the_java_index_boundaries() {
        assert_eq!(ModImporter::folder_of("content/blocks/router.json").as_deref(), Some("blocks"));
        assert_eq!(ModImporter::folder_of("blocks/router.json"), None);
        assert_eq!(ModImporter::folder_of("mod.json"), None);
        assert_eq!(ModImporter::folder_of("/"), None);
        assert_eq!(ModImporter::folder_of("a//b"), Some("".to_string()));
        assert_eq!(ModImporter::folder_of("a/b/c/d"), Some("b".to_string()));
    }

    #[test]
    fn asset_kind_covers_every_branch() {
        assert_eq!(asset_kind("sprites/x.png"), "sprite");
        assert_eq!(asset_kind("sprites-override/x.png"), "spriteOverride");
        assert_eq!(asset_kind("scripts/main.ts"), "script");
        assert_eq!(asset_kind("sounds/hit.ogg"), "sound");
        assert_eq!(asset_kind("music/theme.ogg"), "music");
        assert_eq!(asset_kind("maps/turf.mschem"), "map");
        assert_eq!(asset_kind("schematics/base.sch"), "schematic");
        assert_eq!(asset_kind("icon.png"), "icon");
        assert_eq!(asset_kind("preview.png"), "icon");
        assert_eq!(asset_kind("misc/logo.png"), "extra");
        assert_eq!(asset_kind("sprites"), "extra");
        assert_eq!(asset_kind("Sprites/x.png"), "extra");
    }

    #[test]
    fn sprite_assets_count_and_warn_on_mixed_case_extension() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        pkg.asset_files.put("sprites/a.png", vec![0u8; 3]);
        pkg.asset_files.put("sprites-override/B.PNG", vec![0u8; 5]);
        pkg.asset_files.put("sounds/hit.ogg", vec![0u8; 7]);
        let imported = ModImporter::new(&registry)
            .import_package(&mut pkg)
            .expect("import ok");
        assert_eq!(imported.report.asset_count, 3);
        assert_eq!(imported.report.sprite_count, 2);
        assert_eq!(
            imported.report.warnings,
            vec![
                "sprites-override/B.PNG is in a sprite folder but its extension is not lowercase \".png\" - the game will not pack it"
            ]
        );
        let kinds: Vec<String> = imported.project.assets.iter().map(|a| a.kind.clone()).collect();
        assert_eq!(kinds, vec!["sprite", "spriteOverride", "sound"]);
        let sizes: Vec<i64> = imported.project.assets.iter().map(|a| a.size).collect();
        assert_eq!(sizes, vec![3, 5, 7]);
    }

    #[test]
    fn content_warnings_reproduce_the_java_strings() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        let mut misparsed = source("content/items/good.json", "good", ContentType::Item, Object::new());
        misparsed.parse_error = Some("line 3: expected ,".to_string());
        pkg.contents.push(misparsed);
        pkg.contents.push(source(
            "content/tiles/wrong-folder.json",
            "wrong-folder",
            ContentType::Item,
            Object::new(),
        ));
        pkg.contents.push(source(
            "content/items/bad name.json",
            "bad name",
            ContentType::Item,
            Object::new(),
        ));
        pkg.contents.push(source(
            "content/weathers/monsoon.json",
            "monsoon",
            ContentType::Weather,
            Object::new(),
        ));
        pkg.skipped.push("data/deep/nested/secret.bin".to_string());

        let imported = ModImporter::new(&registry)
            .import_package(&mut pkg)
            .expect("import ok");
        let warnings = &imported.report.warnings;
        assert!(warnings.contains(&"could not parse content/items/good.json (kept verbatim): line 3: expected ,".to_string()));
        assert!(warnings.contains(&"content folder 'tiles' is not the name the game looks for [items]; it will not load on a case-sensitive filesystem".to_string()));
        assert!(warnings.contains(&"bad name: name contains ' ', which breaks content lookup on disk".to_string()));
        assert!(warnings.contains(&"content/weathers/monsoon.json declares no \"type\"; the game requires one for weather".to_string()));
        assert_eq!(imported.report.parse_failures.len(), 1);
        assert_eq!(
            imported.report.parse_failures.get("content/items/good.json"),
            Some(&"line 3: expected ,".to_string())
        );
        assert_eq!(imported.report.content_count, 4);
        assert!(imported.project.contents.iter().any(|r| r.parse_failed));
        // 收尾才并入的跳过项：顺序在五次 tick 之后，内容与告警都已就位。
        assert_eq!(imported.report.skipped, vec!["data/deep/nested/secret.bin".to_string()]);
    }

    #[test]
    fn vanilla_names_become_patches_unless_a_block_declares_type() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        pkg.contents.push(source(
            "content/blocks/router.json",
            "router",
            ContentType::Block,
            json_object(&[("health", Json::Num("120".to_string()))]),
        ));
        pkg.contents.push(source(
            "content/blocks/duo.json",
            "duo",
            ContentType::Block,
            json_object(&[("type", Json::Str("Wall".to_string()))]),
        ));
        pkg.contents.push(source(
            "content/blocks/mine.json",
            "mine",
            ContentType::Block,
            Object::new(),
        ));

        let imported = ModImporter::new(&registry)
            .import_package(&mut pkg)
            .expect("import ok");
        assert_eq!(imported.report.patch_count, 1);
        let router = imported
            .project
            .contents
            .iter()
            .find(|r| r.name == "router")
            .expect("router");
        let duo = imported
            .project
            .contents
            .iter()
            .find(|r| r.name == "duo")
            .expect("duo");
        let mine = imported
            .project
            .contents
            .iter()
            .find(|r| r.name == "mine")
            .expect("mine");
        assert!(router.patch);
        assert!(!duo.patch);
        assert!(!mine.patch);
        assert_eq!(
            imported.report.warnings,
            vec!["content/blocks/duo.json declares a type over the vanilla block 'duo'; the game treats it as a NEW block (omit \"type\" to patch it)"]
        );
    }

    #[test]
    fn inline_bullets_become_their_own_records_without_touching_the_host() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let bullet = |extra: Vec<(&str, Json)>| {
            let mut pairs: Vec<(&str, Json)> = vec![("type", Json::Str("BulletType".to_string()))];
            pairs.extend(extra);
            Json::Obj(json_object(&pairs))
        };
        let mut pkg = package();
        pkg.contents.push(source(
            "content/units/dagger.json",
            "dagger",
            ContentType::Unit,
            json_object(&[(
                "ammoTypes",
                Json::Arr(vec![
                    bullet(vec![("damage", Json::Num("9".to_string())), ("collider", bullet(vec![]))]),
                    bullet(vec![("speed", Json::Num("2.5".to_string()))]),
                ]),
            )]),
        ));

        let imported = ModImporter::new(&registry)
            .import_package(&mut pkg)
            .expect("import ok");
        assert_eq!(imported.report.inline_count, 2);
        let host = imported
            .project
            .contents
            .iter()
            .find(|r| r.name == "dagger")
            .expect("dagger host");
        let inline: Vec<&ContentRecord> = imported
            .project
            .contents
            .iter()
            .filter(|r| r.is_inline())
            .collect();
        assert_eq!(inline.len(), 2);
        assert_eq!(inline[0].ctype, ContentType::Bullet);
        assert_eq!(inline[0].name, "ammoTypes");
        assert_eq!(inline[0].inline_path.as_deref(), Some("ammoTypes[0]"));
        assert_eq!(inline[1].inline_path.as_deref(), Some("ammoTypes[1]"));
        for record in &inline {
            assert_eq!(record.inline_owner_id, host.id);
            assert!(!record.dirty);
            assert!(record.id.as_deref().expect("id").starts_with('i'));
        }
        // 第一个 bullet 的字段被整份拷贝，包括它内嵌的那个 bullet 对象。
        assert_eq!(
            inline[0].fields.get("damage"),
            Some(&Json::Num("9".to_string()))
        );
        assert!(
            matches!(inline[0].fields.get("collider"), Some(Json::Obj(_))),
            "宿主对象里的嵌套对象要原样留在内联记录的字段表里"
        );
        // 命中内联类型即返回，不再深入：内层那个 bullet 不会第三次成为记录。
        assert_eq!(inline[0].fields.len(), 3);
        assert_eq!(inline[1].fields.len(), 2);
        // 宿主字段不被改动：ammoTypes 仍是那个数组，且宿主自己没有 inline 标记。
        assert!(!host.is_inline());
        assert_eq!(host.fields.len(), 1);
        match host.fields.get("ammoTypes") {
            Some(Json::Arr(items)) => assert_eq!(items.len(), 2),
            other => panic!("host field changed: {other:?}"),
        }
        // 排序趟把文件记录排在前面、内联记录按 inlinePath 排在后。
        assert_eq!(imported.project.contents[0].name, "dagger");
        assert!(imported.project.contents[1].is_inline());
        assert!(imported.project.contents[2].is_inline());
    }

    #[test]
    fn a_second_bundle_for_one_locale_survives_as_an_asset() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        pkg.bundle_files.put(
            "locale/bundle.properties",
            "block.router.name=Router\nstat.ignore=1\ncategory.tools=Tools\n".to_string(),
        );
        pkg.bundle_files.put(
            "locale/bundle_en.properties",
            "block.duo.name=Duo\n".to_string(),
        );

        let assets_before = pkg.asset_files.len();
        let imported = ModImporter::new(&registry)
            .import_package(&mut pkg)
            .expect("import ok");
        assert_eq!(imported.report.bundle_count, 1);
        assert_eq!(imported.report.string_count, 1);
        assert_eq!(imported.project.strings.bundle_raw.len(), 1);
        assert_eq!(
            imported.project.strings.bundle_raw.get("en").map(|raw| raw.as_ref()),
            Some("block.router.name=Router\nstat.ignore=1\ncategory.tools=Tools\n")
        );
        assert_eq!(
            imported.project.strings.bundle_files.get("en").map(String::as_str),
            Some("bundle.properties")
        );
        assert_eq!(imported.project.strings.get("block.router.name", "en"), Some("Router"));
        // 不受管的键一个都不丢：留在 extra 里。
        let extra = imported
            .project
            .strings
            .extra
            .get("en")
            .expect("extra keys kept");
        assert_eq!(
            extra.get("category.tools"),
            Some(&"Tools".to_string())
        );
        // 落败的 bundle 变成资源，并且从 bundle_files 里被移走（调用方看得见）。
        assert!(!pkg.bundle_files.contains_key("locale/bundle_en.properties"));
        assert_eq!(pkg.bundle_files.len(), 1);
        let bytes = pkg
            .asset_files
            .get("locale/bundle_en.properties")
            .expect("duplicate kept verbatim");
        assert_eq!(bytes, &"block.duo.name=Duo\n".as_bytes().to_vec());
        assert_eq!(imported.report.asset_count, 1);
        assert_eq!(imported.project.assets[0].kind, "extra");
        assert_eq!(imported.report.sprite_count, 0);
        assert_eq!(pkg.asset_files.len(), assets_before + 1);
        assert_eq!(
            imported.report.warnings,
            vec!["locale/bundle_en.properties is a second bundle for the same locale as another file (bundle_en.properties resolves to the same bundle); it is kept verbatim as an asset and cannot be edited in the string table"]
        );
        assert_eq!(imported.project.locales, vec!["en".to_string()]);
    }

    #[test]
    fn locales_put_en_first_and_sort_the_rest() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        pkg.bundle_files.put("locale/bundle_zh_CN.properties", "block.a.name=A\n".to_string());
        pkg.bundle_files.put("locale/bundle_af_ZA.properties", "block.a.name=a\n".to_string());
        pkg.bundle_files.put("locale/bundle.properties", "block.a.name=ah\n".to_string());

        let imported = ModImporter::new(&registry)
            .import_package(&mut pkg)
            .expect("import ok");
        assert_eq!(imported.report.bundle_count, 3);
        assert_eq!(imported.report.warnings, Vec::<String>::new());
        assert_eq!(
            imported.project.locales,
            vec!["en".to_string(), "af_ZA".to_string(), "zh_CN".to_string()]
        );
        assert_eq!(imported.project.strings.bundle_files.len(), 3);
        assert_eq!(
            imported.project.strings.bundle_files.get("af_ZA").map(String::as_str),
            Some("bundle_af_ZA.properties")
        );
        // 三个语言槽各一条受管键。
        assert_eq!(imported.report.string_count, 3);
    }

    #[test]
    fn building_ticks_check_cancel_before_announcing_the_pass() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        pkg.skipped.push("data/deep/thing.bin".to_string());
        let mut recorder = Recorder::default();
        let imported = ModImporter::new(&registry)
            .import_package_with(&mut pkg, &mut recorder)
            .expect("import ok");
        assert_eq!(
            recorder.events(),
            vec![
                "stage_started(Building,5)".to_string(),
                "check".to_string(),
                "progressed(Building,1,5)".to_string(),
                "check".to_string(),
                "progressed(Building,2,5)".to_string(),
                "check".to_string(),
                "progressed(Building,3,5)".to_string(),
                "check".to_string(),
                "progressed(Building,4,5)".to_string(),
                "check".to_string(),
                "progressed(Building,5,5)".to_string(),
            ]
        );
        assert_eq!(imported.report.skipped, pkg.skipped);
        assert_eq!(imported.project.boards.len(), 1);
    }

    #[test]
    fn a_cancel_midway_returns_cancelled_and_stops_the_build() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        let mut recorder = Recorder::default();
        recorder.cancel_after.set(Some(2));
        let error = ModImporter::new(&registry)
            .import_package_with(&mut pkg, &mut recorder)
            .expect_err("second tick must cancel");
        assert!(matches!(error, ImportError::Cancelled(_)));
        assert_eq!(
            recorder.events(),
            vec![
                "stage_started(Building,5)".to_string(),
                "check".to_string(),
                "progressed(Building,1,5)".to_string(),
                "check".to_string(),
            ]
        );
    }

    #[test]
    fn the_board_holds_every_content_and_edges_match_the_report() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let mut pkg = package();
        pkg.contents.push(source(
            "content/items/steel.json",
            "steel",
            ContentType::Item,
            Object::new(),
        ));
        pkg.contents.push(source(
            "content/blocks/smelter.json",
            "smelter",
            ContentType::Block,
            json_object(&[(
                "requirements",
                Json::Arr(vec![Json::Str("steel".to_string()), Json::Num("2".to_string())]),
            )]),
        ));

        let imported = ModImporter::new(&registry)
            .import_package(&mut pkg)
            .expect("import ok");
        assert_eq!(imported.project.boards.len(), 1);
        let board = &imported.project.boards[0];
        assert!(board.id.as_deref().expect("board id").starts_with('b'));
        assert_eq!(board.name, "Main");
        assert_eq!(board.file, "main.canvas");
        assert_eq!(board.nodes.len(), 2);
        assert_eq!(
            imported.report.edge_count as usize,
            board.edges.len(),
            "edge count must equal the board's own edges"
        );
        assert_eq!(
            imported.report.edge_count, 1,
            "a requirements reference is one edge"
        );
    }
}
