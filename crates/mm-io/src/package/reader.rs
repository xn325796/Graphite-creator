//! `ModPackageReader.java` 的等价移植：像游戏那样把一个模组读成 [`ModPackage`]。
//!
//! 对应 Java `dev.modmaker.core.io.ModPackageReader`。

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use mm_fmt::codec::{is_bundle_file, mod_meta_from_json};
use mm_fmt::ContentType;
use mm_model::ModMeta;
use mm_schema::KeyedMap;
use zip::ZipArchive;

use super::mod_package::{ModPackage, SourceContent};
use crate::import::cancelled::ImportError;
use crate::import::progress::{worth_reporting, ImportProgress, NoProgress, Stage};
use crate::project::TEMP_SUFFIX;

/// Java `NOISE_FILES`：打包/下载时顺手混进来的操作系统与版本控制垃圾，按**文件名**整名比对。
/// 它们不是模组内容，带进项目只会在下次构建时再被打回包里，所以一律丢弃（并记入 `skipped`）。
const NOISE_FILES: [&str; 7] = [
    ".DS_Store",
    "desktop.ini",
    "Thumbs.db",
    ".git",
    ".gitignore",
    ".gitattributes",
    ".gitmodules",
];

/// Java `NOISE_DIRS`：整目录式的垃圾——macOS 压缩时的 `__MACOSX`、各家 IDE / VCS 的元数据目录，
/// 以及本编辑器自己写进模组目录的工作区 `modmaker/`。按路径前缀判定。
const NOISE_DIRS: [&str; 6] = [
    "__MACOSX/",
    ".git/",
    ".idea/",
    ".vscode/",
    ".svn/",
    "modmaker/",
];

const CONTENT: &str = "content/";
const BUNDLES: &str = "bundles/";

/// 导入预算：构造出来的 zip 不能把内存耗尽或让导入挂住（zip 炸弹防护）。
const MAX_ENTRIES: usize = 20_000;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
/// 记住多少个被拒绝的条目名就够了；带敌意的归档会有几百万个。
const MAX_SKIPPED: usize = 200;

/// Java `read(Path)`：即 `read(source, ImportProgress.NONE)`——目录走目录通道，`.zip`/`.jar`
/// 走归档通道，其余报错。
///
/// [`NoProgress`]（Java 的 `NONE`）的每个回调都是空操作且永不取消，所以这条路径实际不可能返回
/// [`ImportError::Cancelled`]；与带进度入口共用结果类型（Rust 类型系统的代价——Java 侧同样共用
/// 代码路径，靠不受检的 `ImportCancelledException` 穿过 `throws IOException` 声明）。
pub fn read(source: &Path) -> Result<ModPackage, ImportError> {
    read_with(source, &mut NoProgress)
}

/// Java `read(Path, ImportProgress)`：目录走目录通道，`.zip`/`.jar` 走归档通道，其余报错。
/// 取消检查与 READING 阶段的进度上报发生在两个读取循环里。
pub fn read_with(source: &Path, progress: &mut dyn ImportProgress) -> Result<ModPackage, ImportError> {
    if source.is_dir() {
        return read_folder(source, progress);
    }
    // Java: source.getFileName().toString().toLowerCase(Locale.ROOT)
    let name = source
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if name.ends_with(".zip") || name.ends_with(".jar") {
        return read_zip(source, progress);
    }
    Err(ImportError::Io(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Not a mod folder or zip: {}", source.display()),
    )))
}

fn read_folder(root: &Path, progress: &mut dyn ImportProgress) -> Result<ModPackage, ImportError> {
    let mut module = ModPackage { source_label: absolute_label(root), ..Default::default() };
    let mut files: KeyedMap<Vec<u8>> = KeyedMap::new();
    let mut scan = FolderScan::default();
    scan.walk(root, root)?;

    // Java L123-132：READING 阶段先报总数，循环是「查取消 → 读字节 → 节流 tick」。
    let total = scan.file_names.len() as i32;
    progress.stage_started(Stage::Reading, total);
    for i in 0..total {
        progress.check_cancelled()?;
        files.put(scan.file_names[i as usize].clone(), fs::read(&scan.file_paths[i as usize])?);
        let done = i + 1;
        if worth_reporting(done, total) {
            progress.progressed(Stage::Reading, done, total);
        }
    }
    module.empty_dirs.extend(empty_dirs(&scan.dirs, files.keys()));
    classify(&mut module, &files)?;
    // Java: 普通目录导入的根就是这个目录本身，所以什么都不剥。
    Ok(module)
}

fn read_zip(zip_path: &Path, progress: &mut dyn ImportProgress) -> Result<ModPackage, ImportError> {
    let mut module =
        ModPackage { source_label: absolute_label(zip_path), from_zip: true, ..Default::default() };

    let mut files: KeyedMap<Vec<u8>> = KeyedMap::new();
    let mut entries_by_name: KeyedMap<usize> = KeyedMap::new();
    let mut dirs: Vec<String> = Vec::new();

    // ZipError 折进 io::Error 对应 Java 的 ZipException extends IOException。
    let mut archive = ZipArchive::new(fs::File::open(zip_path)?)
        .map_err(io::Error::from)?;
    let mut names: Vec<String> = Vec::new();
    let mut seen: usize = 0;
    for index in 0..archive.len() {
        let entry = archive.by_index_data(index).map_err(io::Error::from)?;
        // Java 的 `entry.getName()`：zip crate 尊重 UTF-8 标志位、未置位时按 CP437 解，与 JDK 一致；
        // 真解不出来时退回对原始字节做 UTF-8 有损解码（坏字节替换成 U+FFFD），而不是整包失败。
        let name = match entry.name() {
            Ok(name) => name.into_owned(),
            Err(_) => String::from_utf8_lossy(entry.name_raw()).into_owned(),
        }
        .replace('\\', "/");
        // 每个条目都计入预算，不管它最后变成什么：目录条目和被拒绝的穿越名以前会整个跳过检查，
        // 于是「一大堆空目录」这种包会被接受并全部留在内存里。
        seen += 1;
        if seen > MAX_ENTRIES {
            return Err(budget_error(format_args!(
                "the archive holds more than {MAX_ENTRIES} entries and is refused as a possible zip bomb"
            ))
            .into());
        }
        // 在源头挡住 zip-slip：路径逃出货架根的条目（`../`、绝对路径或带根的路径、盘符开头）
        // 在被使用之前就跳过。
        if is_unsafe_entry_name(&name) {
            if module.skipped.len() < MAX_SKIPPED {
                module.skipped.push(format!("{name} (path escapes the package)"));
            }
            continue;
        }
        // Java: `entry.isDirectory()`，即 JDK 的「名字以 '/' 结尾」。这里直接按名字判断，
        // 与 JDK 语义一致（zip crate 的 `is_dir()` 还掺了外部属性，可能多判出目录来）。
        if name.ends_with('/') {
            dirs.push(strip_trailing_slash(&name));
            continue;
        }
        names.push(name.clone());
        // 保留**枚举到的那一条**，而不是回头按名字再查：Windows 工具写的归档把名字存成反斜杠，
        // 这时 `archive.getEntry(归一化名)` 查不到，文件就被无声丢掉了——而它是真内容。
        entries_by_name.put(name, index);
    }

    let prefix = single_root_prefix(&names);
    module.root_prefix = prefix.clone();
    let mut total_bytes: u64 = 0;
    // Java L183-190：READING 先报总数；tick 在取名字与干活**之前**（计的是枚举到的条目数，
    // 与目录通道「读完再 tick」的顺序不同，照搬不改）。
    let total = names.len() as i32;
    progress.stage_started(Stage::Reading, total);
    for i in 0..total {
        progress.check_cancelled()?;
        if worth_reporting(i + 1, total) {
            progress.progressed(Stage::Reading, i + 1, total);
        }
        let name = &names[i as usize];
        if !prefix.is_empty() && !name.starts_with(&prefix) {
            continue;
        }
        let relative = strip_prefix(name, &prefix);
        if relative.is_empty() || relative.ends_with('/') {
            continue;
        }
        let Some(index) = entries_by_name.get(name) else {
            continue;
        };
        let entry = archive.by_index(*index).map_err(io::Error::from)?;
        let mut data: Vec<u8> = Vec::new();
        // Java 的 `readNBytes(MAX_ENTRY_BYTES + 1)`：多读一字节用来判断「是否超限」。
        entry.take(MAX_ENTRY_BYTES + 1).read_to_end(&mut data)?;
        if (data.len() as u64) > MAX_ENTRY_BYTES {
            return Err(budget_error(format_args!(
                "zip entry '{name}' exceeds {} MiB and is refused",
                MAX_ENTRY_BYTES / (1024 * 1024)
            ))
            .into());
        }
        total_bytes += data.len() as u64;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(budget_error(format_args!(
                "the archive unpacks to more than {} GiB and is refused as a possible zip bomb",
                MAX_TOTAL_BYTES / (1024 * 1024 * 1024)
            ))
            .into());
        }
        files.put(relative, data);
    }

    // Java 保留枚举到的条目而不是再查一次名字，同样的理由在下面这个剥前缀的循环里也成立。
    if !prefix.is_empty() {
        dirs.retain(|dir| dir.starts_with(&prefix));
    }
    let mut stripped: Vec<String> = Vec::new();
    for dir in &dirs {
        let relative = strip_prefix(dir, &prefix);
        if !relative.is_empty() {
            stripped.push(relative);
        }
    }
    module.empty_dirs.extend(empty_dirs(&stripped, files.keys()));
    classify(&mut module, &files)?;
    Ok(module)
}

/// Java `ModPackageReader.singleRootPrefix`，包私有静态方法（golden 测试直接断言它）。
///
/// 游戏怎么剥包装目录，这里就怎么剥：`Mods.resolveRoot`（Mindustry `Mods.java:1080`）在顶层只有
/// **一个条目**时才进入它，并把 `META_FILES` 拼到其后去找元数据；两个并列目录时根本不往下找。
/// 照此办理，包装目录只在其内含**单个文件**时才被剥掉——那正是 `mods/` 里 jar 的形状。
/// 返回要剥掉的含斜杠前缀，无需剥时返回空串。
pub(crate) fn single_root_prefix(entry_names: &[String]) -> String {
    // Java 的 `LinkedHashSet<String> tops`：保序去重，所以用 KeyedMap 的键序（值用不到）。
    let mut tops: KeyedMap<()> = KeyedMap::new();
    for name in entry_names {
        // 顶层条目就是到第一个斜杠为止的内容；没有斜杠说明文件直接躺在根上，不能剥。
        let Some(slash) = name.find('/') else {
            return String::new();
        };
        let top = &name[..slash];
        if !tops.contains_key(top) {
            tops.put(top.to_string(), ());
        }
        if tops.len() > 1 {
            return String::new();
        }
    }
    if tops.len() != 1 {
        return String::new();
    }
    let only = tops.keys().next().expect("已经确认过恰有一个顶层条目");
    format!("{only}/")
}

/// Java `emptyDirs(List<String> directories, Set<String> filePaths)`，私有静态方法。
///
/// 一个目录只要在某个文件路径里当过祖先目录就不算空，所以内容**深深埋着**的目录（有文件在其下）
/// 不算空；而垃圾目录被排除，免得编辑器自己造出一堆空目录，下次构建时又被塞回包里。
/// 输入是模组相对路径（zip 的已剥掉包装前缀），输出按 `directories` 的原有顺序保序去重。
fn empty_dirs<'f>(directories: &[String], file_paths: impl Iterator<Item = &'f String>) -> Vec<String> {
    // Java 用 LinkedHashSet，但只按 contains 查询、从不迭代它，所以去重后的 Vec 完全等价。
    let mut with_files: Vec<String> = Vec::new();
    for path in file_paths {
        // Java: `while ((slash = path.lastIndexOf('/', slash - 1)) > 0)`，Rust 的 split 语义等价，
        // 这里逐次取「比 slash 更靠前的最后一个 '/'」。
        let mut slash = path.rfind('/');
        while let Some(at) = slash {
            if at == 0 {
                break;
            }
            let ancestor = &path[..at];
            if !with_files.contains(&ancestor.to_string()) {
                with_files.push(ancestor.to_string());
            }
            slash = ancestor.rfind('/');
        }
    }
    let mut empty: Vec<String> = Vec::new();
    // Java: `for (String dir : new LinkedHashSet<>(directories))`——保序去重后遍历。
    let mut visited: Vec<&str> = Vec::new();
    for dir in directories {
        if visited.contains(&dir.as_str()) {
            continue;
        }
        visited.push(dir.as_str());
        if !with_files.contains(dir) && !is_noise(&format!("{dir}/")) {
            empty.push(dir.clone());
        }
    }
    empty
}

/// Java `classify(ModPackage mod, Map<String, byte[]> files)`，私有静态方法。
///
/// 元数据、内容、bundle、资产的分派。除元数据外的文件都留在包里，所以原封不动的导入再输出时
/// 仍是逐字节一致的。
fn classify(module: &mut ModPackage, files: &KeyedMap<Vec<u8>>) -> Result<(), ImportError> {
    for candidate in ModMeta::META_FILES {
        let Some(bytes) = files.get(candidate) else {
            continue;
        };
        module.meta_file = candidate.to_string();
        module.meta_text = decode_text(bytes, candidate)?;
        match mm_json::read_object(&module.meta_text) {
            Ok(object) => module.meta = mod_meta_from_json(&object),
            Err(failure) => {
                module.meta_error = failure.to_string();
                module.skipped.push(format!("{candidate} (unreadable meta: {failure})"));
            }
        }
        break;
    }

    for (path, bytes) in files.iter() {
        let file_name = tail_name(path);
        // Java L300-303：垃圾不进来，但**要记进 `skipped`**，好让人看出包里有什么被刻意没带过来。
        if is_noise(path) {
            module.skipped.push(path.clone());
            continue;
        }
        if *path == module.meta_file {
            continue;
        }
        if path.starts_with(CONTENT)
            && let Some(content) = read_content(path, bytes)?
        {
            module.contents.push(content);
            continue;
        }
        if path.starts_with(BUNDLES) && is_bundle_file(file_name) {
            module.bundle_files.put(path.clone(), decode_text(bytes, path)?);
            continue;
        }
        module.asset_files.put(path.clone(), bytes.clone());
    }
    Ok(())
}

/// Java `readContent(String path, byte[] bytes)`，私有静态方法。
///
/// `content/<文件夹>/` 下的 JSON/HJSON 文件。类型由文件夹名决定，用游戏的宽松规则
/// （历史遗留的复数别名照样能用，Mindustry `Mods.java:877-885`）。非 JSON 后缀返回 null，
/// JSON 语法错误**不**返回 null：记录照样带出来，好让它作为问题显示出来，而不是被静默丢弃。
fn read_content(path: &str, bytes: &[u8]) -> Result<Option<SourceContent>, ImportError> {
    let file_name = tail_name(path);
    let dot = file_name.rfind('.');
    let ext = match dot {
        Some(at) => file_name[at + 1..].to_lowercase(),
        None => String::new(),
    };
    if ext != "json" && ext != "hjson" {
        return Ok(None);
    }
    let rest = &path[CONTENT.len()..];
    let slash = rest.find('/');
    // Java `slash <= 0` 覆盖「没找到（-1）」与「开头就是斜杠（0）」两种情况；
    // Rust 的 find 返回 Option<usize>，None 对应 -1，usize 无负数故 `at <= 0` 等价 `at == 0`。
    if slash.is_none_or(|at| at == 0) {
        return Ok(None);
    }
    let folder = &rest[..slash.expect("上面已确认存在正位置")];
    let Some(ctype) = ContentType::from_folder_loose(folder) else {
        return Ok(None);
    };
    let mut content = SourceContent {
        path: path.to_string(),
        name: match dot {
            Some(at) => file_name[..at].to_string(),
            None => file_name.to_string(),
        },
        ext,
        text: decode_text(bytes, path)?,
        ctype,
        fields: mm_json::Object::new(),
        parse_error: None,
    };
    // Java: `ext.equals("json") ? HjsonReader.readJsonObject(text) : HjsonReader.readObject(text)`
    let parsed = if content.ext == "json" {
        mm_json::read_json_object(&content.text)
    } else {
        mm_json::read_object(&content.text)
    };
    match parsed {
        // Java 把 JsonObject 的 entrySet() 依序 put 进 LinkedHashMap，这里依序 insert 进保序 Object。
        Ok(object) => {
            for (key, value) in object.iter() {
                content.fields.insert(key.clone(), value.clone());
            }
        }
        Err(failure) => content.parse_error = Some(failure.to_string()),
    }
    Ok(Some(content))
}

/// Java `isUnsafeEntryName(String name)`，私有静态方法。
///
/// 货架构建（`MindustryHacks.buildFromWorkspace`）会把条目写到 `archives/<name>` 之下，
/// 所以「干净」的名字定义为**没有任何路径段逃出 `archives/`**：穿越段 `..`、以斜杠开头的名字、
/// 盘符（`C:`）、任何控制字符或 Windows 非法字符（`<>:"|?*`）、以及以 `.` 或空格结尾的段
/// （它们会被 Windows 无声地剪掉，让一个名字撞进另一个）。
fn is_unsafe_entry_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') {
        return true;
    }
    // Java: name.matches("(?i)^[a-z]:.*")（不引 regex 依赖，手写等价判定）。
    // 注意 Java 的 `.` 不跨换行，而带换行的名字必定在下面「控制字符」这一关被拒，两者结论一致。
    {
        let mut chars = name.chars();
        if let (Some(first), Some(':')) = (chars.next(), chars.next())
            && first.is_ascii_alphabetic()
        {
            return true;
        }
    }
    // Java 的 `split("/", -1)` 与 Rust 的 `split('/')` 一致：保留空段。
    for segment in name.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == ".." {
            return true;
        }
        for c in segment.chars() {
            // Java: `c < 0x20 || "<>:\"|?*".indexOf(c) >= 0`
            if c < '\u{20}' || "<>:\"|?*".contains(c) {
                return true;
            }
        }
        let last = segment.chars().next_back().expect("段非空");
        if last == '.' || last == ' ' {
            return true;
        }
    }
    false
}

/// Java `isNoise(String path)`，包私有静态方法（golden 测试直接断言它）。
///
/// Java 里它是 `ModPackageReader` 的包私有静态方法（`ModPackageReader.java` 的 isNoise），
/// 同包的测试可以直接调用；Rust 的集成测试二进制是独立 crate，看不见 `pub(crate)`，
/// 故放宽为 `pub`。这与 `fixture_project.rs` 里 `has_meta_file` 的既有先例一致。
///
/// 只丢明确已知的操作系统与版本控制垃圾，其余一律保留，好让输出仍是逐字节一致的。
/// 名字与 `ModPackageReader.NOISE_FILES`/`NOISE_DIRS` 一致（和 `ProjectIo` 一样重复了一份）。
pub fn is_noise(path: &str) -> bool {
    let file_name = tail_name(path);
    if NOISE_FILES.contains(&file_name) || file_name.ends_with(TEMP_SUFFIX) {
        return true;
    }
    for dir in NOISE_DIRS {
        // Java L407：`path.startsWith(dir) || path.contains("/" + dir)`——垃圾目录既可能躺在包根，
        // 也可能嵌在包装目录之下（zip 里 `MyMod/.git/config` 这种形态很常见）。
        if path.starts_with(dir) || path.contains(&format!("/{dir}")) {
            return true;
        }
    }
    false
}

/// Java 的 `new String(bytes, StandardCharsets.UTF_8)` **不再照搬**（2026-11-20 v1 口径）。
///
/// Java 的解码器用 REPLACE 策略：坏字节按「最大合法子序列」换成 U+FFFD。那张字符串是要写回
/// 磁盘的（未修改的记录按原文重写），于是「一个没被碰过的文件」保存后就和原文不一样了——这正是
/// v1 要禁掉的静默改写。这里改成严格解码：不是合法 UTF-8 就报 [`ImportError::Encoding`]，
/// 由壳映射成 `encoding` 错误码。BOM（U+FEFF）与 CRLF 本身是合法 UTF-8，照旧逐字节保留。
fn decode_text(bytes: &[u8], path: &str) -> Result<String, ImportError> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_string()),
        Err(failure) => Err(ImportError::Encoding(format!(
            "'{path}' is not valid UTF-8 (byte offset {}); this editor refuses to rewrite it \
             rather than silently replacing bytes with U+FFFD",
            failure.valid_up_to()
        ))),
    }
}

/// Java 的 `path.substring(path.lastIndexOf('/') + 1)`：最后一个斜杠之后的文件名。
fn tail_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) => &path[at + 1..],
        None => path,
    }
}

/// Java 的 `name.endsWith("/") ? name.substring(0, name.length() - 1) : name`。
fn strip_trailing_slash(name: &str) -> String {
    match name.strip_suffix('/') {
        Some(rest) => rest.to_string(),
        None => name.to_string(),
    }
}

/// Java 的 `prefix.isEmpty() ? name : name.substring(prefix.length())`（前缀已由调用方确认可剥）。
fn strip_prefix(name: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return name.to_string();
    }
    name[prefix.len()..].to_string()
}

/// Java 的 `path.toAbsolutePath().toString()`：只绝对化、不 normalize。
fn absolute_label(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

/// Java 在预算超限时抛的 `IOException`：本 crate 统一用 `io::Result`，故包一层。
fn budget_error(message: std::fmt::Arguments<'_>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_string())
}

/// Java `readFolder` 里那次「先枚举、后读字节」的枚举通道状态。
///
/// 两趟的理由（照 Java 注释）：一边遍历一边读的话就事先不知道文件总数，
/// UI 只能显示一根不确定的进度条；枚举这趟很便宜（每个条目一次 stat），
/// 于是 READING 阶段变成可度量的。
#[derive(Default)]
struct FolderScan {
    dirs: Vec<String>,
    file_names: Vec<String>,
    file_paths: Vec<PathBuf>,
    total_bytes: u64,
    entries: usize,
}

impl FolderScan {
    /// 前序深度优先（Java `Files.walk(root)` 的遍历形状），且不跟随符号链接：Java 侧没有传
    /// `FOLLOW_LINKS`，链接本身既不算目录也不算普通文件，于是整条被跳过。
    /// 读不到某个目录时直接把错误抛给调用方，等价于 Java 把 `UncheckedIOException` 的
    /// cause 还原成 `IOException`。
    fn walk(&mut self, root: &Path, dir: &Path) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            // Java: root.relativize(path).toString().replace('\\', '/')
            let relative = relative_to(root, &path);
            if relative.is_empty() {
                continue;
            }
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return Err(budget_error(format_args!(
                    "the folder holds more than {MAX_ENTRIES} entries and is refused as a possible zip bomb"
                )));
            }
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                self.dirs.push(relative);
                self.walk(root, &path)?;
            } else if file_type.is_file() {
                let size = path.metadata()?.len();
                if size > MAX_ENTRY_BYTES {
                    return Err(budget_error(format_args!(
                        "'{relative}' exceeds {} MiB and is refused",
                        MAX_ENTRY_BYTES / (1024 * 1024)
                    )));
                }
                self.total_bytes += size;
                if self.total_bytes > MAX_TOTAL_BYTES {
                    return Err(budget_error(format_args!(
                        "the folder unpacks to more than {} GiB and is refused as a possible zip bomb",
                        MAX_TOTAL_BYTES / (1024 * 1024 * 1024)
                    )));
                }
                self.file_names.push(relative);
                self.file_paths.push(path);
            }
        }
        Ok(())
    }
}

/// Java 的 `root.relativize(path).toString()`：遍历得到的路径就在 root 之下，
/// `strip_prefix` 与 `relativize` 在这里等价。
fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::scratch::temp_case;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// 用例目录：落在 `crates/mm-io/target/mm-io-tests` 之下（本机沙箱禁止在 `%TEMP%` 建目录），
    /// 用 RAII 保证断言 panic 时也能把目录清掉。
    struct Case(PathBuf);

    impl Case {
        fn new(tag: &str) -> Self {
            Case(temp_case(tag))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Case {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 造一个 zip：目录条目先写、文件条目按给定顺序写。用 Stored（不压缩），
    /// 这样「读回的字节 == 写入的字节」测的是本层，而不是 deflate 的往返。
    fn make_zip(
        case: &Case,
        name: &str,
        entries: &[(&str, &[u8])],
        directories: &[&str],
    ) -> PathBuf {
        let path = case.path().join(name);
        let mut writer = zip::ZipWriter::new(
            fs::File::create(&path)
                .unwrap_or_else(|e| panic!("创建 zip {} 失败：{e}", path.display())),
        );
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for dir in directories {
            writer.add_directory(*dir, options).expect("写目录条目失败");
        }
        for (entry, bytes) in entries {
            writer.start_file(*entry, options).expect("写文件条目失败");
            writer.write_all(bytes).expect("写条目字节失败");
        }
        writer.finish().expect("收尾 zip 失败");
        path
    }

    /// 目录通道：在用例目录里按模组相对路径落一个文件。
    fn write_at(case: &Case, relative: &str, bytes: &[u8]) {
        let path = case
            .path()
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("建父目录失败");
        }
        fs::write(&path, bytes).unwrap_or_else(|e| panic!("写文件失败：{e}"));
    }

    fn asset_paths(module: &ModPackage) -> Vec<&str> {
        module.asset_files.keys().map(String::as_str).collect()
    }

    #[test]
    fn single_root_prefix_only_unwraps_one_top_folder() {
        let wrapped = vec![
            "MyMod/mod.json".to_string(),
            "MyMod/content/units/titan.json".to_string(),
        ];
        assert_eq!(single_root_prefix(&wrapped), "MyMod/");

        // 顶层直接躺着文件：游戏不会往下找，所以不剥。
        assert_eq!(single_root_prefix(&["mod.json".to_string()]), "");

        // 顶层有两个并列条目：同样不剥。
        let two = vec!["A/mod.json".to_string(), "B/mod.json".to_string()];
        assert_eq!(single_root_prefix(&two), "");

        // 唯一顶层目录里再嵌一层：顶层仍只有一个条目，按 resolveRoot 照样进得去。
        let nested = vec!["MyMod/a/x.json".to_string(), "MyMod/b/y.json".to_string()];
        assert_eq!(single_root_prefix(&nested), "MyMod/");

        assert_eq!(single_root_prefix(&[]), "");
    }

    #[test]
    fn zip_unwraps_single_top_folder() {
        let case = Case::new("zip-unwrap");
        let zip = make_zip(
            &case,
            "wrapped.zip",
            &[
                ("MyMod/mod.json", b"{\"name\":\"wrapped\"}"),
                ("MyMod/content/blocks/ore.json", b"{\"name\":\"ore\"}"),
                ("MyMod/sprites/ore.png", b"\x89PNG\r\n\x1a\n"),
            ],
            &["MyMod", "MyMod/sprites", "MyMod/textures"],
        );

        let module = read(&zip).expect("读带包装目录的 zip");
        assert!(module.from_zip);
        assert_eq!(module.root_prefix, "MyMod/");
        assert_eq!(module.meta_file, "mod.json");
        assert_eq!(module.meta.name, "wrapped");
        assert_eq!(module.contents.len(), 1);
        assert_eq!(module.contents[0].path, "content/blocks/ore.json");
        assert_eq!(module.contents[0].ctype, ContentType::Block);
        // 剥掉包装目录后，所有键都是模组相对路径。
        assert_eq!(asset_paths(&module), vec!["sprites/ore.png"]);
        // "MyMod" 这一层被前缀过滤器整个去掉；"MyMod/sprites" 有文件在下面，不算空。
        assert_eq!(module.empty_dirs, vec!["textures"]);
        assert!(Path::new(&module.source_label).is_absolute());
    }

    #[test]
    fn zip_keeps_root_when_top_level_has_several_entries() {
        let case = Case::new("zip-multi-top");
        let zip = make_zip(
            &case,
            "multi.zip",
            &[
                ("A/mod.json", b"{\"name\":\"a\"}"),
                ("B/mod.json", b"{\"name\":\"b\"}"),
            ],
            &[],
        );

        let module = read(&zip).expect("读多顶层条目 zip");
        assert_eq!(module.root_prefix, "");
        // 根上没有元数据候选名，两个文件都作为资产留着（Java 不校验、不丢弃）。
        assert_eq!(module.meta_file, "");
        assert_eq!(asset_paths(&module), vec!["A/mod.json", "B/mod.json"]);
    }

    #[test]
    fn zip_keeps_root_when_a_file_lies_at_top_level() {
        let case = Case::new("zip-no-wrap");
        let zip = make_zip(
            &case,
            "flat.zip",
            &[
                ("content/items/card.json", b"{\"name\":\"card\"}"),
                ("mod.json", b"{\"name\":\"flat\"}"),
            ],
            &[],
        );

        let module = read(&zip).expect("读无包装目录 zip");
        assert_eq!(module.root_prefix, "");
        assert_eq!(module.meta_file, "mod.json");
        assert_eq!(module.contents.len(), 1);
        assert_eq!(module.contents[0].ctype, ContentType::Item);
        assert!(module.asset_files.is_empty());
    }

    #[test]
    fn meta_file_names_are_tried_in_the_games_order() {
        // Java: ModMeta.META_FILES = mod.json, mod.hjson, plugin.json, plugin.hjson（Mods.java:34）。
        let cases: &[(&[&str], &str)] = &[
            (&["mod.json", "mod.hjson"], "mod.json"),
            (&["mod.hjson", "plugin.json"], "mod.hjson"),
            (&["plugin.json", "plugin.hjson"], "plugin.json"),
            (&["plugin.hjson"], "plugin.hjson"),
            (&["plugin.hjson", "mod.hjson", "plugin.json"], "mod.hjson"),
        ];
        for (present, winner) in cases {
            let case = Case::new("meta-order");
            let texts: Vec<String> = present
                .iter()
                // 每个候选文件的 name 字段就是它自己，用来证明「取的是哪一个文件」。
                .map(|name| format!("{{\"name\":\"{name}\"}}"))
                .collect();
            let entries: Vec<(&str, &[u8])> = present
                .iter()
                .zip(&texts)
                .map(|(name, text)| (*name, text.as_bytes()))
                .collect();
            let zip = make_zip(&case, "meta.zip", &entries, &[]);

            let module = read(&zip).expect("读元数据候选 zip");
            assert_eq!(module.meta_file, *winner, "present={present:?}");
            assert_eq!(module.meta.name, *winner);
            assert!(module.meta_error.is_empty());
            // 元数据文件本身不进资产，其余候选名作为资产原样保留。
            let assets = asset_paths(&module);
            assert!(!assets.contains(winner), "winner={winner}");
            for name in *present {
                if *name != *winner {
                    assert!(assets.contains(name), "present={present:?}");
                }
            }
        }
    }

    #[test]
    fn noise_files_and_temp_suffix_entries_are_dropped() {
        let case = Case::new("zip-noise");
        let zip = make_zip(
            &case,
            "noise.zip",
            &[
                ("mod.json", b"{\"name\":\"ok\"}"),
                (".gitignore", b"*.mm-tmp\n"),
                ("Thumbs.db", b"junk"),
                ("sub/desktop.ini", b"junk"),
                ("notes.mm-tmp", b"half written"),
                ("__MACOSX/._mod.json", b"junk"),
                (".git/config", b"junk"),
                ("modmaker/draft.json", b"junk"),
                // 嵌在包装目录之下的垃圾目录：Java 的 `contains("/" + dir)` 那一半。
                ("deep/.idea/workspaces.xml", b"junk"),
                ("textures/a.png", b"\x00\xff"),
            ],
            &[],
        );

        let module = read(&zip).expect("读带垃圾条目的 zip");
        assert_eq!(module.meta_file, "mod.json");
        // 只剩真正属于模组的资产。
        assert_eq!(asset_paths(&module), vec!["textures/a.png"]);
        assert!(module.bundle_files.is_empty());
        assert!(module.contents.is_empty());
        // Java L300-303：每个被丢弃的垃圾路径都按插入序记进 `skipped`。
        assert_eq!(
            module.skipped,
            vec![
                ".gitignore".to_string(),
                "Thumbs.db".to_string(),
                "sub/desktop.ini".to_string(),
                "notes.mm-tmp".to_string(),
                "__MACOSX/._mod.json".to_string(),
                ".git/config".to_string(),
                "modmaker/draft.json".to_string(),
                "deep/.idea/workspaces.xml".to_string(),
            ]
        );
    }

    #[test]
    fn text_channel_keeps_bom_and_crlf_untouched() {
        let case = Case::new("zip-text");
        let meta = "{\"name\":\"ok\"}";
        let content = "\u{FEFF}{\r\n  \"name\": \"titan\"\r\n}";
        let bundle = "\u{FEFF}unit.titan.name=泰坦\r\nother=x\r\n";
        let zip = make_zip(
            &case,
            "text.zip",
            &[
                ("mod.json", meta.as_bytes()),
                ("content/units/titan.json", content.as_bytes()),
                ("bundles/bundle_zh_CN.properties", bundle.as_bytes()),
            ],
            &[],
        );

        let module = read(&zip).expect("读文本通道 zip");
        assert_eq!(module.meta_text, meta);
        assert_eq!(module.contents.len(), 1);
        let record = &module.contents[0];
        assert_eq!(record.path, "content/units/titan.json");
        assert_eq!(record.name, "titan");
        assert_eq!(record.ext, "json");
        assert_eq!(record.ctype, ContentType::Unit);
        // BOM 留在串首、CRLF 一个都没变成 LF。
        assert_eq!(record.text, content);
        assert!(record.text.starts_with('\u{FEFF}'));
        assert!(record.text.contains("\r\n"));
        // bundle 走的是同一套解码，同样原样。
        let stored = module.bundle_files.get("bundles/bundle_zh_CN.properties");
        assert_eq!(stored, Some(&bundle.to_string()));
        // 三个文件都归到了文本通道，资产通道不该被误当二进制读走。
        assert!(module.asset_files.is_empty());
    }

    #[test]
    fn asset_bytes_are_carried_verbatim() {
        let case = Case::new("zip-asset");
        // 含 NUL、非法 UTF-8 序列与 CRLF 字节的「二进制」内容：只能按字节进、按字节出。
        let bytes: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0xff, 0xfe, 0x41];
        let zip = make_zip(
            &case,
            "asset.zip",
            &[("mod.json", b"{\"name\":\"ok\"}"), ("sprites/px.png", bytes)],
            &[],
        );

        let module = read(&zip).expect("读资产 zip");
        assert_eq!(
            module.asset_files.get("sprites/px.png"),
            Some(&bytes.to_vec()),
            "资产必须逐字节一致"
        );
    }

    #[test]
    fn empty_dirs_are_recorded_and_dirs_with_files_are_not() {
        let case = Case::new("zip-empty-dirs");
        let zip = make_zip(
            &case,
            "dirs.zip",
            &[
                ("mod.json", b"{\"name\":\"ok\"}"),
                ("content/blocks/ore.json", b"{\"name\":\"ore\"}"),
            ],
            &["textures", "content/blocks", "content"],
        );

        let module = read(&zip).expect("读带空目录的 zip");
        // content 与 content/blocks 都是某个文件路径的祖先，所以不算空；只有 textures 是空的。
        assert_eq!(module.empty_dirs, vec!["textures".to_string()]);
    }

    #[test]
    fn unreadable_meta_keeps_the_rest_of_the_mod() {
        let case = Case::new("zip-bad-meta");
        let zip = make_zip(
            &case,
            "badmeta.zip",
            &[
                ("mod.json", b"[1,2]"),
                ("content/units/titan.json", b"{\"name\":\"titan\"}"),
            ],
            &[],
        );

        let module = read(&zip).expect("读元数据解析不了的 zip");
        assert_eq!(module.meta_file, "mod.json");
        assert!(!module.meta_error.is_empty());
        assert_eq!(module.skipped.len(), 1);
        assert!(module.skipped[0].starts_with("mod.json (unreadable meta:"));
        // 元数据坏了不影响其余部分载入。
        assert_eq!(module.contents.len(), 1);
        assert_eq!(module.contents[0].ctype, ContentType::Unit);
    }

    #[test]
    fn content_parse_error_is_carried_not_dropped() {
        let case = Case::new("zip-bad-content");
        let zip = make_zip(
            &case,
            "badcontent.zip",
            &[
                ("mod.json", b"{\"name\":\"ok\"}"),
                ("content/units/broken.json", b"[1,2]"),
            ],
            &[],
        );

        let module = read(&zip).expect("读内容文件语法坏的 zip");
        assert_eq!(module.contents.len(), 1);
        let record = &module.contents[0];
        assert!(record.parse_error.is_some());
        assert!(record.fields.is_empty());
        assert_eq!(record.text, "[1,2]");
    }

    #[test]
    fn windows_backslash_entry_names_still_find_the_meta() {
        let case = Case::new("zip-backslash");
        // Windows 工具会把条目名存成反斜杠；Java 侧保留「枚举到的那一条」正是为了这种情况。
        let zip = make_zip(
            &case,
            "backslash.zip",
            &[("MyMod\\mod.json", b"{\"name\":\"backslash\"}")],
            &[],
        );

        let module = read(&zip).expect("读反斜杠条目名 zip");
        assert_eq!(module.root_prefix, "MyMod/");
        assert_eq!(module.meta_file, "mod.json");
        assert_eq!(module.meta.name, "backslash");
    }

    #[test]
    fn folder_channel_reads_without_unwrapping_anything() {
        let case = Case::new("folder-read");
        write_at(&case, "mod.json", b"{\"name\":\"folder\"}");
        write_at(&case, "content/blocks/ore.json", b"{\"name\":\"ore\"}");
        write_at(&case, "sprite.png", b"\x89PNG\r\n");
        write_at(&case, ".gitignore", b"target\n");
        write_at(&case, "bundles/bundle.properties", b"unit.x.name=X\r\n");
        fs::create_dir_all(case.path().join("textures")).expect("建空目录失败");

        let module = read(case.path()).expect("读目录模组");
        assert!(!module.from_zip);
        // 普通目录导入的根就是这个目录本身，所以什么都不剥。
        assert_eq!(module.root_prefix, "");
        assert_eq!(module.meta_file, "mod.json");
        assert_eq!(module.meta.name, "folder");
        assert_eq!(module.contents.len(), 1);
        assert_eq!(module.contents[0].path, "content/blocks/ore.json");
        assert_eq!(module.contents[0].ctype, ContentType::Block);
        assert_eq!(asset_paths(&module), vec!["sprite.png"]);
        assert_eq!(
            module.bundle_files.get("bundles/bundle.properties"),
            Some(&"unit.x.name=X\r\n".to_string())
        );
        assert_eq!(module.empty_dirs, vec!["textures".to_string()]);
        // 目录通道同样把丢掉的垃圾记进 skipped（Java L301 不看通道）。
        assert_eq!(module.skipped, vec![".gitignore".to_string()]);
        assert!(Path::new(&module.source_label).is_absolute());
    }

    #[test]
    fn is_noise_matches_files_suffixes_and_dir_prefixes() {
        for name in NOISE_FILES {
            assert!(is_noise(name), "{name}");
            assert!(is_noise(&format!("sprites/{name}")), "{name}");
        }
        // Java L400 的另一半：以 ProjectIo.TEMP_SUFFIX 结尾的名字。
        assert!(is_noise("notes.mm-tmp"));
        assert!(is_noise(&format!("sub/x{}", TEMP_SUFFIX)));
        for dir in NOISE_DIRS {
            assert!(is_noise(&format!("{dir}inside.json")), "{dir}");
            // Java L407 的另一半：垃圾目录嵌在别之下也算。
            assert!(is_noise(&format!("MyMod/{dir}inside.json")), "{dir}");
        }
        assert!(!is_noise("mod.json"));
        assert!(!is_noise("sprites/unit-titan.png"));
    }

    #[test]
    fn unsafe_entry_names_are_refused() {
        assert!(is_unsafe_entry_name(""));
        assert!(is_unsafe_entry_name("/absolute/mod.json"));
        assert!(is_unsafe_entry_name("../escape.json"));
        assert!(is_unsafe_entry_name("content/../escape.json"));
        assert!(is_unsafe_entry_name("C:/mod.json"));
        assert!(is_unsafe_entry_name("c:\\mod.json"));
        // 以 '.' 或空格结尾的段会被 Windows 无声剪掉，于是撞进另一个名字。
        assert!(is_unsafe_entry_name("sprites/ore."));
        assert!(is_unsafe_entry_name("sprites/ore "));
        assert!(is_unsafe_entry_name("sprites/od|d.png"));
        assert!(is_unsafe_entry_name("sprites/a\u{7}b.png"));
        assert!(!is_unsafe_entry_name("mod.json"));
        assert!(!is_unsafe_entry_name("content/units/titan-2.json"));
        assert!(!is_unsafe_entry_name("bundles/bundle_zh_CN.properties"));
    }

    /// v1 口径：内容文本不是合法 UTF-8 时显式失败（`encoding`），不再静默换成 U+FFFD。
    #[test]
    fn a_non_utf8_content_file_is_refused_with_an_encoding_error() {
        let case = Case::new("non-utf8-content");
        write_at(&case, "mod.json", br#"{"name":"broken"}"#);
        // 0xFF 永远不是合法 UTF-8 的首字节。
        let mut bytes = b"name: bad\xFF".to_vec();
        bytes.extend_from_slice(b"\n");
        write_at(&case, "content/items/bad.json", &bytes);

        let failure = read(case.path()).expect_err("非 UTF-8 内容必须被拒绝");
        match failure {
            ImportError::Encoding(message) => {
                assert!(message.contains("content/items/bad.json"), "{message}");
                assert!(message.contains("not valid UTF-8"), "{message}");
            }
            other => panic!("期望 Encoding，实际：{other:?}"),
        }
    }

    /// 二进制资产不走文本通道：坏字节按原样搬进 `asset_files`，保存时逐字节写回。
    #[test]
    fn a_non_utf8_asset_is_carried_verbatim() {
        let case = Case::new("binary-asset");
        write_at(&case, "mod.json", br#"{"name":"binary"}"#);
        let png = [0x89u8, b'P', b'N', b'G', 0xFF, 0xFE, 0x00];
        write_at(&case, "sprites/bad-0.png", &png);

        let module = read(case.path()).expect("二进制资产不该触发编码错误");
        assert_eq!(module.asset_files.get("sprites/bad-0.png"), Some(&png.to_vec()));
    }
}
