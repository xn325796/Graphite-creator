//! `ModExporter.java` 的等价移植（`dev.modmaker.core.io.ModExporter`，313 行）：
//! 把一个项目导出成一个可安装的 mod。
//!
//! 两种产物：
//! - **解开的目录树**，正是 Mindustry 在 `mods/` 里所期望的东西，改完立刻能重载；
//! - **`.zip`**，用来分享与发布。
//!
//! 两种产物都只带 mod 真正拥有的东西（Java `isModPath`，即本文件的 `is_mod_path`），并尊重项目
//! `.gitignore` 里已有的排除规则（经由读包器那份保守的噪声清单，见 `crate::package::reader`）。
//!
//! 导出**永远不修改**源项目。项目内部不允许存在构建产物——那会让每次构建都自我嵌套、并把上一次
//! 构建的结果重新导入进来——所以 Java `requireOutside`（本文件的 `require_outside`）会先拒绝这种目标。
//!
//! # 锁
//!
//! Java 的 `ModExporter` **一个 `synchronized` 方法都没有**（第 34-313 行的每个成员都是
//! `public static` / `private static`，无监视器），而且它全程只读源项目、只往项目目录**之外**写
//! （`requireOutside` 就是那条保证），也**不调用** `ProjectIo.save` / `ProjectIo.write`。所以 Rust
//! 侧这里**不取** `project_io::SAVE_LOCK`，与 Java 一致：取锁反而会引入 Java 没有的串行化。
//! `AtomicFiles` 只在自己的临时名（`.mm-tmp-*`）上操作，那条路径由 `temp_for` 保证不与
//! `SAVE_LOCK` 保护的写入相冲。判定过程写在这里，后来人不要「顺手补一把锁」。
//!
//! # 路径 helper 的复用
//!
//! Java 的 `relative()`（第 310-312 行）等价于 `ProjectIo` 里那套词法路径比较，而
//! `project_io.rs` 是冻结文件、其中 `lexical_normalize` / `absolute_normalized` /
//! `path_starts_with` / `component_eq` / `java_relativize` / `path_eq` 都是**模块私有**。
//! 本步不允许改动 `project_io.rs`，故在下面**逐字符照抄同一套算法**（不另起炉灶、不调低比较
//! 条件），只把可见性放到 `pub(super)`，让同目录的 `build_manifest.rs` 共用一份实现。

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use mm_model::{ModMeta, ModProject};
use mm_schema::name_rules::is_folder_safe;
use mm_validate::Issue;

use super::build_manifest::civil_from_days;
use super::export_report::ExportReport;
use crate::files::atomic_files;
use crate::package::reader::is_noise;
use crate::project::{AUTHORING_DIR, TEMP_SUFFIX};

/// `ModExporter.MOD_DIRECTORIES`：Java 是 `Set.of(...)`（只用来做 `contains`，从不迭代），
/// 这里用定长数组 + 线性 `contains`，成员与顺序无关，语义一致。
const MOD_DIRECTORIES: [&str; 9] = [
    "content",
    "bundles",
    "sprites",
    "sprites-override",
    "sounds",
    "music",
    "scripts",
    "maps",
    "schematics",
];

/// `ModExporter.FIXED_TIME`（Java 第 34-36 行）：固定时间戳，让同一项目的重复构建产出完全相同的包。
/// Java 写作 `LocalDateTime.of(2020,1,1,0,0).toInstant(ZoneOffset.UTC).toEpochMilli()`，
/// 也就是 2020-01-01T00:00:00Z = 1_577_836_800_000 毫秒。
const FIXED_TIME: i64 = 1_577_836_800_000;

/// `java.io.File.separatorChar`（Java 在 `pruneStale` 第 91 行用到）。
const FILE_SEPARATOR_CHAR: char = if cfg!(windows) { '\\' } else { '/' };

/// `java.io.File.separator`（Java 在 `requireOutside` 第 299 行以字符串拼接的方式用到）。
const FILE_SEPARATOR: &str = if cfg!(windows) { "\\" } else { "/" };

/// `Deflater.BEST_COMPRESSION`（Java 第 140 行 `out.setLevel(...)`）。
const BEST_COMPRESSION: i64 = 9;

/// Java `buildFolder(ModProject, Path, List<Issue>)`（第 42-45 行）。
///
/// Java 的 `List<Issue> issues` **可以为 null**（第 76 行与第 175 行都有
/// `issues == null ? List.of() : issues` 这个分支），Rust 用 `Option<&[Issue]>` 表达这个可空性，
/// 好把那条静默分支原样保留下来。Java 里 `IOException` 的落点，Rust 全部返回 `io::Result`。
pub fn build_folder(
    project: &ModProject,
    destination: &Path,
    issues: Option<&[Issue]>,
) -> io::Result<ExportReport> {
    build_folder_root_internal(
        &require_root(project)?,
        &require_internal_name(project)?,
        destination,
        issues,
    )
}

/// Java `buildFolder(Path, String, Path, List<Issue>)`（第 51-77 行）的值参数重载：
/// 一个只持有已校验的 root/internal name 的调用点可以用它
/// （Java 侧唯一的调用点是 `BuildPipeline.java:112`，那个类还没移植）。
///
/// `destination` 可以为 null：Java **没有**任何显式判空，第 53 行 `destination.resolve(internal)`
/// 直接抛 `NullPointerException`。Rust 的 `&Path` 表达不了 null，故那条崩溃路径由类型系统排除
/// （与 `mm-json` 里「Rust 的类型是非 optional 的，故 null 分支被排除」的既有处理一致）。
pub fn build_folder_root_internal(
    root: &Path,
    internal: &str,
    destination: &Path,
    issues: Option<&[Issue]>,
) -> io::Result<ExportReport> {
    let mod_dir = destination.join(internal);
    require_outside(root, &mod_dir)?;
    fs::create_dir_all(&mod_dir)?;

    for dir in mod_directories(root)? {
        fs::create_dir_all(mod_dir.join(relative(root, &dir)))?;
    }

    let mut written: Vec<String> = Vec::new();
    let mut bytes: i64 = 0;
    for file in mod_files(root)? {
        let relative = relative(root, &file);
        let target = mod_dir.join(&relative);
        // `toAbsolutePath()` 保证 parent 是 Some（`.expect` 恒不触发）。
        let parent = absolute_unnormalized(&target)
            .parent()
            .expect("toAbsolutePath 之后必有父目录，对应 Java Files.createDirectories 的 getParent()")
            .to_path_buf();
        fs::create_dir_all(&parent)?;
        // Java `Files.copy(file, target, REPLACE_EXISTING)`（第 68 行）在落进 provider 之前先做
        // **same-file 判定**（JDK 里是 `Util.checkSameFile` → `Files.isSameFile`）：源与目标是同一个
        // 文件时**不搬数据、直接返回 0**，既不打开源文件也不创建/删除目标。
        // `std::fs::copy` **没有**这一步：Windows 上它先试 `CopyFileExW`，失败再回退到「打开源 +
        // 以 `CREATE_ALWAYS` 建目标」，自拷贝会撞上自己刚持有的句柄，实测报
        // `Os { code: 32, kind: Uncategorized, message: "另一个程序正在使用此文件，进程无法访问。" }`
        // （Win32 `ERROR_SHARING_VIOLATION`）。
        // 触发场景是实打实的：`destination` 恰好就是项目根本身（Java
        // `ExporterPruneTest.pruningSkipsTheProjectFolderItself` 第 74-75 行的前提，
        // `mod_dir == root`）时，每个 `target` 都与 `file` 是同一个文件，Java 那边整条循环是绿的、
        // Rust 这边在第一个文件上就整次构建失败。故这里补上 JDK 那步判定，把自拷贝挡掉；
        // 判定为什么等价于 JDK 的 `Files.isSameFile`、在哪些边角上与 JDK 不同，见 [`same_file`]。
        //
        // 下面两行**不在** if 里：Java 第 69-70 行的 `written.add(relative)` 与
        // `bytes += Files.size(file)` 本就在 `Files.copy` **之外**、无条件执行，且 size 取的是
        // **源文件**，所以跳过拷贝时两者照记不误——报告的 `files`/`bytes` 与 Java 完全同值。
        if !same_file(&file, &target) {
            // `REPLACE_EXISTING` → `fs::copy` 同样覆盖已存在的目标。
            fs::copy(&file, &target)?;
        }
        written.push(relative);
        bytes += fs::metadata(&file)?.len() as i64;
    }

    let pruned = prune_stale(root, &mod_dir, &written)?;
    Ok(ExportReport::new(
        absolute_unnormalized(&mod_dir).to_string_lossy().to_string(),
        false,
        written,
        bytes,
        issues.map(<[Issue]>::to_vec).unwrap_or_default(),
        pruned,
    ))
}

/// JDK `Files.isSameFile` 的等价判定（`Files.copy` 经 `Util.checkSameFile` 调用的那一步）：
/// 比较的是**底层文件标识**，不是路径拼写。
///
/// 为什么 [`std::fs::canonicalize`] 能如实代表它——两边的判据都是「打开之后问内核这是哪个文件」：
/// - **Windows**：JDK 的 `WindowsFileSystemProvider.isSameFile` 打开两个文件后比
///   `GetFileInformationByHandle` 给出的 volume serial number + file index（同一卷上的同一个文件
///   才有同一对号码）。Rust 的 `canonicalize` 同样先 `CreateFileW` 打开、再
///   `GetFinalPathNameByHandleW` 取那个**句柄所在文件**的最终名；同一个文件经任何一条路径打开
///   （不同大小写、`.`/`..`、8.3 短名、目录 junction 或符号链接绕进去的别名），拿到的最终名都是
///   同一串，于是「两条 canonical 路径字符串相等」与「同一对 volume+file id」给出同样的答案。
///   返回串带 `\\?\` 前缀（网络路径是 `\\?\UNC\server\share\...`）：两边都带、形状相同，整串比较
///   即可，**不要**拿这条路径去做任何别的事（不拼相对路径、不进报告、不喂给上面那套词法 helper）。
/// - **Unix**：JDK 比 `stat` 的 `st_dev`/`st_ino`；Rust 的 `canonicalize` 走 `realpath`，同一个
///   inode 只有一个真实路径，两个判据同样一致。
///
/// 与 JDK 的两处边角差别，以及为什么到不了：
/// 1. **硬链接**：JDK 按文件标识判，会把同一文件的两个硬链接名认成同一个；
///    `GetFinalPathNameByHandleW` 只回该文件的「主别名」，两条只有硬链接之差的拼写在 JDK 里同判、
///    在这里可能各回各名而判为不同，于是走真正的 `fs::copy`（覆盖其中一条名、断开链接）。
///    要落进这条分支，得让 `mod_dir` 与项目根**不是**同一拼写、却逐文件互为硬链接——那要求目标目录
///    树整体等于项目树又与它不同名；`require_outside` 只允许目标等于项目根、或落在根之外（含根的
///    祖先），既「不等于根」又「逐文件同内容」的那棵树不是本模块任何调用点能构造出来的形态
///    （导出全程不创建目标侧的新文件，唯一写目标的地方就是这处拷贝），故到不了。
/// 2. **任一路径已不存在**：JDK 在这条路上要么按 `LinkFile`/realpath 继续判、要么抛
///    `NoSuchFileException`；这里直接把判定结果当作「不是同一个文件」，让紧随其后的 `fs::copy`
///    去产生与 Java 同源的错误上抛（源文件没了 → 两边都失败于拷贝，Java 亦然）。
///    目标侧不存在是**首次构建的常态**（Java 第 68 行的拷贝正是在创建目标目录之后凭空建文件），
///    那条路上必须放行到真正的拷贝，所以这里不吃 `canonicalize` 的错误、也不把它上抛。
///
/// 返回 `bool` 而非 `io::Result<bool>` 的理由就是第 2 点：判不出来不是失败，交回拷贝去失败。
/// 注意这与文件末尾那套「绝不访问文件系统」的词法 helper **刻意不同**：这一步在 JDK 里本来就要
/// 打开文件问内核，照抄语义就不能退回字符串比较。
///
/// 与本 crate 既有写法的对应：Java 的 `ModExporter` 里并没有显式的 same-file helper，这一步藏在
/// `Files.copy` 内部；仓库里已经把 JDK `Files.isSameFile` 承接成「`fs::canonicalize` 两边 + 出错即
/// 不算同一个文件」的是 `project_io.rs` 的 `same_file_on_disk`（对应 Java `ProjectIo.sameFileOnDisk`
/// 第 509-516 行：`Files.exists(a) && Files.exists(b) && Files.isSameFile(a, b)`，IOException 吞掉返回
/// false），本函数与它判据一致。不复用那个函数只因它是 `project_io` 模块的私有 `fn`，放宽它的可见性
/// 要动 `project_io.rs`，超出本次修复「只改 `mod_exporter.rs`」的范围。
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

/// Java `buildZip(ModProject, Path, List<Issue>)`（第 116-119 行）。
pub fn build_zip(
    project: &ModProject,
    zip_file: &Path,
    issues: Option<&[Issue]>,
) -> io::Result<ExportReport> {
    build_zip_root_internal(
        &require_root(project)?,
        &require_internal_name(project)?,
        zip_file,
        issues,
    )
}

/// Java `buildZip(Path, String, Path, List<Issue>)`（第 123-176 行）的值参数重载：
/// 用已经从模型上读好的值来打包，好让调用方把「读模型」放在拥有它的线程、把「打包」放在工作线程
/// （Java 侧的调用点是 `BuildPipeline.java:111`，那个类还没移植）。
pub fn build_zip_root_internal(
    root: &Path,
    internal: &str,
    zip_file: &Path,
    issues: Option<&[Issue]>,
) -> io::Result<ExportReport> {
    require_outside(root, zip_file)?;
    let absolute_zip = absolute_unnormalized(zip_file);
    // 同样是 `toAbsolutePath()` 之后 parent 必为 Some。
    let parent = absolute_zip
        .parent()
        .expect("toAbsolutePath 之后必有父目录，对应 Java Files.createDirectories(zipFile.toAbsolutePath().getParent())")
        .to_path_buf();
    fs::create_dir_all(&parent)?;

    let files = mod_files(root)?;
    let dirs_with_files = directories_with_files(root, &files);

    // Java 第 133-173 行：临时文件 + `moveIntoPlace`，失败则删临时文件并把原异常**原样**抛出。
    let temp = atomic_files::temp_for(&absolute_zip);
    match (|| -> io::Result<(Vec<String>, i64)> {
        let packed = write_zip(&temp, root, internal, &files, &dirs_with_files)?;
        atomic_files::move_into_place(&temp, zip_file)?;
        Ok(packed)
    })() {
        Ok((written, bytes)) => Ok(ExportReport::new(
            absolute_zip.to_string_lossy().to_string(),
            true,
            written,
            bytes,
            issues.map(<[Issue]>::to_vec).unwrap_or_default(),
            // Java 第 175 行：zip 不 prune，列表为空。
            Vec::new(),
        )),
        Err(failure) => {
            atomic_files::delete_quietly(&temp);
            Err(failure)
        }
    }
}

/// Java `buildZip` 里那个 try-with-resources 块（第 138-168 行）：打开临时文件、写条目、关掉流，
/// 返回 Java 用方法体内局部变量带回来的 `written` 与 `bytes`。
///
/// 把它拆成独立函数只是**表达**上的差异：Rust 需要让 `moveIntoPlace` 与它同处一个 `?` 作用域，
/// 才能复刻第 170-173 行那个 catch。条目顺序与 Java 严格相同：先按 `files`（已排序）逐个写文件
/// 条目，再遍历 `modDirectories(root)`——Java 第 159 行是**第二次**遍历，不是复用第一次的结果——
/// 写空目录条目，命中 `dirsWithFiles` 的目录被跳过：那个集合是**跳过集**，不是产出集。
fn write_zip(
    temp: &Path,
    root: &Path,
    internal: &str,
    files: &[PathBuf],
    dirs_with_files: &[PathBuf],
) -> io::Result<(Vec<String>, i64)> {
    // Java `Files.newOutputStream(temp)`：CREATE + TRUNCATE_EXISTING + WRITE → `File::create`。
    let file = fs::File::create(temp)?;
    let mut zip = zip::ZipWriter::new(file);
    // Java `out.setLevel(Deflater.BEST_COMPRESSION)`（第 140 行）是**流级**设置，zip crate 里是
    // **条目级**设置，所以把它编进所有条目共用的 options；下面的 `add_directory` 会在内部把目录
    // 条目强制改成 Stored（Java 那边空目录条目同样没有内容可压），故 level 只作用于文件条目。
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(BEST_COMPRESSION))
        .last_modified_time(fixed_dos_time());

    let mut written: Vec<String> = Vec::new();
    let mut bytes: i64 = 0;
    for file in files {
        let relative = relative(root, file);
        // Java 第 143-144 行：条目名 `internal + "/" + relative`，时间 FIXED_TIME。
        zip.start_file(format!("{internal}/{relative}"), options)?;
        // Java 第 151-153 行 `Files.newInputStream(file)` + `in.transferTo(out)`：整份读入再写出，
        // 与流式转发在字节上一致。Java 累加的是 `transferTo` 的返回值，即**实际写出的字节数**，
        // 故这里用 `data.len()`，不去查 `Files.size`（两者对 regular file 同值，但取处要一样）。
        let data = fs::read(file)?;
        zip.write_all(&data)?;
        written.push(relative);
        bytes += data.len() as i64;
    }

    // 空目录条目：以 `/` 结尾、没有内容的条目，否则解开 zip 就丢了这些文件夹。
    // `dirsWithFiles` 在这里是**跳过集**：含文件的目录已经由上面的文件条目隐式带出，
    // 只有真正空的目录才需要一个显式条目（Java 第 158-167 行）。
    for dir in mod_directories(root)? {
        if dirs_with_files.contains(&dir) {
            continue;
        }
        let relative = relative(root, &dir);
        zip.add_directory(format!("{internal}/{relative}/"), options)?;
    }

    // Java 的 try-with-resources 在 `moveIntoPlace` **之前**关闭流（写出中央目录）。
    // `finish()` 归还内部的 `File` 并立即 drop，Windows 上这保证重命名时文件已不被占用。
    zip.finish()?;
    Ok((written, bytes))
}

/// `FIXED_TIME` 的**UTC 日历分量**，用作 zip 条目时间。
///
/// Java 把同一个 epoch 毫秒交给 `ZipEntry.setTime(long)`，JDK 再按 **JVM 默认时区**拆成
/// DOS 日期/时间字段（`Calendar.getInstance()`），并在中央目录额外补一个 NTFS 扩展字段存
/// UTC 的 FILETIME。zip crate 的 `DateTime` 就是一个不带时区的日历时间、也不写 NTFS 字段，
/// 所以这里取「按 UTC 解释 FIXED_TIME」：跨机器、跨时区都能得到**恒定**的时间戳，
/// 正好是 Java 那个常量存在的理由（「Fixed timestamp so repeated builds of the same project
/// produce identical archives」）。差异只在字节层面，条目名/顺序/内容不受影响。
fn fixed_dos_time() -> zip::DateTime {
    let (year, month, day) = civil_from_days(FIXED_TIME.div_euclid(86_400_000));
    let second_of_day = FIXED_TIME.rem_euclid(86_400_000) / 1000;
    zip::DateTime::from_date_and_time(
        year as u16,
        month,
        day,
        (second_of_day / 3_600) as u8,
        ((second_of_day % 3_600) / 60) as u8,
        (second_of_day % 60) as u8,
    )
    .expect("FIXED_TIME = 2020-01-01T00:00:00Z，恒在 zip DOS 时间可表示的 1980-2107 区间内")
}

/// Java `modFiles(Path)`（第 189-199 行）：会进包的文件，路径以 `project_root` 为前缀、已排序。
///
/// 过滤链的四步顺序照 Java 原样：`isRegularFile`（由 [`walk_tree`] 承担）→ `isModPath` →
/// 排除 `.mm-tmp` 后缀 → 排序。
///
/// 返回 `io::Result` 只为对上 Java 的 `throws IOException`：遍历失败的处理见 [`walk_tree`]，
/// 那条路上不产生错误。
pub fn mod_files(project_root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    walk_tree(project_root, &mut dirs, &mut files);
    files.retain(|file| is_mod_path(project_root, file));
    // Java 第 193-195 行的注释：保存中途崩溃会留下 `<target>.mm-tmp`，它不属于 mod，绝不能进包。
    // `Files.walk` 产出的 regular file 必带文件名，故取不到文件名时按「不以后缀结尾」放行。
    files.retain(|file| {
        !file
            .file_name()
            .map(|name| name.to_string_lossy().ends_with(TEMP_SUFFIX))
            .unwrap_or(false)
    });
    files.sort_by(|a, b| java_path_cmp(a, b));
    Ok(files)
}

/// Java `modDirectories(Path)`（第 202-210 行）：会进包的目录，路径以 `project_root` 为前缀、已排序。
pub fn mod_directories(project_root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    walk_tree(project_root, &mut dirs, &mut files);
    dirs.retain(|dir| {
        dir.is_dir() && !path_eq(dir, project_root) && is_mod_path(project_root, dir)
    });
    dirs.sort_by(|a, b| java_path_cmp(a, b));
    Ok(dirs)
}

/// Java `isModPath(Path, Path)`（第 234-265 行）。
///
/// Java 的 Javadoc（第 224-233 行）说清了三条排除的顺序与「根层文件是例外」的理由，
/// 下面的分支严格照那个顺序写：dot 段 → 作者层 → 单段特例 → 多段只看首段是否在清单里。
fn is_mod_path(project_root: &Path, path: &Path) -> bool {
    let relative = java_relativize(project_root, path);
    let segments = name_segments(&relative);
    if segments.is_empty() {
        return false;
    }
    // Java 第 240-245 行：任何一段为空或以 `.` 开头就整条排除。这是三条排除里的**第一条**，
    // 大小写**敏感**（Java 只有 `segment.charAt(0) == '.'`）。Javadoc 第 224-233 行给了两个理由：
    // VCS/git 簿记（`.git/`、`.gradle/`、`.gitignore`）与 OS/IDE 垃圾（`.DS_Store`、编辑器备份文件）；
    // 曾经因此把 `.git/objects` 打进 zip、把每个 blob 注册成游戏资源。
    // `bundles/` 里真正的 bundle 用 `*.bundle` 这种写法、不靠前导点，所以这条规则不会误伤。
    if segments
        .iter()
        .any(|segment| segment.is_empty() || segment.starts_with('.'))
    {
        return false;
    }
    let first = &segments[0];
    // Java 第 246-249 行：第二条排除——作者层，判定**大小写不敏感**，于是 Windows 上 `BuildManifest/`、
    // `BUILDMANIFEST/` 也一样挡掉：绝不进包，也绝不参与哈希。
    if equals_ignore_case(first, AUTHORING_DIR) {
        return false;
    }
    if segments.len() == 1 {
        // Java 第 250-263 行：根层这一层是例外（mod 合法地在根下带任意文件——banner、readme、licence，
        // 导入器会把它们每一个登记为 extra 资源，所以固定白名单会丢真 payload、破字节级往返）。
        // `String name = relative.toString();`——段数为 1 时它就等于首段。
        let name = relative.to_string_lossy().to_string();
        // 第 252-256 行：根层的 mod.hjson / text.hjson / *.bundle 之类的元数据文件。
        for meta in ModMeta::META_FILES {
            if *meta == name {
                return true;
            }
        }
        // 第 257-259 行：根层目录**只**按 MOD_DIRECTORIES 判，绝不落到 `!isNoise`。
        if path.is_dir() {
            return MOD_DIRECTORIES.contains(&name.as_str());
        }
        // 第 260-262 行：其余根层条目沿用读包器那份保守的噪声清单——`.gitignore` 排除掉的东西不该进包，
        // 这正是「导入与导出对 mod 含什么的看法一致」那条不变式。
        // 三段必须是**顺序分支**，不能写成三个条件的 `||` 串联：一个不在 MOD_DIRECTORIES 里的根层
        // 目录（`build/`、`out/`、`gradle/`）在 Java 里返回 false，而 `|| !is_noise(name)` 会因为
        // 「它不在已知噪声文件名的清单里」把它当成合法条目放行。
        return !is_noise(&name);
    }
    // Java 第 264 行：多段路径由首段定性，其余段不再检查——只有 `MOD_DIRECTORIES` 里那些目录的子树
    // 才是 mod 的内容。这就是「开发垃圾不打包」那条规则真正被执行的地方。
    MOD_DIRECTORIES.contains(&first.as_str())
}

/// Java `requireInternalName(ModProject)`（第 274-286 行）。三条错误消息逐字照抄。
pub fn require_internal_name(project: &ModProject) -> io::Result<String> {
    let internal = project.internal_name();
    if internal.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mod name is empty; the built folder needs a name",
        ));
    }
    if !is_folder_safe(&internal) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("mod name is not a safe folder name: {internal}"),
        ));
    }
    // Java 第 282-284 行：`internal.contains(" ") || !internal.equals(internal.toLowerCase(Locale.ROOT))`。
    // 这一条**不是**冗余：上面那个 `is_folder_safe`（`name_rules.rs` 第 128-136 行）只管 `/`、`\`、
    // `..`、`:`、保留名、长度，**空格是允许的**，所以带空格或带大写的内部名一路走到这里才被拦下——
    // 大小写不敏感的文件系统上 `My Mod` 会跟目录名对不上，包整个塌掉。
    // `toLowerCase(Locale.ROOT)` 对应 Rust 的 `to_lowercase()`：两者都是 Unicode 小写映射，
    // 差异只在 Java 按 UTF-16 码元逐字符映射、Rust 允许一对一展开（如 `İ`）；内部名实际是 ASCII。
    if internal.contains(' ') || internal.to_lowercase() != internal {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("mod name does not clean up to a folder-safe name: {internal}"),
        ));
    }
    Ok(internal)
}

/// Java `requireRoot(ModProject)`（第 267-272 行）。
pub fn require_root(project: &ModProject) -> io::Result<PathBuf> {
    match project.root.as_deref() {
        Some(root) if root.is_dir() => Ok(root.to_path_buf()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project has no folder on disk; save it before building",
        )),
    }
}

/// Java `requireOutside(Path, Path)`（第 293-304 行）。
///
/// Javadoc 第 288-292 行的措辞要照搬过来理解：严格落在项目文件夹**之内**的目标会把 mod 的副本
/// 嵌进它自己、每次构建都再长一层，因此直接拒绝；而构建到项目文件夹本身**之上**（同一个目录或
/// 它的祖先）是被容忍的——那种情况下 `pruneStale` 拒绝删任何东西。
fn require_outside(project_root: &Path, target: &Path) -> io::Result<()> {
    // Java `root.toAbsolutePath().normalize()`：`Path::normalize` 在本 rustc 未稳定，故沿用
    // project_io.rs 的那套词法处理——先 `std::path::absolute`（只拼 cwd，等价 toAbsolutePath），
    // 再 `lexical_normalize` 纯词法消掉 `.` 与 `x/..`，全程不碰文件系统。
    let root = absolute_normalized(project_root);
    let resolved = absolute_normalized(target);
    // Java 第 296-297 行的行内注释：这里**故意**两处都大小写不敏感——换一种拼写在 Windows/macOS 上
    // 仍然指向项目文件夹内部的路径，构建到那儿照样每次多嵌一层。
    let same = equals_ignore_case(&resolved.to_string_lossy(), &root.to_string_lossy());
    let inside = starts_with_ignore_case(
        &resolved.to_string_lossy(),
        &format!("{}{FILE_SEPARATOR}", root.to_string_lossy()),
    );
    if !same && inside {
        // 消息里的 `resolved` 用 Java 的 `Path.toString()` 拼进字符串，
        // 不是 `{:?}`——后者会加引号并把反斜杠再转义一遍。
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the build target is inside the project folder: {} - \
                 the mod would nest a copy of itself; pick a folder outside",
                resolved.to_string_lossy()
            ),
        ));
    }
    Ok(())
}

/// Java `pruneStale(Path, Path, List<String>)`（第 84-113 行）。
///
/// Javadoc 第 80-83 行说得很直接：删掉上一次构建留在目标里、而本次构建没有写的那些普通文件；
/// 当目标就是项目本身、或项目位于目标之内时**绝不**运行——在那儿 prune 会吃掉项目自己的作者文件。
///
/// **不要**把它读成 `mod_files` 的反向版本：Java 在这里**不**调用 `isModPath`，判据只有两条——
/// `keep` 集合与作者层。也就是说产物目录里冒出来的 `.git/` 之类，Java 是会删的。
/// 删掉的东西按相对路径返回，供报告使用。
fn prune_stale(project_root: &Path, mod_dir: &Path, written: &[String]) -> io::Result<Vec<String>> {
    let root_path = absolute_normalized(project_root).to_string_lossy().to_string();
    let target_path = absolute_normalized(mod_dir).to_string_lossy().to_string();
    // 构建目标**包含**了项目时，prune 会删掉正在读的那些文件。那种目标早已被
    // `requireOutside` 挡在门外，但顺序随时可能反过来，故这里仍做检查。
    // Java 第 88-89 行：祖先这一侧的检查**必须**大小写不敏感——Windows/macOS 上换一种拼写的目标
    // 仍然指着这个项目文件夹，在那儿 prune 会吃掉项目自己的作者文件。
    // Java：`startsWithIgnoreCase(rootPath, targetPath + File.separatorChar)`——注意方向。
    if equals_ignore_case(&root_path, &target_path)
        || starts_with_ignore_case(&root_path, &format!("{target_path}{FILE_SEPARATOR_CHAR}"))
    {
        return Ok(Vec::new());
    }

    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    walk_tree(mod_dir, &mut dirs, &mut candidates);
    // Java 用 `new HashSet<>(written)` 做 contains；这里用 `Vec::contains`，成员判定
    // （区分大小写的整串相等）完全一致，只是复杂度不同。
    candidates.sort_by(|a, b| java_path_cmp(a, b));

    let mut pruned: Vec<String> = Vec::new();
    for file in candidates {
        let relative = relative(mod_dir, &file);
        if written.contains(&relative) {
            continue;
        }
        // 作者状态与源项目里一样，绝不进包，也绝不删。
        if relative == AUTHORING_DIR || relative.starts_with(&format!("{AUTHORING_DIR}/")) {
            continue;
        }
        // 这里**没有** `modFiles` 第 195 行那条 `TEMP_SUFFIX` 过滤：Java 的 pruneStale
        // （第 98-110 行）判据只有 `keep` 与作者层两条，留在构建目录里的 `*.mm-tmp` 半成品
        // 会被一并删掉。多加一层过滤就是加 Java 没有的容错。
        //
        // Java `Files.deleteIfExists`（第 108 行）：文件在这一步之前已经消失则返回 false、
        // **不**抛异常，其余 I/O 错误照常上抛；而第 109 行无条件把相对路径记进 pruned。
        if let Err(failure) = fs::remove_file(&file)
            && failure.kind() != io::ErrorKind::NotFound
        {
            return Err(failure);
        }
        pruned.push(relative);
    }
    Ok(pruned)
}

/// Java `startsWithIgnoreCase(String, String)`（第 306-308 行）：
/// `text.regionMatches(true, 0, prefix, 0, prefix.length())`。
/// `regionMatches` 允许 `text` 的码元数超过 `prefix`，`prefix` 更长则直接 false。
fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    let mut left = text.chars();
    for expected in prefix.chars() {
        match left.next() {
            Some(actual) if chars_equal_ignore_case(actual, expected) => {}
            _ => return false,
        }
    }
    true
}

/// JDK `String.equalsIgnoreCase(String)`：`length()` 相同 + `regionMatches(true, ...)`。
/// Java 侧的两个落点是 `ModExporter` 第 90 行（`pruneStale`）与第 298 行（`requireOutside`）。
pub(super) fn equals_ignore_case(a: &str, b: &str) -> bool {
    a.chars().count() == b.chars().count() && starts_with_ignore_case(a, b)
}

/// JDK `String.regionMatches(ignoreCase = true)` 的逐字符判定，等价于
/// [`cmp_char_ignore_case`] 的「是否相等」。三段顺序照原样保留，不简化成
/// `eq_ignore_ascii_case`——后者会漏掉 Java 对非 ASCII 字符的那两条回退。
fn chars_equal_ignore_case(a: char, b: char) -> bool {
    cmp_char_ignore_case(a, b) == std::cmp::Ordering::Equal
}

/// JDK 大小写不敏感逐字符比较的三段回退（`String.regionMatches(ignoreCase = true)` 与
/// `String.CASE_INSENSITIVE_ORDER.compare` 共用同一套）：先原样比；不等则各自
/// `toUpperCase()` 再比；仍不等则各自对**上一步的大写形式**取 `toLowerCase()` 再比，
/// 此时若还不等就返回这两个字符之差（对应 JDK 的 `return c1 - c2`）。
///
/// 近似两处，与本文件其余 `char` 口径同源：JDK 在第二段相等时会记下 `collationIndex`、
/// 循环结束后再对剩余字符做一遍 `toLowerCase()` 复查（只影响少数 Unicode 字符），这里不复刻；
/// `to_uppercase()` / `to_lowercase()` 都只取首个码点。ASCII 与 BMP 常用路径结果与 Java 一致。
fn cmp_char_ignore_case(a: char, b: char) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }
    let (upper_a, upper_b) = (upper_char(a), upper_char(b));
    if upper_a == upper_b {
        return std::cmp::Ordering::Equal;
    }
    lower_char(upper_a).cmp(&lower_char(upper_b))
}

/// `Character.toUpperCase(char)`：Java 在 UTF-16 码元上做单字符映射，Rust 的
/// `char::to_uppercase` 可能返回多个码点，这里取第一个（ASCII/BMP 路径与 Java 一致）。
fn upper_char(value: char) -> char {
    value.to_uppercase().next().unwrap_or(value)
}

/// `Character.toLowerCase(char)`：同上，取 `to_lowercase()` 的首个码点。
fn lower_char(value: char) -> char {
    value.to_lowercase().next().unwrap_or(value)
}

/// Java `directoriesWithFiles(Path, List<Path>)`（第 212-222 行）：`files` 里每个文件在
/// `project_root` 之下的**整条祖先目录链**，用 `HashSet` 去重。
///
/// 它在 Java 里只被当作**跳过集**用：`buildZip` 第 129 行取到它，之后唯一的调用是第 160 行的
/// `dirsWithFiles.contains(dir)`，从头到尾没有迭代过这个集合，所以它的次序在 Java 那边本来就
/// 不可观察。这里用 `Vec` + `contains` 表达同一个集合，次序稳定但没有任何行为依赖它。
/// 唯一的口径差别：Java 的 `HashSet<Path>` 用 `WindowsPath` 的 `hashCode`/`equals`，在 Windows 上
/// **大小写不敏感**，而 Rust 的 `Path: PartialEq` 敏感。两边在这里必然同判：`dir` 来自
/// `modDirectories(root)`，集合元素来自同一趟 walk 的 `file.getParent()`，同一次遍历给出的拼写
/// 完全一致，不存在「同一目录的两种大小写」同时出现的可能。
fn directories_with_files(project_root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for file in files {
        let mut current = file.parent().map(Path::to_path_buf);
        while let Some(node) = current {
            if !path_starts_with(&node, project_root) {
                break;
            }
            if !dirs.contains(&node) {
                dirs.push(node.clone());
            }
            current = node.parent().map(Path::to_path_buf);
        }
    }
    dirs
}

/// Java `relative(Path, Path)`（第 310-312 行，Java 那边是 `private static`）：
/// `root.relativize(file).toString().replace('\\', '/')`。
fn relative(root: &Path, file: &Path) -> String {
    to_slash(&java_relativize(root, file))
}

/// `Files.walk(root)`（默认 `FileVisitOption` 为空、maxDepth 无限）的等价遍历。
///
/// 三点忠实性说明：
/// 1. 「是不是目录」用 `Path::is_dir()` 判，因为调用方的 `Files::isDirectory` 与
///    `Files::isRegularFile` 一样**跟随**链接：指向目录的符号链接在 Java 里会进 `modDirectories`。
///    但**下潜与否**跟着 Java 的 walker 走——它没传 `FOLLOW_LINKS`，所以那样的链接只被访问一次、
///    绝不进入；这里用 `DirEntry::file_type()`（不跟随链接）识别符号链接 / junction 并挡掉下潜，
///    否则 Unix 上会少收一个目录条目、Windows 上会多潜一层（甚至成环）；
/// 2. 入选文件用 `Path::is_file()` 判定，同样**跟随**链接，与 `Files::isRegularFile` 一致：
///    指向文件的符号链接在两边都算普通文件；
/// 3. Java 里 `Files.walk` 的 I/O 错误会以异常抛出；本仓库对 walk 已有先例
///    （`project_io.rs` 第 710-724 行的 `collect_regular_files`，同一取舍另见第 660-661 行）
///    是「目录读取失败只表现为看不到文件」，这里照同一口径办——调用方在遍历前都已用
///    `is_dir` 确认过根目录存在，唯一区别是「中途某个子目录读不动」，Java 会让整次导出失败、
///    Rust 视之为空目录。
///
/// `build_manifest.rs` 的 `hashTree` 共用这一份遍历，故可见性放到 `pub(super)`。
pub(super) fn walk_tree(dir: &Path, dirs: &mut Vec<PathBuf>, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path.clone());
            let link = matches!(entry.file_type(), Ok(file_type) if file_type.is_symlink());
            if !link {
                walk_tree(&path, dirs, files);
            }
        } else if path.is_file() {
            files.push(path);
        }
    }
}

/// Java `Path.getNameCount()` + `getName(i).toString()` 的等价物：路径的名字元素列表。
/// 相对化之后的路径只含 `Normal` 元素；`to_string_lossy` 的取舍与 `to_slash` 同源。
fn name_segments(path: &Path) -> Vec<String> {
    path.components().map(|component| component.as_os_str().to_string_lossy().to_string()).collect()
}

// ---------------------------------------------------------------------------
// 以下 helper 与 project_io.rs 第 1318-1430 行的同名私有函数逐字符同构
// （`lexical_normalize` / `absolute_normalized` / `path_starts_with` / `component_eq` /
// `path_eq` / `java_relativize` / `to_slash`；那里是冻结文件，本步不许改，故在此复制一份
// 给 export 子模块共用）。`absolute_unnormalized` 是这边多出的一份，对应 Java 只
// `toAbsolutePath()`、不 normalize 的那些落点。口径以 project_io.rs 那一份为准：
// Java 是纯词法操作，Rust 也绝不做文件系统调用。
// ---------------------------------------------------------------------------

/// Java `Path.toAbsolutePath()`：绝对化，**不** normalize。
/// `std::path::Path::normalize` 在本 rustc 未稳定，故用 std 的 `absolute`。
pub fn absolute_unnormalized(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Java `path.toAbsolutePath().normalize()`：`normalize()` 只做**词法**处理——解析 `.` 与 `..`，
/// 既不解链也不访问文件系统；绝对化同样不访问文件系统。
pub fn absolute_normalized(path: &Path) -> PathBuf {
    match std::path::absolute(path) {
        Ok(absolute) => lexical_normalize(&absolute),
        // 空路径之类展开不了的输入，Java 也是直接抛，这里保持词法形式即可。
        Err(_) => lexical_normalize(path),
    }
}

/// 纯词法的 `.` / `..` 解析，复刻 Java `Path.normalize()`：`..` 只在**没有**字面父目录时才保留，
/// 故 `a/../..` 会塌成 `..`；根元素与其后的分隔符原样保留。
///
/// Java 的 `..` 折叠比「弹出一个名字元素」更微妙：当 `a` 是一个**已存在的符号链接**时，
/// `a/..` 归一化到的并不是 `a` 的父目录。Java 的 `normalize()` 是纯词法的（它从不调
/// `readLink`），故这里照抄那份词法实现；在 `a` 实为符号链接时保留 `..` 会偏离 Java，
/// 而那条路径在构建流程里不可达（mod 根由用户选定、构建目标又由 `requireOutside` 校验）。
/// 这一点连同理由都在 `project_io.rs` 的 `lexical_normalize` 注释里，本处只是复用同一算法。
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

/// Java `Path.startsWith`：Windows 上按不敏感方式逐组件比较（Java 的 `WindowsPath` 正是如此），
/// 其余平台逐字节比较。这里的不敏感只用 `to_lowercase()`，对模组里的 ASCII 名字与 Java 等价。
pub fn path_starts_with(path: &Path, base: &Path) -> bool {
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

/// Java `Path.equals` 的语义（同样带 Windows 的不敏感比较），用于已归一化的路径。
pub fn path_eq(left: &Path, right: &Path) -> bool {
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

/// `Files.walk(...).sorted()` 用的 `Path` 自然序：Java 在 Windows 上是
/// `WindowsPath.compareTo` = `String.CASE_INSENSITIVE_ORDER` 比较**归一化后的整条路径字符串**，
/// 在 Unix 上则是 `UnixPath.compareTo` 的字节序比较。同一个 root 之下的条目共享前缀，故等价于
/// 比较各自去掉前缀后的那一段，这里直接比较完整字符串。
///
/// `CASE_INSENSITIVE_ORDER` 的算法见 [`cmp_char_ignore_case`]：三段回退都不等才算不等，
/// 逐字符都判平之后返回长度差（JDK 的 `return s1.length() - s2.length()`）。Rust 侧以
/// `char` 为单位、取 `to_uppercase()`/`to_lowercase()` 的首个码点近似：ASCII 与 BMP 常用范围
/// 结果与 Java 一致，非 BMP 码点（Java 走代理对）与 Java 可能不同——mod 目录名不会落在后者上，
/// 故保留这个近似而不引入 UTF-16 编码往返。
fn java_path_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    if !cfg!(windows) {
        return a.as_os_str().cmp(b.as_os_str());
    }
    let (left, right) = (a.to_string_lossy(), b.to_string_lossy());
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let shared = left_chars.len().min(right_chars.len());
    for index in 0..shared {
        match cmp_char_ignore_case(left_chars[index], right_chars[index]) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    left_chars.len().cmp(&right_chars.len())
}

/// Java `Path.relativize`：公共前缀之后的每一层补一个 `..`，再接上目标剩下的部分。
pub(super) fn java_relativize(base: &Path, target: &Path) -> PathBuf {
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

/// Java `path.toString().replace('\\', '/')`。
pub(super) fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
