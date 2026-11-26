//! `ProjectIo.java` 的等价移植：项目目录 = 一个真实模组目录 + 一层作者数据。
//!
//! 对应 Java `dev.modmaker.core.io.ProjectIo`。目录形状（Java 类注释的直译）：
//!
//! ```text
//! <project>/mod.json          真实的 meta 文件（在被编辑之前逐字节保持原样）
//! <project>/content/**        真实的内容文件（在被编辑之前逐字节保持原样）
//! <project>/bundles/**        真实的 bundle 文件
//! <project>/sprites|scripts|… 真实的资产
//! <project>/modmaker/project.json   画布清单、记录 id、来源信息
//! <project>/modmaker/boards/*.canvas  JSON Canvas 画布
//! ```
//!
//! 因为状态就落在模组文件自己身上，项目目录可以直接丢进游戏的 mods 目录去试跑；保存只重写
//! 真正改过的东西，构建就是一次普通拷贝。从没被碰过的记录保住它们原始字节。
//!
//! 常量 [`AUTHORING_DIR`] / [`PROJECT_FILE`] / [`BOARDS_DIR`] / [`TEMP_SUFFIX`] 落在
//! [`crate::project`]，由 `ProjectIo` 与 `ProjectExporter` 共用，这里不再重复定义。

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};

use mm_fmt::codec::{
    file_name_of, is_bundle_file, java_double_text, locale_of, mod_meta_from_json, mod_meta_to_json,
    PropertiesFile,
};
use mm_fmt::ids::uid;
use mm_json::{
    child, child_index, parse_object, parse_or_null, read_file, read_object, replace, set,
    span_text, value_span, write as hjson_write, write_nested as hjson_write_nested, write_pretty,
    Json, Object,
};
use mm_model::mod_project::FORMAT;
use mm_model::{Board, ContentRecord, ModMeta, ModProject};
use mm_schema::{name_rules::is_windows_reserved_name, KeyedMap, SchemaRegistry};

use crate::canvas::json_canvas;
use crate::files::atomic_files;
use crate::import::progress::{worth_reporting, ImportProgress, NoProgress, Stage};
use crate::import::ImportError;
use crate::importer::ModImporter;
use crate::package::{read as read_package, ModPackage};
use crate::project::{AUTHORING_DIR, BOARDS_DIR, PROJECT_FILE};

/// Java 的类级监视器：`ProjectIo` 里所有 `static synchronized` 方法都锁在 `ProjectIo.class` 上。
///
/// Java 的监视器是可重入的，所以 `save`（持锁）里再调 `write`（也要持锁）不会自死锁；Rust 的
/// [`std::sync::Mutex`] 不可重入，因此持锁入口只取一次锁，内部一律走不加锁的
/// [`write_unchecked`]，公开的 [`write`] 自己取锁后再转进去。
static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// 取保存锁。Java 的监视器不会因为某个持锁线程抛异常而「中毒」，异常照原样往外传；
/// Rust 的 `Mutex` 在持锁者 panic 后会拒绝后来者（`PoisonError`），为了让「一次失败保存
/// 不会把整个应用的保存能力永久锁死」这条 Java 行为保持不变，这里丢弃 `PoisonError`
/// 并把锁继续交出去——保存本身是原子的（见 [`crate::files::atomic_files`]），锁只负责
/// 串行化写者，中毒并不损坏磁盘状态。
fn save_lock() -> MutexGuard<'static, ()> {
    SAVE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// --- 写 --------------------------------------------------------------------------------------------

/// 把一个导入来的包落成项目目录，每个文件都按打包时的样子保存。
/// 对应 Java `ProjectIo.unpack(ModProject, ModPackage, Path)`。
pub fn unpack(
    project: &mut ModProject,
    pkg: &mut ModPackage,
    root: &Path,
) -> Result<(), ImportError> {
    unpack_with(project, pkg, root, &mut NoProgress)
}

/// Java `ProjectIo.unpack(ModProject, ModPackage, Path, ImportProgress)`。
///
/// 借用 Java 文档里的关键提醒：下面每一条分支的目标路径都来自不可信的包，所以全部走
/// [`safe_resolve`]，不只是资产（zip-slip：`content/../../evil.hjson`）。
pub fn unpack_with(
    project: &mut ModProject,
    pkg: &mut ModPackage,
    root: &Path,
    progress: &mut dyn ImportProgress,
) -> Result<(), ImportError> {
    fs::create_dir_all(root)?;
    // 拆开字段，好让「读 contents」与「写 skipped」在 Rust 的借用规则下并存（Java 里
    // 一边遍历一边 pkg.skipped.add(...) 是同一件事）。
    let ModPackage {
        meta_file,
        meta_text,
        contents,
        bundle_files,
        asset_files,
        empty_dirs,
        skipped,
        ..
    } = pkg;

    let meta_name = if meta_file.is_empty() {
        "mod.hjson".to_string()
    } else {
        meta_file.clone()
    };
    project.meta_file = meta_name;
    // 逐字保留，BOM 也留着；解析器读回去时会自己剥掉 BOM。
    if meta_text.is_empty() {
        project.meta_text = format!(
            "{}\n",
            write_pretty(&Json::Obj(mod_meta_to_json(&project.meta)))
        );
    } else {
        project.meta_text = meta_text.clone();
    }
    project.meta_dirty = false;

    let total = (contents.len() + bundle_files.len() + asset_files.len() + empty_dirs.len()) as i32;
    progress.stage_started(Stage::Writing, total);
    let mut done: i32 = 0;
    for content in contents.iter() {
        done += 1;
        writing_tick(progress, done, total)?;
        let Some(target) = safe_resolve(root, &content.path) else {
            skipped.push(format!(
                "{} (path escapes the project folder)",
                content.path
            ));
            continue;
        };
        atomic_files::write_text(&target, &content.text)?;
    }
    for (path, text) in bundle_files.iter() {
        done += 1;
        writing_tick(progress, done, total)?;
        let Some(target) = safe_resolve(root, path) else {
            skipped.push(format!("{path} (path escapes the project folder)"));
            continue;
        };
        atomic_files::write_text(&target, text)?;
        let locale = bundle_locale(path);
        project.strings.bundle_raw.put(locale, Arc::from(text.clone()));
    }
    for (path, bytes) in asset_files.iter() {
        done += 1;
        writing_tick(progress, done, total)?;
        let Some(target) = safe_resolve(root, path) else {
            skipped.push(format!("{path} (path escapes the project folder)"));
            continue;
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, bytes)?;
    }
    // 空目录同样是包的一部分（比如一个空的 sprites-override/）。
    for dir in empty_dirs.iter() {
        done += 1;
        writing_tick(progress, done, total)?;
        let Some(target) = safe_resolve(root, dir) else {
            skipped.push(format!("{dir} (path escapes the project folder)"));
            continue;
        };
        fs::create_dir_all(&target)?;
    }
    project.root = Some(root.to_path_buf());
    // 2026-11-26（R-02）：脚本刚被写到 `root/scripts/` 下，落盘之后再扫一遍，导入后的项目
    // 与「重新打开这个目录」得到同一张表（打开路径见 [`open`]）。
    project.script_classes = crate::script_scan::scan_script_classes(root);
    save(project, root)?;
    Ok(())
}

/// 取消检查 + 每个包文件一次节流后的 WRITING tick。
/// 对应 Java `ProjectIo.writingTick`（Java 先 `++done` 再传，这里由调用方自增）。
fn writing_tick(
    progress: &mut dyn ImportProgress,
    done: i32,
    total: i32,
) -> Result<(), ImportError> {
    progress.check_cancelled()?;
    if worth_reporting(done, total) {
        progress.progressed(Stage::Writing, done, total);
    }
    Ok(())
}

/// 解析一个来自不可信包的路径；会逃出项目根目录时返回 `None`。
/// 对应 Java `ProjectIo.safeResolve(Path, String)`。
///
/// Java 靠 `InvalidPathException` 拒绝非法路径（最常见的是 NUL），Rust 的 [`Path`] 不校验，
/// 故这里显式把含 NUL 的相对路径同样判为不可用；归一化与 `startsWith` 的语义见
/// [`lexical_normalize`] 与 [`path_starts_with`]。
pub fn safe_resolve(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.contains('\0') {
        return None;
    }
    let target = lexical_normalize(&root.join(relative));
    let normalized_root = lexical_normalize(root);
    if path_starts_with(&target, &normalized_root) {
        Some(target)
    } else {
        None
    }
}

/// 一次性把改过的状态同步到磁盘。保留给「已经跑在持有模型的那个线程上」的调用方
/// （退出流程、新建项目）；后台保存改用 [`plan`] + [`write`]，这样用户编辑时模型
/// 绝不会被工作线程读取。对应 Java `ProjectIo.save`（`static synchronized`）。
pub fn save(project: &mut ModProject, root: &Path) -> io::Result<()> {
    let _guard = save_lock();
    let planned = plan(project, root)?;
    write_unchecked(&planned)
}

/// 决定一次保存会对磁盘做什么：文件内容、删除与清理。**必须跑在持有模型的线程上**——
/// 它会读字段树并更新 `raw`/`dirty`/`raw_path`，所以不能有别人同时在改这些。返回的计划
/// 是 [`write`] 唯一的输入。对应 Java `ProjectIo.plan`。
///
/// Java 里这一步用 `try/catch` 把 `previousMetaFile` 与 `metaDirty` 复原：规划过程已经翻过
/// 模型上的标记了，不还原的话下一次保存就不会再删掉那个过期的 meta 文件。Rust 没有
/// `catch`，所以把 Java 的主体挪进 [`plan_inner`]，出错时在同一个地方手工还原。
pub fn plan(project: &mut ModProject, root: &Path) -> io::Result<SavePlan> {
    let previous_meta_file_before = project.previous_meta_file.clone();
    let meta_dirty_before = project.meta_dirty;
    match plan_inner(project, root) {
        Ok(planned) => Ok(planned),
        Err(failure) => {
            project.previous_meta_file = previous_meta_file_before;
            project.meta_dirty = meta_dirty_before;
            Err(failure)
        }
    }
}

/// Java `ProjectIo.plan` 的 `try` 块本体。
fn plan_inner(project: &mut ModProject, root: &Path) -> io::Result<SavePlan> {
    let mut planned = SavePlan::new(root, project);
    project.root = Some(root.to_path_buf());

    write_meta(project, root, &mut planned);
    let expected_content = write_content(project, root, &mut planned)?;
    let protected = project.asset_paths();
    planned.prune(
        root.join("content"),
        expected_content,
        protected,
        vec![".json".to_string(), ".hjson".to_string()],
    );
    write_bundles(project, root, &mut planned);
    write_authoring_layer(project, root, &mut planned)?;
    Ok(planned)
}

/// 执行一个计划。这一半只碰文件系统，所以可以在工作线程上跑，用户那边继续编辑。
/// 加锁：退出流程在 FX 线程上写，而 io 线程池可能同时在写，两个写者不能在同一些
/// `.mm-tmp` 文件上赛跑。对应 Java `ProjectIo.write`（`static synchronized`）。
pub fn write(planned: &SavePlan) -> io::Result<()> {
    let _guard = save_lock();
    write_unchecked(planned)
}

/// Java `ProjectIo.write` 的 `synchronized` 本体；由 [`save`] 与 [`write`] 各自取锁后调用，
/// 以免 Rust 不可重入的锁把 Java 的「保存内部再调 write」变成自死锁。
fn write_unchecked(planned: &SavePlan) -> io::Result<()> {
    fs::create_dir_all(&planned.root)?;
    for op in planned.writes.iter() {
        // 增量跳写：盘上内容已一致就不碰那个文件——autosave 不再每周期重写整包，
        // 未改文件的 mtime 也不被打扰。读不出来（不存在/非 UTF-8）按不一致照常写。
        if fs::read_to_string(&op.target).map(|existing| existing == op.text).unwrap_or(false) {
            continue;
        }
        atomic_files::write_text(&op.target, &op.text)?;
    }
    for target in planned.deletes.iter() {
        delete_if_exists(target)?;
    }
    for op in planned.prunes.iter() {
        prune_managed(
            &planned.root,
            &op.folder,
            &op.expected,
            &op.protected_paths,
            &op.extensions,
        )?;
    }
    Ok(())
}

/// 计划要写的一个文件。对应 Java `ProjectIo.WriteOp`（私有 record）。
struct WriteOp {
    target: PathBuf,
    text: String,
}

/// 计划要清理的一个受管目录，外加必须活过清理的绝对路径。
/// 对应 Java `ProjectIo.PruneOp`（私有 record）。
struct PruneOp {
    folder: PathBuf,
    expected: Vec<PathBuf>,
    protected_paths: Vec<String>,
    extensions: Vec<String>,
}

/// 一次保存将要做什么——在模型还读得到的时候就定下来。把两半拆开正是消除赛跑的办法：
/// 从前 io 线程会在 FX 线程改同一个 map 的同时序列化 `record.fields`。
/// 对应 Java `ProjectIo.SavePlan`。
///
/// Java 的 `SavePlan` 由 `synchronized` 的 `write` 保护，且对模型只留引用；Rust 侧
/// 借用规则不允许一个跨线程的计划借着模型，所以这里把写者需要的一切、以及回滚要用的
/// 那一份模型状态，全部按值搬进计划里。`root` 因此由「每次现算」变成构造时定好，
/// `Files.createDirectories(plan.root())` 与 `plan.root()` 用的是同一个路径。
///
/// Java 的 `recordDirtyBefore` 以记录对象本身为键，为的是「写进行中新增或被删掉的记录
/// 不会让回滚落到错误的记录上」。Rust 没有对象身份，改用作者层自己就在用的记录 id
/// 作身份键（导入器与 `ProjectFactory` 一定会给 id 赋值），没有 id 的记录退回按下标定位。
pub struct SavePlan {
    root: PathBuf,
    writes: Vec<WriteOp>,
    deletes: Vec<PathBuf>,
    prunes: Vec<PruneOp>,
    record_dirty_before: Vec<(usize, Option<String>, bool)>,
    meta_dirty_before: bool,
    bundle_dirty_before: KeyedMap<bool>,
    previous_meta_file_before: Option<String>,
}

impl SavePlan {
    /// Java 的包私有构造器。
    fn new(root: &Path, project: &ModProject) -> SavePlan {
        SavePlan {
            root: root.to_path_buf(),
            writes: Vec::new(),
            deletes: Vec::new(),
            prunes: Vec::new(),
            record_dirty_before: project
                .contents
                .iter()
                .enumerate()
                .map(|(index, record)| (index, record.id.clone(), record.dirty))
                .collect(),
            meta_dirty_before: project.meta_dirty,
            bundle_dirty_before: project.strings.bundle_dirty.clone(),
            previous_meta_file_before: project.previous_meta_file.clone(),
        }
    }

    /// 这次保存会落笔的目录。对应 Java `SavePlan.root()`。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 对应 Java `SavePlan.write(Path, String)`（包私有）。
    fn write(&mut self, target: PathBuf, text: String) {
        self.writes.push(WriteOp { target, text });
    }

    /// 对应 Java `SavePlan.delete(Path)`（包私有）。
    fn delete(&mut self, target: PathBuf) {
        self.deletes.push(target);
    }

    /// 对应 Java `SavePlan.prune(Path, Set, Set, Set)`（包私有）。
    fn prune(
        &mut self,
        folder: PathBuf,
        expected: Vec<PathBuf>,
        protected_paths: Vec<String>,
        extensions: Vec<String>,
    ) {
        self.prunes.push(PruneOp {
            folder,
            expected,
            protected_paths,
            extensions,
        });
    }

    /// 把模型上的脏标记恢复到规划之前的样子；被删掉的记录会重新标脏，所以下一次保存
    /// 能把它写回去；规划之后才新增的记录仍然脏，因此照样落盘。
    /// 对应 Java `SavePlan.rollback()`——它只往回套脏位，绝不做结构改动，所以可以安全地
    /// 跑在工作线程上，即使用户正在加/删记录。
    pub fn rollback(&self, project: &mut ModProject) {
        for (index, id, dirty) in self.record_dirty_before.iter() {
            match id {
                Some(id) => {
                    if let Some(record) = project
                        .contents
                        .iter_mut()
                        .find(|candidate| candidate.id.as_deref() == Some(id.as_str()))
                    {
                        record.dirty = *dirty;
                    }
                }
                // Java 用对象身份做键，这条分支只覆盖「没有 id 的记录」这种进不了
                // 保存流程的中间态：按规划时记下的下标原位还原。
                None => {
                    if let Some(record) = project.contents.get_mut(*index) {
                        record.dirty = *dirty;
                    }
                }
            }
        }
        project.meta_dirty = self.meta_dirty_before;
        project.previous_meta_file = self.previous_meta_file_before.clone();
        project.strings.bundle_dirty = self.bundle_dirty_before.clone();
    }
}

/// 写根目录上的 meta 文件。对应 Java `ProjectIo.writeMeta`。
fn write_meta(project: &mut ModProject, root: &Path, planned: &mut SavePlan) {
    let mut meta_name = if project.meta_file.is_empty() {
        "mod.hjson".to_string()
    } else {
        project.meta_file.clone()
    };
    // 纵深防御：meta 名只可能来自固定的探测表，或者来自（打开时已经校验过的）作者层；
    // 出现别的名字就说明状态已经损坏了。
    if !ModMeta::META_FILES.iter().any(|name| *name == meta_name) {
        meta_name = "mod.hjson".to_string();
        project.meta_dirty = true;
    }
    let target = root.join(&meta_name);
    let hjson = meta_name.to_lowercase().ends_with(".hjson");
    let text = if !project.meta_dirty && !project.meta_text.is_empty() {
        project.meta_text.clone()
    } else {
        let json = Json::Obj(mod_meta_to_json(&project.meta));
        if hjson {
            format!("{}\n", hjson_write(&json))
        } else {
            format!("{}\n", write_pretty(&json))
        }
    };
    planned.write(target, text.clone());
    project.meta_text = text;
    project.meta_dirty = false;
    // 一次格式转换会把 meta 文件改名：旧文件不能留在根目录，那里游戏照样会探测到它、
    // 构建也照样会把它拷进包里。
    if let Some(previous) = &project.previous_meta_file
        && previous != &meta_name
    {
        planned.delete(root.join(previous));
    }
    project.previous_meta_file = None;
}

/// Java 的 `record.id.equals(inline.inlineOwnerId)`。`id` 为 `None` 的记录在本工具里不会出现
/// （导入器与 [`crate::project_factory`] 必定赋值），Java 那一步会直接 NPE，这里按「不相等」处理。
fn owned_by(host_id: Option<&str>, inline: &ContentRecord) -> bool {
    match host_id {
        Some(id) => inline.inline_owner_id.as_deref() == Some(id),
        None => false,
    }
}

/// 写出全部内容文件，返回「必须活过 content 清理」的那些绝对路径。
/// 对应 Java `ProjectIo.writeContent`——Java 用 `LinkedHashSet<Path>`，这里用 [`Vec`] 保序并显式判重。
fn write_content(
    project: &mut ModProject,
    root: &Path,
    planned: &mut SavePlan,
) -> io::Result<Vec<PathBuf>> {
    let mut expected: Vec<PathBuf> = Vec::new();
    let mut expected_seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    // 「哪些宿主有脏内联」一次建表：逐宿主全量扫 contents 是 O(n²)/次保存。
    // 持有所有权而非借用：循环体内要就地改 project（raw/dirty/raw_path）。
    let dirty_inline_hosts: std::collections::HashSet<String> = project
        .contents
        .iter()
        .filter(|record| record.is_inline() && record.dirty)
        .filter_map(|record| record.inline_owner_id.clone())
        .collect();
    for index in 0..project.contents.len() {
        if project.contents[index].is_inline() {
            continue;
        }
        let host_id = project.contents[index].id.clone();
        let inline_changed = host_id
            .as_deref()
            .map(|id| dirty_inline_hosts.contains(id))
            .unwrap_or(false);
        let previous_path = project.contents[index].raw_path.clone();

        let target = target_path(root, &project.contents[index]);
        // 纵深防御：两条记录落到同一个文件上，后者会把前者覆盖掉，既丢内容、又（重新打开时）
        // 丢记录本身。宁可大声拒绝，也不要悄悄丢数据。
        let key = absolute_normalized(&target);
        if !expected_seen.insert(key.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "two content records resolve to the same file ({}); rename one of them before saving",
                    to_slash(&java_relativize(root, &target))
                ),
            ));
        }
        expected.push(key);

        let dirty = project.contents[index].dirty;
        let raw = project.contents[index].raw.clone();
        let text = if !dirty && raw.is_some() {
            // 宿主自己没动、只有内联记录动了：把改过的那一段值拼回原始字节里，
            // 文件其余部分的注释、空行和手写格式才保得住。
            let patched = if inline_changed {
                patch_inlines(project, index)
            } else {
                raw.clone()
            };
            match patched {
                Some(text) => text,
                None => serialize(project, index),
            }
        } else {
            serialize(project, index)
        };
        planned.write(target.clone(), text.clone());

        let raw_path = to_slash(&java_relativize(root, &target));
        project.contents[index].raw_path = Some(raw_path.clone());
        project.contents[index].raw = Some(text);
        project.contents[index].dirty = false;

        if let Some(previous_path) = &previous_path
            && previous_path != &raw_path
        {
            // 大小写不敏感的文件系统：只改了名字大小写时，旧路径解析到的正是刚刚写出的那个
            // 文件，不能删。
            let previous = root.join(previous_path);
            if !same_file_on_disk(&previous, &target) {
                planned.delete(previous);
            }
        }
    }
    for record in project.contents.iter_mut() {
        if record.is_inline() {
            record.dirty = false;
        }
    }
    Ok(expected)
}

/// 这条记录的文件该放在哪里。来自包的内容留在它被导入时所在的那个目录里（模组会把内容套进
/// 自己的子目录，一次编辑不该把它挪走）；改名只改该目录里的文件名。
/// 对应 Java `ProjectIo.targetPath`。
fn target_path(root: &Path, record: &ContentRecord) -> PathBuf {
    let canonical = || root.join(canonical_content_path(record));
    let Some(raw_path) = record.raw_path.as_deref() else {
        return canonical();
    };
    let existing = root.join(raw_path);
    let Some(parent) = existing.parent() else {
        return canonical();
    };
    let target = parent.join(format!("{}.{}", record.name, record.ext));
    if !path_starts_with(&lexical_normalize(&target), &lexical_normalize(root)) {
        return canonical();
    }
    target
}

/// 只写磁盘上真有 bundle 的那些语言，外加编辑器碰过的语言。语言永远不会被凭空造出来：
/// 一个只带 `bundle_zh_CN.properties` 的模组就恰好只保留那一个文件。
/// 对应 Java `ProjectIo.writeBundles`。
fn write_bundles(project: &mut ModProject, root: &Path, planned: &mut SavePlan) {
    let mut expected: Vec<PathBuf> = Vec::new();
    let mut locales: Vec<String> = project.strings.bundle_files.keys().cloned().collect();
    for locale in project.locales.iter() {
        if project.strings.is_bundle_dirty(locale) {
            push_unique(&mut locales, locale);
        }
    }
    for locale in project.strings.extra.keys() {
        push_unique(&mut locales, locale);
    }

    for locale in locales {
        let file_name = project
            .strings
            .bundle_files
            .get(&locale)
            .cloned()
            .unwrap_or_else(|| file_name_of(Some(locale.as_str())));
        let target = root.join("bundles").join(&file_name);
        // Java 是 `!isBundleDirty && containsKey` 之后再 `get`；`KeyedMap::get` 本身就返回
        // `Option`，两次查询合成一次即可。
        let cached = if project.strings.is_bundle_dirty(&locale) {
            None
        } else {
            project.strings.bundle_raw.get(&locale).cloned()
        };
        let text: Arc<str> = match cached {
            Some(text) => text,
            None => Arc::from(render_bundle(project, &locale)),
        };
        planned.write(target.clone(), text.to_string());
        project.strings.bundle_raw.put(locale.clone(), text);
        project.strings.bundle_files.put(locale.clone(), file_name);
        project.strings.bundle_dirty.put(locale.clone(), false);
        expected.push(absolute_normalized(&target));
    }
    let protected = project.asset_paths();
    planned.prune(
        root.join("bundles"),
        expected,
        protected,
        vec![".properties".to_string()],
    );
}

/// 一个语言的 bundle 文本：模组随包带来的那个文件里只重写被改过的那些行，于是注释、空行、
/// 键序和排版都能在一次编辑之后活下来（手改过的 bundle 是一份文档，不是一张表）。没有随包
/// 文本的语言仍像过去那样从头渲染。
///
/// 一个键只有在项目不再定义它时才会丢掉那一行——即被删掉的内容，或被作者清空的字符串——
/// 而这正是「重新渲染整个 bundle」过去会对每个键做的事。随包文件自己以空值定义的键保持原样：
/// 用户没碰它，工具也就不该碰。
/// 对应 Java `ProjectIo.renderBundle`。
fn render_bundle(project: &ModProject, locale: &str) -> String {
    let mut file = match project.strings.bundle_raw.get(locale) {
        Some(shipped) => PropertiesFile::parse(shipped),
        None => PropertiesFile::default(),
    };

    let mut wanted: KeyedMap<String> = KeyedMap::new();
    // Java：`.filter(...).sorted(Map.Entry.comparingByKey())`，即先按外层键的自然序排序再落表。
    let mut keys: Vec<String> = project
        .strings
        .values
        .iter()
        .filter(|(_, by_locale)| by_locale.contains_key(locale))
        .map(|(key, _)| key.clone())
        .collect();
    keys.sort();
    for key in keys {
        // 上面的过滤已保证这一项存在；Java 里 `get(locale) != null` 那一半在 Rust 由 `Option` 承担。
        if let Some(value) = project
            .strings
            .values
            .get(&key)
            .and_then(|by_locale| by_locale.get(locale))
        {
            wanted.put(key, value.clone());
        }
    }
    if let Some(extras) = project.strings.extra.get(locale) {
        for (key, value) in extras.iter() {
            wanted.put(key.clone(), value.clone());
        }
    }

    for (key, value) in wanted.iter() {
        let shipped_value = file.values.get(key).cloned();
        if value.is_empty() && shipped_value.as_ref().is_none_or(|held| !held.is_empty()) {
            file.remove(key);
            continue;
        }
        file.put(key.clone(), value.clone());
    }
    for key in file.values.keys().cloned().collect::<Vec<String>>() {
        if !wanted.contains_key(&key) {
            file.remove(&key);
        }
    }
    file.write()
}

/// `LinkedHashSet.add`：已存在就什么都不做，否则按到达顺序追加。
fn push_unique(items: &mut Vec<String>, value: &str) {
    if !items.iter().any(|item| item == value) {
        items.push(value.to_string());
    }
}

/// 删掉本工具管理、但已经不再对应项目里任何东西的文件。属于项目的资产（未知的内容目录、
/// 不合规矩的文件名）绝不碰，因此一次保存不可能弄丢模组带来的任何东西。
/// 对应 Java `ProjectIo.pruneManaged`。
///
/// Java 里 `Files.walk` 的 `IOException` 会往外传；这里让目录读取失败只表现为「看不到文件」，
/// 与 Java 在同一个目录被并发删除时的实际后果一致（没有候选文件可删）。
fn prune_managed(
    root: &Path,
    folder: &Path,
    expected: &[PathBuf],
    protected_paths: &[String],
    extensions: &[String],
) -> io::Result<()> {
    if !folder.is_dir() {
        return Ok(());
    }
    let expected_real = real_paths(expected);
    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_regular_files(folder, &mut candidates);
    for file in candidates {
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // 注意这里刻意不是 `Path::extension()`：Java 取的是 `name.substring(lastIndexOf('.'))`，
        // **带着那个点**，所以下面的集合里写的也是 ".json"/".hjson"/".properties"。
        let extension = match name.rfind('.') {
            None => String::new(),
            Some(dot) => name[dot..].to_lowercase(),
        };
        if !extensions.iter().any(|held| held == &extension) {
            continue;
        }
        if folder.file_name().and_then(|held| held.to_str()) == Some("bundles")
            && !is_bundle_file(&name)
        {
            continue;
        }
        let relative = to_slash(&java_relativize(root, &file));
        if protected_paths.iter().any(|held| held == &relative) {
            continue;
        }
        // 比较真实路径，好让一个只是与「预期文件」大小写不同的文件（大小写不敏感的文件系统）
        // 被认成同一个文件而保留下来。取不到真实路径时 Java 的 `Set.contains(null)` 为假，
        // 也就是照删不误，这里保持同样结果。
        match real_path(&file) {
            Some(real) if expected_real.iter().any(|held| path_eq(held, &real)) => {}
            _ => delete_if_exists(&file)?,
        }
    }
    Ok(())
}

/// 对应 Java `Files.walk(folder).filter(Files::isRegularFile).toList()`：前序、按目录流给出的
/// 顺序收集普通文件。
fn collect_regular_files(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_regular_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

/// 对应 Java `ProjectIo.realPaths`：解析不出来的路径直接跳过（Java 会把 null 挡在外面）。
fn real_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter_map(|path| real_path(path)).collect()
}

/// 对应 Java `ProjectIo.realPath`（`Path.toRealPath()`，失败返回 null）：Rust 用
/// [`fs::canonicalize`]，它在 Windows 上顺带解析大小写与链接，正是 `toRealPath` 的行为。
fn real_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

/// 两个路径是否已经指向同一个文件（大小写不敏感 / 防符号链接的检查）。
/// 对应 Java `ProjectIo.sameFileOnDisk`（`Files.isSameFile`），Java 内部同样是比较真实路径。
fn same_file_on_disk(a: &Path, b: &Path) -> bool {
    if !a.is_file() || !b.is_file() {
        return false;
    }
    let (Some(real_a), Some(real_b)) = (real_path(a), real_path(b)) else {
        return false;
    };
    real_a == real_b
}

/// 对应 Java `Files.deleteIfExists`：文件本来就不存在不算失败。
fn delete_if_exists(target: &Path) -> io::Result<()> {
    match fs::remove_file(target) {
        Ok(()) => Ok(()),
        Err(failure) if failure.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(failure) => Err(failure),
    }
}

/// 把宿主记录连同它的全部内联子记录一起序列化成一个文件。对应 Java `ProjectIo.serialize`。
fn serialize(project: &mut ModProject, host: usize) -> String {
    let object = merged_tree(project, host);
    let host_id = project.contents[host].id.clone();
    for inline in project.contents.iter_mut() {
        if owned_by(host_id.as_deref(), inline) {
            inline.dirty = false;
        }
    }
    format!("{}\n", write_tree(&object, &project.contents[host]))
}

/// 宿主字段树与它全部内联子树合成的那一棵树（正是磁盘上该有的内容）。
/// 对应 Java `ProjectIo.mergedTree`（`record.fields.forEach(object::add)` 即按书写顺序拷贝）。
fn merged_tree(project: &ModProject, host: usize) -> Json {
    let host_id = project.contents[host].id.clone();
    let mut tree = Json::Obj(project.contents[host].fields.clone());
    for inline in project.contents.iter() {
        if !owned_by(host_id.as_deref(), inline) {
            continue;
        }
        // 内联记录必定带路径（`is_inline()` 判的就是 owner，路径与 owner 一同赋值）；
        // 缺路径的记录留原样，Java 那一步会 NPE。
        if let Some(path) = inline.inline_path.as_deref() {
            set(&mut tree, path, Json::Obj(inline.fields.clone()));
        }
    }
    tree
}

/// 宿主文件没被改动、只有内联子记录被改动时，把这些子树拼回原始字节里，于是文件其余部分的
/// 注释、空行和手写格式都能保住。任何一段拼不回去就返回 `None`，调用方退回整文件重新序列化。
/// 对应 Java `ProjectIo.patchInlines`（Java 按 `project.contents` 的原序遍历内联记录）。
fn patch_inlines(project: &mut ModProject, host: usize) -> Option<String> {
    let mut text = project.contents[host].raw.clone()?;
    let hjson = hjson_ext(&project.contents[host]);
    let host_id = project.contents[host].id.clone();
    let inlines: Vec<usize> = project
        .contents
        .iter()
        .enumerate()
        .filter(|(_, inline)| owned_by(host_id.as_deref(), inline) && inline.dirty)
        .map(|(index, _)| index)
        .collect();
    for index in inlines {
        // 内联记录必定带路径（与 owner 一同赋值）；缺路径时 Java 会 NPE，这里当作拼不回去。
        let inline_path = project.contents[index].inline_path.clone()?;
        let updated = Json::Obj(project.contents[index].fields.clone());
        // 这一段拼不回去（None）时，整个文件就重新序列化。
        text = splice_inline(&text, &inline_path, &updated, hjson)?;
        project.contents[index].dirty = false;
    }
    let reparsed = parse_or_null(&text);
    // 只有当解析回来的字节与「重新序列化会写出的东西」完全一致时才信任拼接结果——
    // 这是数据安全的检查，不是字符串比较。注意 Java 的 `JsonObject.equals` 与键序无关，
    // 而这里 `Json` 的结构化相等对键序敏感：键序真对不上时只是退回重新序列化，不会丢数据。
    match reparsed {
        Some(reparsed) if reparsed == merged_tree(project, host) => Some(text),
        _ => None,
    }
}

/// 把一个内联子树写回宿主的原始字节。
///
/// 只改叶子的变化逐个值替换，于是这条内联记录自己的注释连同宿主的注释都能保住；形状变了
/// （成员增减）没法用值替换表达，就退回到替换整个嵌套对象，那仍然保住它外面的每一处注释。
/// 返回 `None` 表示连那一步也定位不到。
/// 对应 Java `ProjectIo.spliceInline`（Java 的 `record` 实参只用于判断方言，这里换成 `hjson`）。
fn splice_inline(text: &str, inline_path: &str, updated: &Json, hjson: bool) -> Option<String> {
    let span = value_span(text, inline_path)?;
    let previous = parse_or_null(&span_text(text, span));
    let mut paths: Vec<String> = Vec::new();
    let mut values: Vec<Json> = Vec::new();
    if let Some(previous) = &previous
        && leaf_changes(Some(previous), updated, "", &mut paths, &mut values)
    {
        let mut patched = String::from(text);
        for position in 0..paths.len() {
            let path = if paths[position].is_empty() {
                inline_path.to_string()
            } else {
                format!("{inline_path}.{}", paths[position])
            };
            let rendered = write_nested(&values[position], hjson);
            patched = replace(&patched, &path, &rendered)?;
        }
        return Some(patched);
    }
    // 回退路径：替换整个值，因此至少那些没问题的内联记录仍能保住自己的注释。
    replace(text, inline_path, &write_nested(updated, hjson))
}

/// 递归比较两棵树，把「值确有变化」的叶子路径收集起来。遇到数组长度变化或键集变化就放弃
/// （返回 `false`），因为那种情况需要动结构，逐叶 splice 已经不安全。
/// 对应 Java `ProjectIo.leafChanges`。
fn leaf_changes(
    before: Option<&Json>,
    after: &Json,
    path: &str,
    paths: &mut Vec<String>,
    values: &mut Vec<Json>,
) -> bool {
    if let Some(before) = before {
        if before == after {
            return true;
        }
        if let (Json::Obj(old_object), Json::Obj(new_object)) = (before, after) {
            if !same_key_set(old_object, new_object) {
                return false;
            }
            for (key, new_child) in new_object.iter() {
                if !leaf_changes(
                    old_object.get(key),
                    new_child,
                    &child(path, key),
                    paths,
                    values,
                ) {
                    return false;
                }
            }
            return true;
        }
        if let (Json::Arr(old_items), Json::Arr(new_items)) = (before, after) {
            if old_items.len() != new_items.len() {
                return false;
            }
            for (index, new_child) in new_items.iter().enumerate() {
                if !leaf_changes(
                    old_items.get(index),
                    new_child,
                    &child_index(path, index),
                    paths,
                    values,
                ) {
                    return false;
                }
            }
            return true;
        }
    }
    if path.is_empty() {
        return false;
    }
    paths.push(path.to_string());
    values.push(after.clone());
    true
}

/// 两个对象的键集是否相同——对应 Java 的 `oldObject.keySet().equals(newObject.keySet())`。
fn same_key_set(old_object: &Object, new_object: &Object) -> bool {
    old_object.len() == new_object.len()
        && old_object.keys().all(|key| new_object.contains(key))
}

/// 扩展名是一个格式决定：`.hjson` 记录用手写方言写，其余一律规范 JSON。
/// 对应 Java `writeTree`/`writeNested` 里的 `record.ext.equalsIgnoreCase("hjson")`。
fn hjson_ext(record: &ContentRecord) -> bool {
    record.ext.eq_ignore_ascii_case("hjson")
}

/// 对应 Java `ProjectIo.writeTree`。
///
/// **2026-11-16 P5-M6 更正（原文一字不改地留在下方）**：这里原先接的是 `write_nested`，
/// 而 Java `ProjectIo.java:674-679` 的 `writeTree` 走的是 `HjsonWriter.write(object)`——
/// 也就是**整文件那一支、根对象不带花括号**（`HjsonWriter.java:30-32` → `:45-48`
/// `writeMembers(..., braceless=true)`）；带括号的那一支是 `writeNested`（`:39-43`），
/// 只用于往文件里拼接一个值。于是脏记录被重新序列化时，整个文件被套上一对根括号、
/// 每一行多缩进两级，M6 门实测发现后改回 `hjson_write`。
fn write_tree(object: &Json, record: &ContentRecord) -> String {
    if hjson_ext(record) {
        hjson_write(object)
    } else {
        write_pretty(object)
    }
}

/// 对应 Java `ProjectIo.writeNested`。
fn write_nested(value: &Json, hjson: bool) -> String {
    if hjson {
        hjson_write_nested(value)
    } else {
        write_pretty(value)
    }
}

/// 能不能安全地把这个名字直接用作作者层里唯一的画布文件名。
///
/// 作者层是可手改的文本（用户可以在 Obsidian 或文本编辑器里打开它），所以这里的文件清单在
/// 磁盘布局上属于不可信输入：必须留在 `boards/` 之内，且不能撞上 `pruneManaged` 永不删除的
/// bundle 后缀——那样一块已删的画布就再也删不掉了。Java 声明为包级可见，故这里是
/// `pub(crate)`。
/// 对应 Java `ProjectIo.isSafeBoardFile`。
pub(crate) fn is_safe_board_file(recorded: Option<&str>) -> bool {
    let Some(file) = recorded else {
        return false;
    };
    if file.is_empty() || file.chars().count() > 120 {
        return false;
    }
    if file.contains('/')
        || file.contains('\\')
        || file.contains(':')
        || file.contains("..")
        || file.starts_with('.')
    {
        return false;
    }
    if is_windows_reserved_name(file) {
        return false;
    }
    // `.canvas` 是正向判据，因此 bundle 一类的后缀永远不会被误当成画布。
    file.to_lowercase().ends_with(".canvas")
}

/// 在 `used` 已被占用的前提下，取一个安全且唯一的画布文件名。
/// 对应 Java `ProjectIo.uniqueBoardFile`。
fn unique_board_file(preferred: Option<&str>, used: &[String]) -> String {
    let name = if is_safe_board_file(preferred) {
        preferred.unwrap_or_default().to_string()
    } else {
        "main.canvas".to_string()
    };
    if !used.contains(&name) {
        return name;
    }
    // 走到这里时 `name` 必定以 `.canvas` 结尾（要么是安全名，要么是那个兜底名）。
    let prefix = name[..name.len() - ".canvas".len()].to_string();
    let mut index = 2;
    loop {
        let candidate = format!("{prefix}-{index}.canvas");
        if !used.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

/// 写出工具自己的那一层：板子清单、记录 id、id 与文件的对应关系。对应 Java
/// `ProjectIo.writeAuthoringLayer`。
fn write_authoring_layer(project: &mut ModProject, root: &Path, planned: &mut SavePlan) -> io::Result<()> {
    let authoring = root.join(AUTHORING_DIR);
    let boards_dir = authoring.join(BOARDS_DIR);
    // Java 在 plan 阶段就把目录建好了（它真的碰了磁盘），保持一致。
    fs::create_dir_all(&boards_dir)?;

    let mut meta = Object::new();
    meta.insert("format", Json::num(FORMAT.to_string()));
    meta.insert("schemaProfile", Json::str(project.schema_profile.clone()));
    meta.insert("importedFrom", Json::str(project.imported_from.clone()));
    meta.insert("metaFile", Json::str(project.meta_file.clone()));

    let mut records = Object::new();
    for index in 0..project.contents.len() {
        let Some(key) = record_key(project, index) else {
            continue;
        };
        // `addProperty(key, record.id)`：id 为 null 时 Java 仍会写入一个 null 成员。
        let id = match &project.contents[index].id {
            Some(id) => Json::str(id.clone()),
            None => Json::Null,
        };
        records.insert(key, id);
    }
    meta.insert("records", Json::Obj(records));

    // 第一趟：把文件名在模型上定下来。Java 在同一个循环里既改 `board.file` 又写画布，但这里
    // `to_canvas` 要同时借整个项目与这块板子，所以拆成两趟：本趟只碰模型，不碰计划。
    let mut used_files: Vec<String> = Vec::new();
    for board in project.boards.iter_mut() {
        // 与读路径同样的纵深防御：逃出 boards 目录、或已被别的板子占用的名字绝不能走到
        // writeText，在这里归一化还能让模型与 project.json 保持一致。
        let file = unique_board_file(Some(board.file.as_str()), &used_files);
        used_files.push(file.clone());
        board.file = file;
    }

    // 第二趟：只读地组装清单条目并写出画布，顺序与 Java 的 `plan.write` 完全一致。
    let mut boards: Vec<Json> = Vec::new();
    for board in project.boards.iter() {
        let mut entry = Object::new();
        // Java 的 `addProperty(key, (String) null)` 会写入一个 null 成员，这里同样保留 `Json::Null`。
        entry.insert(
            "id",
            match &board.id {
                Some(id) => Json::str(id.clone()),
                None => Json::Null,
            },
        );
        entry.insert("name", Json::str(board.name.clone()));
        entry.insert("file", Json::str(board.file.clone()));
        let mut viewport = Object::new();
        viewport.insert("x", Json::num(java_double_text(board.viewport.x)));
        viewport.insert("y", Json::num(java_double_text(board.viewport.y)));
        viewport.insert("zoom", Json::num(java_double_text(board.viewport.zoom)));
        entry.insert("viewport", Json::Obj(viewport));
        if !is_blank(&board.edge_style) && board.edge_style != "bezier" {
            entry.insert("edgeStyle", Json::str(board.edge_style.clone()));
        }
        boards.push(Json::Obj(entry));
        // Java：`plan.write(boardsDir.resolve(file), Json.write(JsonCanvas.toCanvas(project, board)) + "\n")`。
        let canvas = json_canvas::to_canvas(Some(project), board);
        planned.write(
            boards_dir.join(&board.file),
            format!("{}\n", write_pretty(&canvas)),
        );
    }
    meta.insert("boards", Json::Arr(boards));

    planned.write(
        authoring.join(PROJECT_FILE),
        format!("{}\n", write_pretty(&Json::Obj(meta))),
    );
    Ok(())
}

/// 打开一个项目目录：内容来自真实的模组文件，板子来自 `modmaker/`。
/// 对应 Java `ProjectIo.open`。
pub fn open(root: &Path, registry: &SchemaRegistry) -> Result<ModProject, ImportError> {
    let mut pkg = read_package(root)?;
    let mut project = ModImporter::new(registry).import_package(&mut pkg)?.project;
    project.root = Some(root.to_path_buf());
    // 2026-11-26（产品评审 R-02）：真实模组的方块常常不写 `type`，类写在 `scripts/**/*.js`
    // 的 `extend(...)` 里。扫描结果只活在内存（`script_classes` 不落盘），每次打开重扫。
    project.script_classes = crate::script_scan::scan_script_classes(root);

    let project_file = root.join(AUTHORING_DIR).join(PROJECT_FILE);
    if !project_file.exists() {
        return Ok(project);
    }
    // 作者层是可再生的：一个坏掉的 project.json 或画布不能让真实模组打不开。保留导入出来的
    // 项目（它自带一块板子），把故障记成警告。Java 捕获的是 `IOException | RuntimeException`。
    if let Err(failure) = read_authoring_layer(&mut project, root, &project_file) {
        project.authoring_warning = Some(format!("authoring layer ignored: {failure}"));
    }
    Ok(project)
}

/// 读取并套用作者层（schema 档位、meta 认领、记录 id、板子清单）。
/// 对应 Java `ProjectIo.readAuthoringLayer`。
fn read_authoring_layer(
    project: &mut ModProject,
    root: &Path,
    project_file: &Path,
) -> io::Result<()> {
    // Java：`Json.parseObject(Json.readFile(projectFile))`——解析失败抛出，由 `open` 吞成警告。
    let meta = parse_object(&read_file(project_file)?).map_err(invalid_data)?;

    if let Some(schema_profile) = text_value(&meta, "schemaProfile") {
        project.schema_profile = schema_profile;
    }
    if let Some(imported_from) = text_value(&meta, "importedFrom") {
        project.imported_from = imported_from;
    }
    if let Some(recorded_meta) = text_value(&meta, "metaFile")
        && !recorded_meta.is_empty()
    {
        let probed = project.meta_file.clone();
        let recorded_path = root.join(&recorded_meta);
        // 只有当记录的名字仍然存在、且是游戏会探测的那些名字之一时才认领它：
        // 一个被外部改过名的 meta 不能让旧文件在下一次保存时复活。
        if ModMeta::META_FILES.iter().any(|name| *name == recorded_meta)
            && recorded_path.is_file()
        {
            if recorded_meta != probed {
                project.previous_meta_file = Some(probed);
                // Java 用的是 `Files.readString(path, UTF_8)`，而不是 `Json.readFile`：
                // 这里刻意**保留 BOM**（解析器读它时会自己剥掉），故必须用 `fs::read_to_string`。
                project.meta_text = fs::read_to_string(&recorded_path)?;
                project.meta_dirty = false;
                // Java 把整个 `ModMetaCodec.fromJson(...)` 包在 try/catch 里；Rust 侧
                // `mod_meta_from_json` 不返回错误，失败只可能来自读取那一步。
                match read_object(&project.meta_text) {
                    Ok(object) => {
                        project.meta = mod_meta_from_json(&object);
                        project.meta_parse_failed = false;
                    }
                    // 保住原始字节：一次格式转换不能把它们换成一份空元数据。
                    Err(_) => project.meta_parse_failed = true,
                }
            }
            project.meta_file = recorded_meta;
        }
    }

    // 还原记录 id，好让保存下来的节点位置和连线仍然对得上。文件支撑的记录先还原，
    // 它们的「旧 id -> 新 id」映射再套到内联链接上。
    if let Some(records) = object_value(&meta, "records") {
        let mut remapped: KeyedMap<String> = KeyedMap::new();
        for record in &project.contents {
            let inline = record.is_inline();
            let raw_path = record.raw_path.clone();
            if inline || raw_path.is_none() {
                continue;
            }
            let old_id = text_value(records, raw_path.as_deref().unwrap_or_default());
            if let (Some(old_id), Some(id)) = (old_id, record.id.clone()) {
                remapped.put(id, old_id);
            }
        }
        for index in 0..project.contents.len() {
            if !project.contents[index].is_inline() {
                continue;
            }
            let owner = project.contents[index].inline_owner_id.clone();
            project.contents[index].inline_owner_id = owner.as_ref().map(|owner| remapped.get(owner).cloned().unwrap_or_else(|| owner.clone()));
        }
        for index in 0..project.contents.len() {
            let key = record_key(project, index);
            if let Some(key) = key
                && let Some(id) = text_value(records, &key)
            {
                project.contents[index].id = Some(id);
            }
        }
    }

    let boards = array_value(&meta, "boards");
    if let Some(boards) = boards
        && !boards.is_empty()
    {
        let mut loaded: Vec<Board> = Vec::new();
        let mut used_files: Vec<String> = Vec::new();
        for element in boards.iter() {
            // Java：`if (!element.isJsonObject()) continue;`
            let Json::Obj(entry) = element else {
                continue;
            };
            let recorded = text_value(entry, "file");
            // 作者层里的文件是可手改的文本，所以这个名字在磁盘布局上属于不可信输入，
            // 与 zip 条目同级：一个未经校验的 "../../mod.hjson" 曾让下一次保存覆盖模组
            // 自己的 meta 文件。
            let recorded_file = if is_safe_board_file(recorded.as_deref()) {
                recorded.clone().unwrap_or_default()
            } else {
                unique_board_file(Some("main.canvas"), &used_files)
            };
            let file = unique_board_file(Some(recorded_file.as_str()), &used_files);
            used_files.push(file.clone());
            let canvas_file = root.join(AUTHORING_DIR).join(BOARDS_DIR).join(&file);
            let id = text_value(entry, "id").unwrap_or_else(|| uid("b"));
            let mut board = if canvas_file.exists() {
                json_canvas::from_canvas(&read_file(&canvas_file)?, &file, &id)
            } else {
                Board::default()
            };
            board.file = file;
            if let Some(name) = text_value(entry, "name")
                && !is_blank(&name) {
                    board.name = name;
                }
            if let Some(viewport) = object_value(entry, "viewport") {
                board.viewport.x = number_value(viewport, "x", board.viewport.x);
                board.viewport.y = number_value(viewport, "y", board.viewport.y);
                let zoom = number_value(viewport, "zoom", board.viewport.zoom);
                board.viewport.zoom = if zoom > 0.0 { zoom } else { 1.0 };
            }
            if let Some(edge_style) = text_value(entry, "edgeStyle")
                && !is_blank(&edge_style) {
                    board.edge_style = edge_style;
                }
            loaded.push(board);
        }
        project.boards = loaded;
    }
    Ok(())
}

/// 一条记录在作者层里的身份：它自己的文件路径；嵌套内容则是「宿主路径 + # + 宿主内位置」。
/// 对应 Java `ProjectIo.recordKey`。
fn record_key(project: &ModProject, index: usize) -> Option<String> {
    let record = &project.contents[index];
    if record.is_inline() {
        let owner = record
            .inline_owner_id
            .as_deref()
            .and_then(|id| project.content_by_id(id));
        let owner_path = owner.and_then(|owner| owner.raw_path.as_deref())?;
        // Java 直接拼接 `inlinePath`，为 null 时会得到字面量 "null"；内联记录必定带路径。
        return Some(format!("{owner_path}#{}", record.inline_path.as_deref()?));
    }
    record.raw_path.clone()
}

/// 对应 Java `ProjectIo.text`：只认基本类型，其余（含 null）一律当作没有这个键。
///
/// 与 Java 唯一的差别：Gson 对布尔值调 `getAsString()` 会抛 `UnsupportedOperationException`
/// （由 `open` 吞成「作者层整体忽略」的警告），这里把它视作「该键没有值」——两者都不丢数据，
/// 而布尔值出现在这些键上只可能是用户手改 project.json 造成的。
fn text_value(object: &Object, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Json::Str(text)) => Some(text.clone()),
        Some(Json::Num(text)) => Some(text.clone()),
        _ => None,
    }
}

/// 对应 Java `ProjectIo.object`。
fn object_value<'a>(object: &'a Object, key: &str) -> Option<&'a Object> {
    match object.get(key) {
        Some(Json::Obj(inner)) => Some(inner),
        _ => None,
    }
}

/// 对应 Java `ProjectIo.array`。
fn array_value<'a>(object: &'a Object, key: &str) -> Option<&'a Vec<Json>> {
    match object.get(key) {
        Some(Json::Arr(items)) => Some(items),
        _ => None,
    }
}

/// 对应 Java `ProjectIo.number`：非基本类型用兜底值，解析不出有限 double 也用兜底值
/// （Java 的 `getAsDouble()` 对布尔与非数字字符串抛异常，正好被同一个 catch 吸收）。
fn number_value(object: &Object, key: &str, fallback: f64) -> f64 {
    let raw = match object.get(key) {
        Some(Json::Num(raw)) => raw,
        Some(Json::Str(raw)) => raw,
        _ => return fallback,
    };
    match raw.parse::<f64>() {
        Ok(parsed) if parsed.is_finite() => parsed,
        _ => fallback,
    }
}

/// 这条记录本来该在的位置。对应 Java `ProjectIo.canonicalContentPath`。
fn canonical_content_path(record: &ContentRecord) -> String {
    format!(
        "content/{}/{}.{}",
        record.ctype.folder_name(),
        record.name,
        record.ext
    )
}

/// 从 bundle 路径里取出语言标签。对应 Java `ProjectIo.bundleLocale`。
fn bundle_locale(path: &str) -> String {
    let file_name = &path[path.rfind('/').map_or(path.len(), |slash| slash + 1)..];
    let locale = locale_of(file_name);
    if locale.is_empty() {
        "en".to_string()
    } else {
        locale
    }
}

/// Java 的 `String.isBlank()`：全为空白（包括空串）时为真。
fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

/// 把解析失败包成 `io::Error`：Java 侧那是个会被 `open` 吞掉的 `RuntimeException`，
/// 在 Rust 里与 IO 故障走同一条通道。
fn invalid_data(failure: mm_json::HjsonError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, failure)
}

/// Java `Path.normalize()` 的词法版本：丢掉 `.`，用 `x/..` 相互抵消；根路径上多余的 `..` 被丢弃，
/// 相对路径开头的 `..` 保留（`../a` 仍是 `../a`）。不碰磁盘。
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut head = std::ffi::OsString::new();
    let mut stack: Vec<std::ffi::OsString> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => head.push(prefix.as_os_str()),
            Component::RootDir => head.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(name) => stack.push(name.to_os_string()),
            Component::ParentDir => {
                // 已经全是 `..` 的前缀不能再自我抵消，否则 `../../a` 会被削成 `a`。
                if stack.last().is_some_and(|name| name != "..") {
                    stack.pop();
                } else if head.is_empty() {
                    stack.push(std::ffi::OsString::from(".."));
                }
                // 有根却没得弹：Java 一样把这个 `..` 丢掉。
            }
        }
    }
    let mut normalized = PathBuf::from(head);
    for name in stack {
        normalized.push(name);
    }
    normalized
}

/// Java `Path.toAbsolutePath().normalize()`：相对路径按当前工作目录展开，再做词法归一化。
fn absolute_normalized(path: &Path) -> PathBuf {
    match std::path::absolute(path) {
        Ok(absolute) => lexical_normalize(&absolute),
        // 空路径之类展开不了的输入，Java 也是直接抛，这里保持词法形式即可。
        Err(_) => lexical_normalize(path),
    }
}

/// Java `Path.startsWith`：Windows 上按不敏感方式逐组件比较（Java 的 `WindowsPath` 正是如此），
/// 其余平台逐字节比较。这里的不敏感只用 `to_lowercase()`，对模组里的 ASCII 名字与 Java 等价。
fn path_starts_with(path: &Path, base: &Path) -> bool {
    let mut given = path.components();
    let mut prefix = base.components();
    loop {
        match (prefix.next(), given.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(base_part), Some(path_part)) => {
                if !component_eq(base_part, path_part) {
                    return false;
                }
            }
        }
    }
}

fn component_eq(left: Component<'_>, right: Component<'_>) -> bool {
    if cfg!(windows) {
        left.as_os_str()
            .to_string_lossy()
            .to_lowercase()
            == right
                .as_os_str()
                .to_string_lossy()
                .to_lowercase()
    } else {
        left == right
    }
}

/// Java `Path.equals` 的语义（同样带 Windows 的不敏感比较），用于已归一化的路径。
fn path_eq(left: &Path, right: &Path) -> bool {
    let mut a = left.components();
    let mut b = right.components();
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) => {
                if !component_eq(x, y) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Java `Path.relativize`：公共前缀之后的每一层补一个 `..`，再接上目标剩下的部分。
fn java_relativize(base: &Path, target: &Path) -> PathBuf {
    let given: Vec<Component<'_>> = base.components().collect();
    let other: Vec<Component<'_>> = target.components().collect();
    let mut common = 0;
    while common < given.len() && common < other.len() && component_eq(given[common], other[common])
    {
        common += 1;
    }
    let mut relative = PathBuf::new();
    for _ in common..given.len() {
        relative.push("..");
    }
    for part in &other[common..] {
        relative.push(part.as_os_str());
    }
    relative
}

/// Java `path.toString().replace('\\', '/')`：作者层与 `rawPath` 一律用正斜杠保存。
fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 一个用完即删的临时项目目录（`temp_case` 自己不删，避免残留拖垮后续基线）。
    struct Case {
        root: PathBuf,
    }

    impl Case {
        fn new(tag: &str) -> Case {
            Case {
                root: crate::files::scratch::temp_case(tag),
            }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Case {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// 与 `importer.rs` 的测试同一套办法：真实 schema 目录在就用来建注册表，不在就跳过该用例。
    fn load_registry() -> Option<SchemaRegistry> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../Mindustry-modmaker/schemas");
        if !dir.join("fields.json").exists() {
            return None;
        }
        SchemaRegistry::load(&dir).ok()
    }

    #[test]
    fn safe_resolve_blocks_escapes() {
        let root = Path::new(if cfg!(windows) { r"C:\proj\mod" } else { "/proj/mod" });
        assert!(safe_resolve(root, "../evil.hjson").is_none());
        assert!(safe_resolve(root, "content/../../evil.hjson").is_none());
        assert!(safe_resolve(root, "").is_none());
        assert!(safe_resolve(root, "content/turret/x.hjson").is_some());
    }

    #[test]
    // 第二次 push 依赖第一次的结果（重名时加序号），vec![] 初始化无法自引用，保持逐次 push。
    #[allow(clippy::vec_init_then_push)]
    fn board_file_names_stay_inside_boards() {
        assert!(is_safe_board_file(Some("main.canvas")));
        assert!(!is_safe_board_file(Some("../../mod.hjson")));
        assert!(!is_safe_board_file(Some("bundle_zh_CN.properties")));
        assert!(!is_safe_board_file(Some("CON")));
        assert!(!is_safe_board_file(None));
        let mut used: Vec<String> = Vec::new();
        used.push(unique_board_file(Some("main.canvas"), &used));
        used.push(unique_board_file(Some("main.canvas"), &used));
        assert_eq!(used[1], "main-2.canvas");
    }

    #[test]
    fn save_does_not_deadlock_the_lock() {
        // `save` 内部会调 `write`；Java 的监视器可重入，Rust 的 `Mutex` 不可重入，
        // 因此持锁入口必须走不加锁的 `write_unchecked`。这里用带时限的通道等结果，
        // 真自锁了就是这条断言失败，而不是整个测试进程永久挂住。
        let case = Case::new("lock");
        let root = case.path().to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker_root = root.clone();
        std::thread::spawn(move || {
            let mut project = crate::project_factory::create("lock", &worker_root).unwrap();
            let outcome = save(&mut project, &worker_root);
            let _ = sender.send(outcome.is_ok());
        });
        let finished = receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap_or(false);
        assert!(finished, "save 自死锁或失败：锁只应由公开入口取一次");
    }

    #[test]
    fn save_then_open_round_trips() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let case = Case::new("roundtrip");
        let root = case.path().to_path_buf();
        let created = crate::project_factory::create("roundtrip", &root).unwrap();
        let reopened = open(&root, &registry).unwrap();
        assert_eq!(reopened.meta.name, created.meta.name);
        assert_eq!(reopened.boards.len(), 1);
        assert_eq!(reopened.boards[0].file, created.boards[0].file);
        assert_eq!(reopened.authoring_warning, None);
    }

    fn write_bytes(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("建父目录");
        }
        fs::write(&path, bytes).expect("写夹具文件");
    }

    /// 快照：模组目录里除作者层（`modmaker/`，它本来就是编辑器自己的产物）之外的每个文件。
    fn snapshot_realm(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut out = std::collections::BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).expect("读目录") {
                let path = entry.expect("目录项").path();
                let relative = to_slash(path.strip_prefix(root).expect("在根下"));
                if relative == AUTHORING_DIR || relative.starts_with(&format!("{AUTHORING_DIR}/")) {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.insert(relative, fs::read(&path).expect("读文件"));
                }
            }
        }
        out
    }

    /// A 项验收：一个**没被改过**的模组目录，打开 → 保存之后每个文件逐字节不变。
    /// 覆盖 BOM、CRLF、键序、注释与二进制资产——这正是「v1 字节保真」的全部承诺。
    #[test]
    fn an_untouched_project_saves_back_byte_for_byte() {
        let registry = match load_registry() {
            Some(registry) => registry,
            None => return,
        };
        let case = Case::new("byte-fidelity");
        let root = case.path().to_path_buf();

        // meta：BOM + CRLF + 非字母序键。
        let meta = "\u{feff}{\r\n  \"name\": \"byteme\",\r\n  \"displayName\": \"Byte Me\",\r\n  \
                    \"author\": \"t\",\r\n  \"version\": \"1.0\",\r\n  \"minGameVersion\": \"160\"\r\n}\r\n";
        write_bytes(&root, "mod.json", meta.as_bytes());
        // HJSON：注释、手写排版、CRLF。
        let item = "// 作者手写注释，保存后必须原样在\r\nname: byteme-item\r\nhardness: 3\r\ncolor: \"ffaa00\"\r\n";
        write_bytes(&root, "content/items/byteme-item.hjson", item.as_bytes());
        // JSON：键序刻意非字母序，CRLF 行尾。
        let block = "{\r\n  \"name\": \"byteme-wall\",\r\n  \"size\": 2,\r\n  \"health\": 120,\r\n  \"requirements\": []\r\n}\r\n";
        write_bytes(&root, "content/blocks/byteme-wall.json", block.as_bytes());
        // 二进制资产：不是合法 UTF-8，但必须按字节原样留住。
        let png: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0xFE, 0x00, 0x7F];
        write_bytes(&root, "sprites/byteme-wall.png", &png);

        let before = snapshot_realm(&root);
        assert_eq!(before.len(), 4, "夹具应当有 4 个文件：{before:?}");

        let mut project = open(&root, &registry).expect("打开夹具模组");
        assert_eq!(project.authoring_warning, None);
        assert_eq!(project.contents.iter().filter(|r| !r.is_inline()).count(), 2);
        save(&mut project, &root).expect("未修改保存");

        let after = snapshot_realm(&root);
        assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
        for (path, bytes) in &before {
            assert_eq!(
                after.get(path),
                Some(bytes),
                "未修改保存改写了 {path}（{} → {} 字节）",
                bytes.len(),
                after.get(path).map(|b| b.len()).unwrap_or(0)
            );
        }
        assert!(!project.meta_dirty, "保存后 meta 不该仍是脏的");
        assert!(project.contents.iter().all(|record| !record.dirty || record.is_inline()));
    }
}
