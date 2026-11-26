//! `Autosave.java` 的等价移植（`dev.modmaker.core.io.Autosave`）：崩溃恢复用的定期快照。
//!
//! Java 是一个 `final` 类加一个私有构造器，全部成员都是 `static`；Rust 里对应成模块级
//! `pub fn`，私有常量 `MARKER` 对应私有 `const`。
//!
//! 崩溃恢复：把可编辑的文本状态（meta、content、bundles、boards）定期快照到
//! `modmaker/autosave/` 之下。这个目录本就待在作者目录里，而导出器和读包器早就忽略作者目录了，
//! 所以一次 autosave 既不会被打进构建产物，也不会被当成内容收养。
//!
//! 一次 autosave 自己就是一个完整的项目目录：[`write`] 用 [`project_io::plan`] 把一份分离快照
//! 以 autosave 目录为根规划出来，因此活体模型的脏标记永远不会被动到。[`restore`] 把那个目录当
//! 项目打开、再存回真实的根目录——复用的是同一次普通保存的那套 prune 逻辑，所以自上次 autosave
//! 以来被删掉的内容文件和 bundle 文件会被删掉，而不是留在盘上。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use mm_model::{ModMeta, ModProject};
use mm_schema::SchemaRegistry;

use crate::import::ImportError;
use crate::project::{AUTHORING_DIR, PROJECT_FILE};
use crate::project_io;

/// `Autosave.AUTOSAVE_DIR`：作者目录（`modmaker/`）之下存放最近一次 autosave 的目录名。
pub const AUTOSAVE_DIR: &str = "autosave";

/// `Autosave.MARKER`：一个标记文件，它的 mtime 就是 autosave 的时间戳；目录里其余部分是项目数据。
const MARKER: &str = ".autosave";

/// Java `System.lineSeparator()`：Windows 上是 `\r\n`，其他平台是 `\n`。marker 的内容是
/// 「毫秒十进制 + 这个分隔符」，不能写成裸 `\n`。
const LINE_SEPARATOR: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// 一个项目的 autosave 目录，无论它是否存在。对应 Java `Autosave.root(Path)`。
pub fn root(project_root: &Path) -> PathBuf {
    project_root.join(AUTHORING_DIR).join(AUTOSAVE_DIR)
}

/// 把当前可编辑状态写成一份自包含快照。参数必须是分离副本（[`ModProject::snapshot`]），
/// 这样 [`project_io::plan`]——它会更新模型上的 root 与脏标记——在用户编辑期间永远碰不到活体模型。
/// 对应 Java `Autosave.write(ModProject)`。
pub fn write(snapshot: &mut ModProject) -> io::Result<()> {
    // Java 这里直接读 `snapshot.root`，root 为 null 时抛 NullPointerException。Rust 的
    // `Option` 用同样「契约被违反就炸」的 panic 表达，不改写成返回 `io::Error`——那是行为变更。
    let target = match snapshot.root.as_ref() {
        Some(project_root) => root(project_root),
        None => panic!("Autosave.write 需要一个带 root 的快照，对应 Java project.root 为 null 时的 NullPointerException"),
    };
    // 与 Java 的顺序一致：`plan` 在 Java 里不是 `synchronized`，所以这里也不取锁；
    // 随后调的是加锁的公开 [`project_io::write`]（Java `ProjectIo.write` 是 `static synchronized`），
    // 绝不走私有的 `write_unchecked`。它返回时锁已经释放，之后才写 marker。
    let planned = project_io::plan(snapshot, &target)?;
    project_io::write(&planned)?;
    // marker 就是一个普通文件写入：Java 用 `Files.writeString`（CREATE+TRUNCATE+WRITE），
    // 所以这里用 `fs::write`，不走 AtomicFiles。
    let millis = SystemTime::UNIX_EPOCH
        .elapsed()
        // Java `System.currentTimeMillis()` 不会失败；只有系统时钟早于 epoch 才走到这个兜底，
        // 实际等价于不可达分支。截断语义与 Java 的 long 毫秒一致。
        .unwrap_or_default()
        .as_millis() as i64;
    fs::write(target.join(MARKER), format!("{millis}{LINE_SEPARATOR}"))
}

/// 快照是否存在（任何一份）。对应 Java `Autosave.exists(Path)`。
pub fn exists(project_root: &Path) -> bool {
    let autosave = root(project_root);
    // 这两层 `modmaker/project.json` 不是笔误，是照抄 Java 的
    // `root(projectRoot).resolve(AUTHORING_DIR).resolve(PROJECT_FILE)`：一次 autosave 本身
    // 就是一个完整项目目录（`write` 把 `plan` 的根设在 autosave 目录上），所以 autosave 目录里
    // 必然还要再套一层作者目录，即 `<root>/modmaker/autosave/modmaker/project.json`。
    // 后来人不要把它「修好」成一层。
    autosave.is_dir() && autosave.join(AUTHORING_DIR).join(PROJECT_FILE).exists()
}

/// 最近一次 autosave 的 mtime，没有就是 0。marker 是最后写的那个，所以它的时间戳就是
/// 快照完成的那一刻。对应 Java `Autosave.lastModified(Path)`。
pub fn last_modified(project_root: &Path) -> i64 {
    let marker = root(project_root).join(MARKER);
    // Java 把整个表达式包在 `try/catch(IOException)` 里：文件不存在给 0，读不到时间也给 0。
    if !marker.exists() {
        return 0;
    }
    file_mtime(&marker).unwrap_or(0)
}

/// 项目自己被最后一次保存的时间：先看作者层，没有就退到 meta 文件。
/// 对应 Java `Autosave.projectSaveTime(Path)`。
pub fn project_save_time(project_root: &Path) -> i64 {
    let project_file = project_root.join(AUTHORING_DIR).join(PROJECT_FILE);
    // Java 的 `catch` 是吞掉异常的（注释写着「fall through to 'no project on disk yet'」），
    // 但那次 `try` 包住了下面整段查找，所以任何一次读时间失败都直接跳到 catch、最终返回 0，
    // 而不是继续往后找。这里用 `unwrap_or(0)` 提前返回，保持同一个落点。
    if project_file.exists() {
        return file_mtime(&project_file).unwrap_or(0);
    }
    for name in ModMeta::META_FILES {
        let meta = project_root.join(name);
        if meta.exists() {
            return file_mtime(&meta).unwrap_or(0);
        }
    }
    0
}

/// autosave 里是否装着比项目最后一次保存更新的东西。对应 Java `Autosave.isNewerThan(Path)`。
pub fn is_newer_than(project_root: &Path) -> bool {
    last_modified(project_root) > project_save_time(project_root)
}

/// 把 autosave 恢复覆盖到项目目录上，并返回恢复出来的模型，已经可以直接发布。
/// 资产（贴图、音效、地图）永远不会被 save 写出，所以 autosave 不携带它们；它们从真实目录里
/// 重读，而 restore 不会碰那个目录。对应 Java `Autosave.restore(Path, SchemaRegistry)`。
///
/// 签名偏离说明：Java 声明的是 `throws IOException`，而本仓库的 [`project_io::open`] 已定成
/// 返回 `Result<_, ImportError>`；`ImportError::Io` 就是 Java 的 `IOException` 那条通道
/// （见 [`crate::import::cancelled`]），所以这里统一用 `ImportError`，[`project_io::save`]
/// 与 [`discard`] 的 `io::Error` 经 `From<io::Error>` 折成 `ImportError::Io`。
pub fn restore(project_root: &Path, registry: &SchemaRegistry) -> Result<ModProject, ImportError> {
    let mut recovered = project_io::open(&root(project_root), registry)?;
    let base = project_io::open(project_root, registry)?;
    // 整体替换（Java `recovered.assets = base.assets;`），不是合并、不是追加：
    // 资产不由 save 写出，因此从真实目录重读。
    recovered.assets = base.assets;
    // `project_io::save` 取锁、放锁都在它自己函数体内完成，返回时锁已释放，
    // 所以后面的 marker 删除（`discard`）不会被保存锁罩住——与 Java 的顺序一致。
    project_io::save(&mut recovered, project_root)?;
    discard(project_root)?;
    Ok(recovered)
}

/// 删掉 autosave 目录，如果有的话。手动保存之后或用户显式丢弃时使用。
/// 对应 Java `Autosave.discard(Path)`。
pub fn discard(project_root: &Path) -> io::Result<()> {
    let autosave = root(project_root);
    if !autosave.exists() {
        return Ok(());
    }
    // Java：`Files.walk(autosave).sorted(Comparator.reverseOrder()).forEach(deleteIfExists)`，
    // 也就是深度优先的后序删除——子先于父，最后连 autosave 目录自己也删掉。
    let mut paths = Vec::new();
    collect_tree(&autosave, &mut paths)?;
    // `Path` 的序按组件比较，父目录是子路径的前缀因此更小，倒序之后子必然排在父前面。
    paths.sort();
    paths.reverse();
    for path in paths.iter() {
        // 逐条删除、失败一律忽略：Java 注释写明「删不掉一个被锁住的文件是尽力而为，
        // 下一次保存会覆盖它」。
        let is_dir = fs::symlink_metadata(path)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false);
        let _ = if is_dir {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        };
    }
    Ok(())
}

/// Java `Files.walk(autosave)` 的默认语义：不跟随符号链接，起点目录自己也在结果里。
/// 读目录失败会往上抛，对应 Java 在流遍历时抛出、最终包成 `UncheckedIOException`/`IOException`
/// 传出 `discard` 的实际行为。
fn collect_tree(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    out.push(dir.to_path_buf());
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_tree(&entry.path(), out)?;
        } else {
            out.push(entry.path());
        }
    }
    Ok(())
}

/// Java `Files.getLastModifiedTime(p).toMillis()`。读不到（Java 抛 `IOException`）时返回 `None`，
/// 由调用方按 Java 的 `catch` 分支折成 0。时间早于 epoch（Rust 的 `duration_since` 报错、
/// Java 侧几乎不会发生）同样按「读不到」处理，与 `catch` 给 0 一致。
fn file_mtime(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64)
}
