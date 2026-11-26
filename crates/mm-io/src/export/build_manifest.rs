//! `BuildManifest.java` 的等价移植（`dev.modmaker.core.io.BuildManifest`，158 行）：
//! 写在构建产物**旁边**的溯源记录。
//!
//! 清单被刻意放在 mod **之外**：zip 与目录树保持项目原样，于是「项目内容 = 构建内容」这句话
//! 逐字节成立，从构建里解包出来的 mod 也能再次导入而不会多出一个凭空造出的文件。
//!
//! # 与 Java 的三处表达差异（都不改变可观察行为）
//!
//! 1. **SHA-256**：Java 用 `MessageDigest.getInstance("SHA-256")`。本 crate 不允许新增依赖
//!    （mm-io 只有 mm-json / mm-model / mm-schema / mm-fmt / mm-validate / zip），故在下面
//!    自带一份 FIPS 180-4 的纯 Rust 实现，按 64 KiB 分块喂入，与 Java 的
//!    `digest.update(buffer, 0, read)` 流式用法逐块对齐；`newDigest()` 那条
//!    `IllegalStateException("SHA-256 is required by the Java platform")` 分支在 Rust 里
//!    由类型系统排除（不存在「算法不可用」这种状态），故无对应错误。
//! 2. **`Json.write`**：Java 第 82 行走的是 Gson 通道，Rust 侧对应 `mm_json::write_pretty`
//!    （**不是** `mm_json::write`，后者是 Hjson 保真通道）。
//! 3. **`Instant.now().truncatedTo(SECONDS).toString()`**：没有 chrono，故用 std 的
//!    [`SystemTime`] + 下面的 [`civil_from_days`] 复刻那个 ISO-8601 文本（UTC、秒精度、`Z` 结尾）。
//!    与 Java 一样「截断」而非四舍五入：`div_euclid` 对负 epoch 也向下取整。
//!
//! 数字字段（`format` / `files` / `bytes`）在 Java 里是 `int` / `long` 经 Gson 写成 JSON number；
//! Rust 侧 `Json::Num` 存的是**源码文本**（等价 Gson 保留原文），所以这里写成十进制字符串，
//! 输出即不带引号的整数。

use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use mm_json::{write_pretty, Json, Object};
use mm_model::ModProject;

use super::mod_exporter::{absolute_unnormalized, equals_ignore_case, java_relativize, to_slash};
use crate::project::{AUTHORING_DIR, TEMP_SUFFIX};

/// Java `BuildManifest.FORMAT`。
pub const FORMAT: i32 = 1;
/// Java `BuildManifest.TOOL`。
pub const TOOL: &str = "Mindustry ModMaker";
/// Java `BuildManifest.TOOL_VERSION`：与 Gradle 项目版本保持同步，`--version` 也打印它。
pub const TOOL_VERSION: &str = "0.1.0";

/// Java `create(ModProject, String, int, long, String, String)`（第 49-74 行）：
/// 造出清单对象。哈希是带 `sha256:` 前缀的十六进制串，与已发布的格式一致。
///
/// Java 返回 `JsonObject`，Rust 返回 [`Json`]（其 `Json::Obj` 分支，成员顺序由 [`Object`] 保证，
/// 等价 Gson `JsonObject` 的 `LinkedTreeMap` 插入序）。成员插入顺序**逐行照抄** Java 第 52-72 行。
///
/// Java 里 `project.meta.version == null ? "" : ...` 与 `dependencies != null` 这两条 null 兜底，
/// 在 Rust 侧由非 optional 的 `String` / `Vec<String>` 排除（与 mm-json 既有处理一致）。
pub fn create(
    project: &ModProject,
    schema_profile: &str,
    files: i32,
    bytes: i64,
    source_hash: &str,
    artifact_hash: &str,
) -> Json {
    let mut manifest = Object::new();
    manifest.insert("format", Json::num(FORMAT.to_string()));
    manifest.insert("tool", Json::str(TOOL));
    manifest.insert("toolVersion", Json::str(TOOL_VERSION));
    let mut module = Object::new();
    module.insert("name", Json::str(project.internal_name()));
    module.insert("version", Json::str(project.meta.version.clone()));
    module.insert("minGameVersion", Json::str(project.meta.min_game_version.clone()));
    manifest.insert("mod", Json::Obj(module));
    manifest.insert("schemaProfile", Json::str(schema_profile));
    manifest.insert(
        "dependencies",
        Json::Arr(project.meta.dependencies.iter().map(|name| Json::str(name.clone())).collect()),
    );
    manifest.insert("builtAt", Json::str(built_at_now()));
    manifest.insert("sourceHash", Json::str(source_hash));
    manifest.insert("files", Json::num(files.to_string()));
    manifest.insert("bytes", Json::num(bytes.to_string()));
    manifest.insert("sha256", Json::str(artifact_hash));
    Json::Obj(manifest)
}

/// Java `write(JsonObject, Path)`（第 77-83 行）：以 UTF-8 JSON 写出清单，需要时创建父目录。
///
/// Java 第 82 行的文本是 `Json.write(manifest) + "\n"`：那个 `Json.write` 是本项目自己的 helper
/// （Gson pretty printing + `disableHtmlEscaping()`，等价于 [`mm_json::write_pretty`]，见该函数
/// 文档），末尾的 `"\n"` 是实打实写进文件的一个换行，一个都不能少。
///
/// 这里**不是**原子写：Java 用 `Files.writeString`（CREATE + TRUNCATE_EXISTING + WRITE），
/// 所以 Rust 也直接 `fs::write`，不换 `AtomicFiles`——那是 `ProjectIo` 的保存路径才有的语义。
/// `toAbsolutePath()` 同样不做 normalize（与 [`absolute_unnormalized`] 一致）。
pub fn write(manifest: &Json, target: &Path) -> io::Result<()> {
    let absolute = absolute_unnormalized(target);
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&absolute, format!("{}\n", write_pretty(manifest)))
}

/// Java `sha256(Path)`（第 86-96 行）：单个文件的 SHA-256，形如 `sha256:<hex>`。
pub fn sha256(file: &Path) -> io::Result<String> {
    Ok(format!("sha256:{}", hex_lower(&digest_file(file)?)))
}

/// Java `sourceHash(Path)`（第 103-105 行）：项目源文件的稳定哈希——相对路径 + 内容，按路径序。
/// 创作目录与 save/crash 临时文件都被排除，于是这个哈希正好描述一次构建会打包的东西。
pub fn source_hash(project_root: &Path) -> io::Result<String> {
    hash_tree(project_root, true)
}

/// Java `treeHash(Path)`（第 108-110 行）：一个已构建目录的稳定哈希——`folder` 下的每个文件，按路径序。
pub fn tree_hash(folder: &Path) -> io::Result<String> {
    hash_tree(folder, false)
}

/// Java `hashTree(Path, boolean)`（第 112-143 行）。
fn hash_tree(root: &Path, exclude_authoring: bool) -> io::Result<String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    super::mod_exporter::walk_tree(root, &mut dirs, &mut files);
    let mut relatives: Vec<String> = Vec::new();
    for file in &files {
        // Java 第 118 行 `file.getFileName().toString().endsWith(TEMP_SUFFIX)`：walk 里的
        // regular file 必有文件名，故 `unwrap_or_default()`（空串）只会命中不可能分支。
        let ends_with_temp = file
            .file_name()
            .map(|name| name.to_string_lossy().ends_with(TEMP_SUFFIX))
            .unwrap_or_default();
        if ends_with_temp {
            continue;
        }
        if exclude_authoring && is_authoring(root, file) {
            continue;
        }
        relatives.push(to_slash(&java_relativize(root, file)));
    }
    // Java 第 119 行的 `.sorted()`（`Path` 的自然序）随后就被第 126 行
    // `relatives.sort(String::compareTo)` 完全覆盖：两次排序的键互不相同的可能性为零
    // （相对路径互不相等），故最终次序只由这次按相对路径字符串的排序决定，这里只做后者。
    //
    // Rust 的 `str::Ord` 按 UTF-8 字节序，Java 的 `String.compareTo` 按 UTF-16 码元序：
    // 对 BMP 字符两者给出的次序相同（UTF-8 与码点序同构，BMP 内码点序即码元序），
    // 只有「增补平面字符 vs U+E000-U+FFFF」这一类的相对次序可能不同。mod 里的相对路径
    // 是 ASCII 文件名，实际不触及该差异，故不做 UTF-16 往返。
    relatives.sort();

    let mut digest = Sha256::new();
    for relative in &relatives {
        // Java 第 128-129 行的注释：再按相对路径排一次，因为 `Files.walk` 的次序取决于文件系统，
        // 而哈希不能取决于它。
        digest.update(relative.as_bytes());
        digest.update(&[0u8]);
        let file = root.join(relative);
        digest.update(&digest_file(&file)?);
        digest.update(&[0u8]);
    }
    Ok(format!("sha256:{}", hex_lower(&digest.finish())))
}

/// Java `isAuthoring(Path, Path)`（第 145-149 行）。
///
/// Java 取的是 `relative.getName(0)`，即**首段**，不是 `getFileName()` 的末段，
/// 所以这里用 `first_name` 而不是 `Path::file_name`。
fn is_authoring(root: &Path, file: &Path) -> bool {
    let relative = java_relativize(root, file);
    match first_name(&relative) {
        Some(name) => equals_ignore_case(&name.to_string_lossy(), AUTHORING_DIR),
        // Java `getNameCount() > 0` 为假时的短路：不视作创作层文件。
        None => false,
    }
}

/// Java `Path.getName(0)`（前提是 `getNameCount() > 0`）：首个名字元素。
fn first_name(path: &Path) -> Option<&std::ffi::OsStr> {
    path.components().find_map(|component| match component {
        Component::Normal(name) => Some(name),
        _ => None,
    })
}

/// Java `sha256(Path)` / `hashTree` 里那段 64 KiB 缓冲的流式读取（第 88-93、133-138 行），
/// 返回**原始**摘要字节：`hashTree` 那边是 `digest.update(digestOf(file))`，喂的是裸字节而非十六进制。
fn digest_file(file: &Path) -> io::Result<[u8; 32]> {
    let mut input = fs::File::open(file)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finish())
}

/// Java `Instant.now().truncatedTo(ChronoUnit.SECONDS).toString()`（第 67-68 行）。
fn built_at_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        // 只有系统时钟早于 epoch 才走到这个兜底，实际等价于不可达分支（同一处理见
        // `crate::autosave` 里取 `System.currentTimeMillis()` 的那处）。
        .unwrap_or_default();
    epoch_seconds_to_iso8601(now.as_secs() as i64)
}

/// 把「自 1970-01-01T00:00:00Z 起的秒数」写成 `Instant.toString()` 在整秒时的形状：
/// `2026-11-12T13:45:30Z`（UTC、秒精度、定宽字段、`Z` 结尾）。
fn epoch_seconds_to_iso8601(seconds: i64) -> String {
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let second_of_day = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60
    )
}

/// 以 3 月为岁首的第 `y` 年（天文纪年，`y = 0` 即公元前 1 年）的第一天距离**起点 0000-03-01** 的天数。
/// 平年 365 天，再补上 `(0, y]` 之间每一个闰年的 2 月 29 日——`y/4 - y/100 + y/400` 正是那个个数，
/// 因为「岁首年 `y`」的区间 `(0000-03-01, y-03-01]` 收尾于 3 月 1 日，把公历第 `y` 年自己的
/// 2 月 29 日也包含在内（`y` 是 400 的倍数时亦然，这也是它和只写 `+ y/4 - y/100` 的
/// 「世纪内序号」版本唯一的区别）。`div_euclid` 保证对负数取下整，与公历的闰年规则同构。
fn march_year_start(y: i64) -> i64 {
    365 * y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400)
}

/// 岁首年 `y` 的长度（365 / 366 天）。
fn march_year_length(y: i64) -> i64 {
    march_year_start(y + 1) - march_year_start(y)
}

/// 从 epoch 天数（1970-01-01 = 0）反推 (年, 月, 日)，纯整数运算，对负数（1970 年之前）同样成立。
///
/// 思路是 Howard Hinnant `civil_from_days` 的那套「以 3 月为岁首」的分解：先按 `400 * 天数 / 146097`
/// 线性估计岁首年，再用 [`march_year_start`] 正向校正（估计值只会差一年左右，循环兜底保证无论差几天
/// 都收敛到唯一正确的年/年内天序），最后用 `(5 * doy + 2) / 153` 这段无分支的月份分段反推月与日。
///
/// 验证：`-361_566..=365_379`（即 980-01-25 到 2970-05-17，含 1970 年之前那段负数）逐日与
/// 「从 1970-01-01 起一天一天数日历」的朴素实现比对，年、月、日全部一致；再对 Hinnant 正向的
/// `days_from_civil` 做往返校验，同样零失配；1970-01-01、2000-02-29、2000-03-01、2100-03-01、
/// 2400-02-29 这些闰世纪边界点另用已知的 epoch 天数单独核对过。
pub(super) fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let shifted = days + 719_468; // 距离 0000-03-01 的天数
    let mut year = (400 * shifted).div_euclid(146_097);
    let mut day_of_year = shifted - march_year_start(year);
    while day_of_year < 0 {
        year -= 1;
        day_of_year += march_year_length(year);
    }
    while day_of_year >= march_year_length(year) {
        day_of_year -= march_year_length(year);
        year += 1;
    }
    // `day_of_year` 现在是岁首年内的第几天，`[0, 365]`。
    let month_prime = (5 * day_of_year + 2) / 153; // [0, 11]
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u8; // [1, 31]
    let month = if month_prime < 10 { month_prime + 3 } else { month_prime - 9 } as u8; // [1, 12]
    if month <= 2 {
        year += 1; // 岁首年比公历年小 1（1、2 月属于上一个岁首年）
    }
    (year, month, day)
}

/// Java `HexFormat.of().formatHex(byte[])`：小写十六进制。
fn hex_lower(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        text.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    text
}

/// FIPS 180-4 的 SHA-256 状态。之所以自带：mm-io 的依赖面是冻结的（见模块文档第 1 条）。
/// `update` 可以任意次数、任意长度地调用，与 `MessageDigest.update(byte[], int, int)` 等价；
/// `finish` 对应 `MessageDigest.digest()`，同样不改变已喂入的前缀（Java 的 digest() 会 reset，
/// 但两处调用点都紧接着丢弃这个 digest，故无差异）。
struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    block_len: usize,
    total_len: u64,
}

/// SHA-256 轮常量（FIPS 180-4 表 4）。
const SHA_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 初始哈希值（FIPS 180-4 5.3.3）。
const SHA_H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

impl Sha256 {
    fn new() -> Sha256 {
        Sha256 { state: SHA_H0, block: [0u8; 64], block_len: 0, total_len: 0 }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.block_len > 0 {
            let take = (64 - self.block_len).min(data.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&data[..take]);
            self.block_len += take;
            data = &data[take..];
            if self.block_len == 64 {
                let block = self.block;
                compress(&mut self.state, &block);
                self.block_len = 0;
            }
        }
        while data.len() >= 64 {
            compress(&mut self.state, &data[..64]);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.block[..data.len()].copy_from_slice(data);
            self.block_len = data.len();
        }
    }

    fn finish(mut self) -> [u8; 32] {
        // FIPS 180-4 5.1.1：追加 0x80、补零到位长度 ≡ 56 (mod 64)、再补 8 字节大端比特长度。
        let bit_length = self.total_len.wrapping_mul(8).to_be_bytes();
        let mut tail = [0u8; 128];
        let start = self.block_len;
        tail[..start].copy_from_slice(&self.block[..start]);
        let mut length = start;
        tail[length] = 0x80;
        length += 1;
        while length % 64 != 56 {
            tail[length] = 0;
            length += 1;
        }
        tail[length..length + 8].copy_from_slice(&bit_length);
        length += 8;
        let mut offset = 0;
        while offset < length {
            compress(&mut self.state, &tail[offset..offset + 64]);
            offset += 64;
        }
        let mut out = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

/// 单个 64 字节块的压缩函数（FIPS 180-4 6.2.2）。
fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for index in 0..16 {
        w[index] = u32::from_be_bytes([
            block[index * 4],
            block[index * 4 + 1],
            block[index * 4 + 2],
            block[index * 4 + 3],
        ]);
    }
    for index in 16..64 {
        let s0 = w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
        let s1 = w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
        w[index] = w[index - 16]
            .wrapping_add(s0)
            .wrapping_add(w[index - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..64 {
        let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(big_s1)
            .wrapping_add(ch)
            .wrapping_add(SHA_K[index])
            .wrapping_add(w[index]);
        let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = big_s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    let adds = [a, b, c, d, e, f, g, h];
    for (word, add) in state.iter_mut().zip(adds) {
        *word = word.wrapping_add(add);
    }
}
