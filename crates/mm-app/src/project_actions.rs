//! `ProjectActions` 的等价移植（Java `dev.modmaker.ui.ProjectActions`，164 行）。
//!
//! 对应 Java 类注释 `ProjectActions.java:22-27`：这一半是**无状态**的编排——从盘上读目录和包、
//! 驱动共享的构建流水线。它被拆出 [`ProjectController`](crate::project_controller) 是为了让那个类
//! 只留下「活的模型 + 编辑命令 + 视图通知」；这一半从不碰视图，可以在文件工作线程上跑。
//!
//! Java 把它做成持有 `registry` 与 `pipeline` 两个字段（`:29-36`）的类，是因为它要给
//! `readProject`／`readImport`／`planBuild`／`writeBuild*` 少写参数。Rust 侧按 `mm_io` 已成型的
//! 风格做成**自由函数 + 显式传参**（那里 `project_io::open`、`project_factory::create` 都是这个形状），
//! 于是「无状态」这件事表达得更直接：没有任何东西可持，也就没有生命周期问题。
//!
//! 本文件三族：读项目目录（`:40-42`）、读导入包（`:48-113`）、构建（`:115-134`），
//! 外加两个私有递归清理工具（`:136-163`）。
//!
//! ## 构建一族与 Java 的两处形态差异（M3 落地时登记）
//!
//! 1. **`pipeline` 字段（`:32`）不存在**。Java 在构造器里 `new BuildPipeline(registry)` 存成字段，
//!    Rust 的 [`BuildPipeline`] 借用了 registry（`BuildPipeline<'a>`，见
//!    `mm_io::export::build_pipeline` 模块头第 1 条），四个函数各自现造一个即可 ——
//!    `BuildPipeline` 本身零状态，现造与存字段在这条链上行为一致。
//! 2. **两个 `writeBuild*` 多一个 `project` 参数**。Java 的 `writeBuildFolder(plan, destination)`
//!    （`:126-129`）只要 plan 与目的地，因为它的 `ProjectIo.SavePlan` 内部攥着模型引用，回滚
//!    （`BuildPipeline.java:107`）不需要别的东西；Rust 的 [`SavePlan`] 不持模型，回滚必须拿到
//!    模型本体（这条差异已登记在 `mm-io/src/export/build_pipeline.rs` 模块头第 2 条），
//!    于是它传导到这里。传进去的就是调用方自己的那个项目，语义与 Java 隐式用的同一个对象一致。
//!
//! [`BuildPipeline`]: mm_io::export::build_pipeline::BuildPipeline
//! [`SavePlan`]: mm_io::project_io::SavePlan

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use mm_io::export::build_pipeline::{BuildPipeline, Plan};
use mm_io::export::export_report::ExportReport;
use mm_io::files::search_paths::normalize_absolute;
use mm_io::import::{ImportError, ImportProgress, ImportReport, NoProgress};
use mm_io::importer::ModImporter;
use mm_io::package::reader as mod_package_reader;
use mm_io::project_io;
use mm_model::ModProject;
use mm_schema::name_rules;
use mm_schema::SchemaRegistry;
use mm_validate::Issue;

/// 对应 Java 的 `record ImportResult(ModProject, Path, ImportReport)`（`:44-46`），
/// 类注释 `:44` 写明它是「一个从盘上读好并拆开的包，等着在模型的线程上发布」。
/// Java 用 record 的三个访问器，这里直接暴露三个公有字段（与 `ImportedProject` 同风格）。
pub struct ImportResult {
    /// 对应 `project()`。
    pub project: ModProject,
    /// 对应 `root()`：包实际落盘的项目目录（内部名非空时是 `destinationRoot/<internalName>`）。
    pub root: PathBuf,
    /// 对应 `report()`。
    pub report: ImportReport,
}

/// 一个导入尝试走到哪一步了，供 `read_import` 的 catch 分支决定怎么善后。
///
/// 对应 Java `readImport` 里 `try` 之前声明、`try` 之内改写的三个局部变量：`root`（`:61`，
/// 初值 `null` → `Option`）、`folderWasEmpty`（`:62`）、`unpackStarted`（`:63`）。
/// Java 靠「局部变量在 `try` 外声明」把它们带进 `catch`，Rust 的 `Result` 不是异常通道，
/// 于是把这三样收成一个结构体，由内层的 `attempt_import` 往里写、外层的 `read_import` 读。
struct Attempt {
    /// 对应 `root`：`None` 就是 Java 的 `null`（失败发生在 `:73` 赋值之前）。
    root: Option<PathBuf>,
    /// 对应 `folderWasEmpty`。
    folder_was_empty: bool,
    /// 对应 `unpackStarted`。
    unpack_started: bool,
}

/// 对应 Java `ProjectActions.readProject(Path root)`（`:40-42`）。
///
/// Java 的方法体只有一行 `return ProjectIo.open(root, registry);`，类注释 `:39` 写明
/// 「读一个项目目录但不发布它；调用方拥有返回的模型」——发布是
/// [`publish_project`](crate::project_controller::ProjectController::publish_project) 的事。
pub fn read_project(
    root: &Path,
    registry: &SchemaRegistry,
) -> Result<ModProject, ImportError> {
    project_io::open(root, registry)
}

/// 对应 Java `readImport(Path, Path, Predicate<Path>, Consumer<String>)`（`:48-51`）：
/// 不带进度回调的那一个重载，方法体就是把 `ImportProgress.NONE` 补进去再转调（`:50`）。
/// Rust 侧 `NoProgress`（`mm_io::import::NoProgress`）就是 Java 的 `ImportProgress.NONE`。
pub fn read_import(
    source: &Path,
    destination_root: &Path,
    registry: &SchemaRegistry,
    overwrite_approver: &mut dyn FnMut(&Path) -> bool,
    log: &mut dyn FnMut(&str),
) -> Result<ImportResult, ImportError> {
    read_import_with(
        source,
        destination_root,
        registry,
        &mut NoProgress,
        overwrite_approver,
        log,
    )
}

/// 对应 Java `readImport(Path, Path, ImportProgress, Predicate<Path>, Consumer<String>)`
/// （`:59-113`），含方法注释 `:53-58`：读一个模组包（zip 或目录）并拆到一个项目目录里，
/// **不**发布它；每个阶段向 `progress` 汇报，并在文件之间尊重它的取消——写入阶段已经开始之后
/// 才取消，若项目目录是这次导入新建的就把它整个删掉，而用户同意覆写的既有目录保持原样。
///
/// Java 从字段 `registry`（`:31`）取注册表；Rust 侧按本文件的自由函数风格显式传参。
pub fn read_import_with(
    source: &Path,
    destination_root: &Path,
    registry: &SchemaRegistry,
    progress: &mut dyn ImportProgress,
    overwrite_approver: &mut dyn FnMut(&Path) -> bool,
    log: &mut dyn FnMut(&str),
) -> Result<ImportResult, ImportError> {
    // `:61-63` 的 `Path root = null; boolean folderWasEmpty = false; boolean unpackStarted = false;`
    let mut attempt = Attempt {
        root: None,
        folder_was_empty: false,
        unpack_started: false,
    };
    // `:64` 的 `try { ... }`
    let outcome = attempt_import(
        source,
        destination_root,
        registry,
        progress,
        overwrite_approver,
        log,
        &mut attempt,
    );
    let Err(err) = outcome else { return outcome };
    // `:98` 的 `catch (ImportCancelledException cancelled)`：只有取消才走这段善后，
    // 其余错误（覆写被拒、名字不安全、读写失败）Java 不捕获，Rust 也原样抛回。
    if !matches!(err, ImportError::Cancelled(_)) {
        return Err(err);
    }
    if attempt.unpack_started {
        if attempt.folder_was_empty {
            // `:100-104`：目录在导入之前不存在（或空着），取消掉的写入不能留下一个
            // 用户从没留下的半成品项目。
            delete_tree_quietly(attempt.root.as_deref());
            let shown = display(attempt.root.as_deref());
            log(&format!("import cancelled: removed the half-written project folder {shown}"));
        } else {
            // `:106`
            let shown = display(attempt.root.as_deref());
            log(&format!("import cancelled: kept the existing files in {shown}"));
        }
    } else {
        // `:109`
        log(&format!("import cancelled: {}", source.display()));
    }
    Err(err)
}

/// `read_import_with` 的 `try` 体（`:65-97`），把进度写进 `attempt`。
///
/// 拆成独立函数是 Rust 没有 `try/catch` 的必然结果：`?` 一走就跳出了函数，
/// 想让外层看到「走到哪一步」就只能把状态提成出参。
fn attempt_import(
    source: &Path,
    destination_root: &Path,
    registry: &SchemaRegistry,
    progress: &mut dyn ImportProgress,
    overwrite_approver: &mut dyn FnMut(&Path) -> bool,
    log: &mut dyn FnMut(&str),
    attempt: &mut Attempt,
) -> Result<ImportResult, ImportError> {
    // `:65` `ModPackage pkg = ModPackageReader.read(source, progress);`
    let mut pkg = mod_package_reader::read_with(source, progress)?;
    // `:66` `new ModImporter(registry).importPackage(pkg, progress)`
    // `ImportedProject.project` 是公有字段；`unpack_with` 要 `&mut` 它，故这里声明 `mut`。
    let mut result = ModImporter::new(registry).import_package_with(&mut pkg, progress)?;
    // `:67-72` 一个带分隔符的 meta 名字会把拆出来的项目放到所选目标之外——宁可拒绝导入，
    // 也不相信这个名字。
    let internal = result.project.internal_name();
    if !internal.is_empty() && !name_rules::is_folder_safe(&internal) {
        return Err(ImportError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the mod's name is not a safe folder name: '{internal}'"),
        )));
    }
    // `:73`
    let root = if internal.is_empty() {
        destination_root.to_path_buf()
    } else {
        destination_root.join(internal)
    };
    // `:74-77` 的注释原样保留其意：把一个目录导入它自己（或导入已经装着它的那个目录），
    // 等于把源复制进自己的子树——一次目标就是模组目录本身的文件夹导入，会在模组里留下
    // `<mod>/<internalName>/` 一份拷贝，悄无声息地把整棵树翻倍。动手写之前先拒绝这种重叠。
    // `:78` 的 `Files.isDirectory(source)`：zip 导入不查，因为源根本不是目录。
    if source.is_dir() {
        // `:79-81` 两侧都做 `toAbsolutePath().normalize()`，比较前缀时补上分隔符，
        // 于是 `…\mod` 不会被 `…\mod2` 前缀误伤。
        let source_path = normalize_absolute(source).to_string_lossy().into_owned();
        let target_path = normalize_absolute(&root).to_string_lossy().into_owned();
        let separator = std::path::MAIN_SEPARATOR;
        // Java 的 `equalsIgnoreCase` 按字符做大小写折叠；临时目录里的模组路径都是 ASCII，
        // `eq_ignore_ascii_case` 与它在这里逐位一致。
        if source_path.eq_ignore_ascii_case(&target_path)
            || source_path.starts_with(&format!("{target_path}{separator}"))
            || target_path.starts_with(&format!("{source_path}{separator}"))
        {
            return Err(ImportError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "refusing to import '{source_path}' into '{target_path}': the project folder \
                     would sit inside its own source and copy the mod into itself"
                ),
            )));
        }
    }
    // `:90`
    attempt.folder_was_empty = !root.is_dir() || is_directory_empty(&root);
    // `:91-94` 覆写没被批准：先记一行日志，再抛「import cancelled」。
    // 这一条 Java 抛的是普通 `IOException`（不是 `ImportCancelledException`），
    // 所以它**不**进 `:98` 的 catch，也就不会删掉既有目录——上面的用例正是盯着这一点。
    if !attempt.folder_was_empty && !overwrite_approver(&root) {
        // `:92` 与 `:93` 打的是同一句话，日志一行、异常一条。
        let message = format!("import cancelled: {} already exists", root.display());
        log(&message);
        return Err(ImportError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            message,
        )));
    }
    // `:95-96`
    attempt.unpack_started = true;
    attempt.root = Some(root.clone());
    project_io::unpack_with(&mut result.project, &mut pkg, &root, progress)?;
    // `:97`
    Ok(ImportResult {
        project: result.project,
        root,
        report: result.report,
    })
}

/// 对应 Java `planBuild(ModProject project)`（`:115-118`）：构建的**模型那一半**——跑校验、
/// 定下这次保存、抓打包器要用的值。方法体只有一行 `return pipeline.plan(project);`（`:117`），
/// 真正的流程在 [`BuildPipeline`] 里，CLI 也走同一条（`BuildPipeline.java:15-27` 的类注释就是
/// 这句话：两边从前各自实现过一遍，改一处要改两处，所以收拢成一条流程）。
///
/// **必须跑在持有模型的线程上**（`:115` 上方 `ProjectController.java:2023-2027` 的注释写明
/// 「Must run on the FX thread, which owns the model」），Rust 侧由 `&mut ModProject` 参数保证。
pub fn plan_build(project: &mut ModProject, registry: &SchemaRegistry) -> io::Result<Plan> {
    BuildPipeline::new(registry).plan(project) // :117
}

/// 对应 Java `planBuild(ModProject project, List<Issue> issues)`（`:120-123`）：
/// 用**已经算好的**发现来构建，`:120` 的注释写明理由——「findings already computed from a
/// snapshot, so rules do not run twice」，即 UI 在 FX 线程之外校验过快照，这一趟不该重跑。
pub fn plan_build_with(
    project: &mut ModProject,
    registry: &SchemaRegistry,
    issues: Vec<Issue>,
) -> io::Result<Plan> {
    BuildPipeline::new(registry).plan_with(project, issues) // :122
}

/// 对应 Java `writeBuildFolder(BuildPipeline.Plan plan, Path destination)`（`:125-129`）：
/// 构建的**磁盘那一半**——纯文件工作，所以工作线程可以跑它（`:125` 注释原话）。
/// 它先保存工程再打包产物，保存失败会把未保存标记滚回去（`BuildPipeline.java:99-113`）。
///
/// 与 Java 的形态差异见模块头第 2 点：多一个 `project`，只为回滚拿得到模型本体。
pub fn write_build_folder(
    project: &mut ModProject,
    registry: &SchemaRegistry,
    plan: &Plan,
    destination: &Path,
) -> io::Result<ExportReport> {
    BuildPipeline::new(registry).write(project, plan, destination, false) // :128
}

/// 对应 Java `writeBuildZip(BuildPipeline.Plan plan, Path zipFile)`（`:131-134`）：
/// zip 产物的磁盘那一半，`zip` 实参传 `true`。形态差异同 [`write_build_folder`]。
pub fn write_build_zip(
    project: &mut ModProject,
    registry: &SchemaRegistry,
    plan: &Plan,
    zip_file: &Path,
) -> io::Result<ExportReport> {
    BuildPipeline::new(registry).write(project, plan, zip_file, true) // :133
}

/// 对应 Java `deleteTreeQuietly(Path root)`（`:136-153`）：把一个被取消的导入刚建出来的
/// 目录尽力递归删掉。
///
/// Java 用 `Files.walk(root).sorted(reverseOrder())` 先叶后根；Rust 标准库没有 walk，
/// 就用递归达到同一个顺序——**先**把子项处理干净，**再**动自己。`:144-147` 的
/// `catch (IOException locked)` 对应这里对 `remove_*` 错误不加理会：单个被占用的文件留在原地，
/// 半成品树的其余部分照删；`:150-152` 的 `catch (IOException unreachable)` 对应
/// 连目录都读不开时同样什么都不做。
fn delete_tree_quietly(root: Option<&Path>) {
    // `:138` `if (root == null || !Files.exists(root)) return;`
    let Some(root) = root else { return };
    if !root.exists() {
        return;
    }
    delete_children_then_self(root);
}

fn delete_children_then_self(path: &Path) {
    // 读不开就当没有子项（`:150` 的 unreachable 分支），下面仍会试着删它本身。
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            delete_children_then_self(&entry.path());
        }
    }
    // `:143` `Files.deleteIfExists(path)`：文件与目录分开试，因为标准库没有「看类型再删」。
    if fs::remove_file(path).is_err() {
        let _ = fs::remove_dir(path);
    }
}

/// 对应 Java `isDirectoryEmpty(Path directory)`（`:155-163`）。
///
/// `:158-161` 的理由照搬：**读不开的目录算「非空」**——把它报成空会跳过整个覆写确认，
/// 于是一个被锁住的目标目录就被静默覆写了。
fn is_directory_empty(directory: &Path) -> bool {
    match fs::read_dir(directory) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}

/// `Option<&Path>` 的 `null` 打印：Java 里 `"…" + root` 在 `root == null` 时打出 `null`。
fn display(root: Option<&Path>) -> String {
    match root {
        Some(path) => path.display().to_string(),
        None => "null".to_string(),
    }
}
