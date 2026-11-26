//! `ModMakerCli.java`（`dev.modmaker.cli.ModMakerCli`，384 行）的等价移植：headless 入口。
//!
//! 包到 crate 的映射照迁移方案 §P4：Java 的 `dev.modmaker.cli` → `mm-cli`，`dev.modmaker.core.io`
//! 的 `BuildPipeline` / `ModExporter` / `BuildManifest` / `ProjectIo` → `mm_io::export`（方案 §P2
//! 已把它们归到那里），`SchemaRegistry` → `mm_schema`，`Issue` / `Validator` → `mm_validate`。
//! 类名、方法名、常量名、错误文案与退出码逐项对齐；下面的差异清单是本文件与 Java 之间**全部**的
//! 不同，逐条编号，别处不再重复说明。
//!
//! # 与 Java 的差别
//!
//! 1. **参数解析交给 `clap`**（用户 2026-11-13 授权的唯一第三方例外，见方案 §P4）。Java 的
//!    `parse`（第 274-314 行）是手写 `for` 循环 + `switch`，`value`（第 322-327 行）自己判缺值；
//!    Rust 这里只用 `clap` 认出「一个位置参数 + 六个长选项」，其余判定——长度不足、`--out` /
//!    `--manifest` 只属于 build、`--json` 只属于 validate、`--profile` 不能全空白、`--help` 抛
//!    `see --help`——仍然逐行照抄 Java，所以**选项与退出码的对应关系不变**。四点可观察的出入：
//!    - `--out=x` 这种等号写法 `clap` 接受，Java 会当成 unknown option；
//!    - 多值选项后面紧跟 `--strict` 时，Java 由 `value`（第 323 行 `startsWith("--")`）报
//!      `--out needs a value`，`clap` 报「a value is required … but none was supplied」；
//!    - 一行里同时有多个非法选项时，报错的是本文件固定的检查顺序（`--out` → `--manifest` →
//!      `--json`），Java 报的是**先出现**的那一个；
//!    - 由 `clap` 生成的用法文案（unknown option、缺值、多余位置参数）措辞与 Java 的手写短句
//!      不同，本文件只剥掉它开头的 `error: ` 前缀，`modmaker: ` 前缀与退出码 `5` 保持一致。
//! 2. **`Issue.Level` 的名字**。Java 第 218 行 `"" + issue.level()` 与第 232 行
//!    `issue.level().name()` 都取枚举常量名，而 Java 侧的常量本身就是小写 `error` / `warning`
//!    （`Issue.java`），Rust 的变体按约定是大写开头的 [`Level::Error`] / [`Level::Warning`]，
//!    故由 [`level_name`] 手工映射回那两个小写串，输出与 Java 逐字符一致。
//! 3. **`args` 的类型载体**。Java 的 `Issue.args()` 是 `List<Object>`，第 242 行 `argument(argument)`
//!    据此把 `Number` 写成 JSON 数字、`Boolean` 写成布尔（第 255-266 行）。Rust 侧曾经是
//!    `Vec<String>`（P3 登记的表达差异），那会让 `--json` 报告把 `[20,16]` 退化成 `["20","16"]`，
//!    实测与 Java 差 4 字节；现已由 `mm_validate::Arg` 承载同一取值域，[`argument_json`] 四分支
//!    与 Java 逐支对应，**本条不再是差异**，保留编号只为让下面的可达性登记有个落脚处：
//!    `Number` 分支**可达**（`ModMakerCli.java` 之外共 9 个校验站点传 `int`/`long`/`Integer`，
//!    逐一点名见 [`argument_json`] 的文档注释），`null` 与 `Boolean` 两支在 Java 侧**不可达**。
//!    Java 的测试只断言 `args` 是数组（`ModMakerCliTest.java:77`），不受影响。
//! 4. **`project.root` 的读法**。Java 第 107、165、172、180 行直接读 `project.root`（为 null 就
//!    NPE）；Rust 的 `ModProject::root` 是 `Option<PathBuf>`，这里改读 [`Plan::root`]——它正是
//!    `pipeline.plan` 内部 `ModExporter::require_root` 原样交出的那个 `project.root` 副本
//!    （`crates/mm-io/src/export/mod_exporter.rs:462-470`），二者同值，且把「为 null」这种
//!    状态从类型上排除掉了。
//! 5. **流与退出**。`System.out` / `System.err` → [`std::io::stdout`] / [`std::io::stderr`]；
//!    `System.exit` 在 `src/main.rs`；Java 的 checked `UsageException`（第 347-351 行）换成
//!    [`UsageError`] + `Result`，由 [`dispatch`] 收敛（对应 Java 第 85-88 行的 catch）。
//! 6. **异常消息文本**。Java 第 100-101、126-127 行把异常交给 `projectFailure`（第 198-201 行，
//!    里面取 `failure.getMessage()`），Rust 用 thiserror 的 `Display`（`SchemaError` /
//!    `ImportError`），句子措辞与 Java 的异常文本不同，但 `modmaker: cannot open the project: `
//!    这个前缀与退出码 `5` 完全一致。
//! 7. **复用 mm-io 的路径 helper**。`absolute_normalized` / `absolute_unnormalized` /
//!    `path_starts_with` / `path_eq` 原本是 `pub(super)`，为本案提升到 `pub`（见
//!    `crates/mm-io/src/export/mod_exporter.rs:700-782`），这样 Java 的 `toAbsolutePath()`、
//!    `toAbsolutePath().normalize()`、Windows 下大小写不敏感的 `startsWith` / `equals` 在 Rust
//!    侧就是同一份实现，不存在第二套判定。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Parser;
use mm_io::export::build_manifest;
use mm_io::export::build_pipeline::BuildPipeline;
use mm_io::export::mod_exporter::{
    absolute_normalized, absolute_unnormalized, path_eq, path_starts_with, require_internal_name,
};
use mm_json::{write_compact, Json, Object};
use mm_schema::SchemaRegistry;
use mm_validate::{Arg, Issue, Level};

/// Java `ModMakerCli.OK`（第 32 行）。
pub const OK: i32 = 0;
/// Java `ModMakerCli.VALIDATION_FAILED`（第 34 行）。
pub const VALIDATION_FAILED: i32 = 2;
/// Java `ModMakerCli.BUILD_FAILED`（第 36 行）。
pub const BUILD_FAILED: i32 = 3;
/// Java `ModMakerCli.USAGE`（第 38 行）。
pub const USAGE: i32 = 5;

/// Java `ModMakerCli.DEFAULT_PROJECT`（第 40 行），只出现在帮助文本里。
const DEFAULT_PROJECT: &str = "<project>";
/// Java `ModMakerCli.DEFAULT_OUT`（第 41 行），只出现在帮助文本里。
const DEFAULT_OUT: &str = "<dir|zip>";

/// Java 第 43-44 行那个「`final class` + 私有构造器」在 Rust 里不需要对应物：本模块全是
/// 自由函数，本来就实例化不出东西。`main`（第 46-48 行）见 `src/main.rs`。
///
/// Java `handles(String[])`（第 51-59 行）：第一个参数是不是本 CLI 认得的子命令。
/// `DesktopLauncher` 用它决定要不要接管参数，所以它必须不碰文件系统。
pub fn handles(args: &[String]) -> bool {
    matches!(
        command_of(args),
        Some("validate") | Some("build") | Some("--help") | Some("-h") | Some("help") | Some(
            "--version"
        ) | Some("-V") | Some("version")
    )
}

/// Java 第 56 行 `switch (args[0])` 的那个被比较的值。`handles` 只需要「在不在名单里」，
/// 所以这里只做取首元素，空参数返回 [`None`]（Java 那边是 `args.length == 0` 提前 return）。
fn command_of(args: &[String]) -> Option<&str> {
    args.first().map(String::as_str)
}

/// Java `run(String[])`（第 62-89 行）：分发到子命令，并把 `UsageException` 收敛成退出码。
///
/// Java 的 `switch (args[0])` 有五个分支，其中 `default` 会连帮助一起打到 `System.err`；
/// 那个 `catch (UsageException problem)` 罩住整条分发链（第 85-88 行），这里是 [`dispatch`]。
pub fn run(args: &[String]) -> i32 {
    if args.is_empty() {
        print_help(std::io::stdout());
        return USAGE;
    }
    match command_of(args) {
        Some("--help") | Some("-h") | Some("help") => {
            print_help(std::io::stdout());
            OK
        }
        Some("--version") | Some("-V") | Some("version") => {
            println!("{} {}", build_manifest::TOOL, build_manifest::TOOL_VERSION);
            OK
        }
        Some("validate") => dispatch(validate(args)),
        Some("build") => dispatch(build(args)),
        Some(unknown) => {
            eprintln!("modmaker: unknown command '{unknown}'");
            print_help(std::io::stderr());
            USAGE
        }
        // `args.is_empty()` 已在上面挡掉，这里只是让 `match` 覆盖 [`Option`]。
        None => USAGE,
    }
}

/// Java 第 85-88 行的 `catch (UsageException problem)`：把消息打到 `System.err` 并返回 `5`。
fn dispatch(result: Result<i32, UsageError>) -> i32 {
    match result {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("modmaker: {}", failure.message);
            USAGE
        }
    }
}

/// Java `validate(String[])`（第 93-114 行）。
fn validate(args: &[String]) -> Result<i32, UsageError> {
    let options = parse(args, false)?;
    // 第 97-102 行：`loadDefault()` 与 `open()` 共用同一个 catch，失败一律走 `projectFailure`。
    let registry = match SchemaRegistry::load_default() {
        Ok(registry) => registry,
        Err(failure) => return Ok(project_failure(&failure.to_string())),
    };
    let pipeline = BuildPipeline::new(&registry);
    let project = match pipeline.open(&options.project, options.profile.as_deref()) {
        Ok(project) => project,
        Err(failure) => return Ok(project_failure(&failure.to_string())),
    };
    let issues = pipeline.validate(&project);
    print_issues(&issues);
    // 第 105-112 行：报告写不下去是**用法**错误（`USAGE`），不是校验失败。
    if let Some(target) = &options.json
        && let Err(failure) =
            write_report(target, project.root.as_deref(), &project.schema_profile, &issues)
    {
        eprintln!("modmaker: cannot write the report: {failure}");
        return Ok(USAGE);
    }
    Ok(exit_code(&issues, options.strict))
}

/// Java `build(String[])`（第 116-196 行）。
fn build(args: &[String]) -> Result<i32, UsageError> {
    let options = parse(args, true)?;
    // 第 118-120 行。
    let Some(out) = options.out.clone() else {
        return Err(UsageError::new("build needs --out <dir|zip>"));
    };
    let registry = match SchemaRegistry::load_default() {
        Ok(registry) => registry,
        Err(failure) => return Ok(project_failure(&failure.to_string())),
    };
    let pipeline = BuildPipeline::new(&registry);
    let mut project = match pipeline.open(&options.project, options.profile.as_deref()) {
        Ok(project) => project,
        Err(failure) => return Ok(project_failure(&failure.to_string())),
    };
    // 第 129-136 行：内部名不合法是**用法**错误。
    if let Err(failure) = require_internal_name(&project) {
        eprintln!("modmaker: {failure}");
        return Ok(USAGE);
    }
    // 第 137-145 行：先校验、再按校验结果决定退不退。
    let plan = match pipeline.plan(&mut project) {
        Ok(plan) => plan,
        Err(failure) => {
            eprintln!("modmaker: cannot save the project: {failure}");
            return Ok(BUILD_FAILED);
        }
    };
    let issues = plan.issues.clone();
    print_issues(&issues);
    let validation_exit = exit_code(&issues, options.strict);
    if validation_exit != OK {
        return Ok(validation_exit);
    }
    let internal = plan.internal_name.clone();
    // 第 154-155 行：目标是不是 zip，只看文件名后缀（大小写不敏感，`Locale.ROOT`）。
    let zip = out
        .file_name()
        .is_some_and(|name| name.to_string_lossy().to_lowercase().ends_with(".zip"));
    let manifest_path =
        options.manifest.clone().unwrap_or_else(|| default_manifest_path(&out));
    let build_target = if zip {
        absolute_normalized(&out)
    } else {
        absolute_normalized(&absolute_normalized(&out).join(&internal))
    };
    let project_root = absolute_normalized(&plan.root);
    // 第 160-169 行：产物落在源目录里面会每构建一次多套一层，先挡住。
    if path_starts_with(&build_target, &project_root) && !path_eq(&build_target, &project_root) {
        eprintln!(
            "modmaker: build target is inside the project folder: {}",
            build_target.display()
        );
        return Ok(BUILD_FAILED);
    }
    if let Err(failure) =
        require_manifest_outside_payload(&plan.root, &out, &internal, zip, &manifest_path)
    {
        eprintln!("modmaker: {failure}");
        return Ok(USAGE);
    }
    // 第 178-195 行那段 `try`：产物、源哈希、产物哈希、清单、三行输出，全成或全败。
    let built = (|| -> io::Result<()> {
        let report = pipeline.write(&mut project, &plan, &out, zip)?;
        let source_hash = build_manifest::source_hash(&plan.root)?;
        let artifact_hash = if zip {
            build_manifest::sha256(&out)?
        } else {
            build_manifest::tree_hash(&out.join(&internal))?
        };
        let manifest = build_manifest::create(
            &project,
            &project.schema_profile,
            report.files.len() as i32,
            report.bytes,
            &source_hash,
            &artifact_hash,
        );
        build_manifest::write(&manifest, &manifest_path)?;
        println!("built {} file(s), {} bytes -> {}", report.files.len(), report.bytes, report.target);
        println!("manifest: {}", absolute_unnormalized(&manifest_path).display());
        println!("sha256: {artifact_hash}");
        Ok(())
    })();
    if let Err(failure) = built {
        eprintln!("modmaker: build failed: {failure}");
        return Ok(BUILD_FAILED);
    }
    Ok(OK)
}

/// Java `projectFailure(Exception)`（第 198-201 行）：开不了项目就是用法错误。
fn project_failure(message: &str) -> i32 {
    eprintln!("modmaker: cannot open the project: {message}");
    USAGE
}

/// Java `exitCode(List<Issue>, boolean)`（第 203-210 行）。
fn exit_code(issues: &[Issue], strict: bool) -> i32 {
    let errors = issues.iter().filter(|issue| issue.is_error()).count();
    if errors > 0 {
        return VALIDATION_FAILED;
    }
    // 第 205 行的口径是 `!issue.isError()`，与 `isWarning()` 等价（`Issue` 只有两个级别）。
    let warnings = issues.iter().filter(|issue| !issue.is_error()).count();
    if strict && warnings > 0 {
        VALIDATION_FAILED
    } else {
        OK
    }
}

/// Java `printIssues(List<Issue>)`（第 212-221 行）。
fn print_issues(issues: &[Issue]) {
    let errors = issues.iter().filter(|issue| issue.is_error()).count();
    println!("validation: {errors} error(s), {} warning(s)", issues.len() - errors);
    for issue in issues {
        let id = match issue.content_id() {
            Some(content_id) => format!(" [{content_id}]"),
            None => String::new(),
        };
        println!(
            "  {} {}{}: {}",
            level_name(issue.level()),
            issue.key(),
            id,
            issue.message()
        );
    }
}

/// Java `writeReport(Path, Path, String, List<Issue>)`（第 223-253 行）。
///
/// 第 248-252 行那句落盘：`toAbsolutePath()` **不** normalize，先 `createDirectories(parent)`，
/// 再 `Files.writeString(..., report.toString() + "\n", UTF_8)`。Gson 的 `toString()` 就是紧凑形
/// 式，对应 [`write_compact`]。
fn write_report(
    target: &Path,
    project_root: Option<&Path>,
    profile: &str,
    issues: &[Issue],
) -> io::Result<()> {
    let mut report = Object::new();
    // 第 226 行 `addProperty("schemaVersion", 1)` 是 JSON **数字** 1，不是字符串。
    report.insert("schemaVersion", Json::num("1"));
    let project = match project_root {
        Some(root) => absolute_unnormalized(root).display().to_string(),
        None => String::new(),
    };
    report.insert("project", Json::str(project));
    report.insert("profile", Json::str(profile));
    let mut entries: Vec<Json> = Vec::new();
    for issue in issues {
        let mut entry = Object::new();
        // 第 232 行取 `level().name()`，即小写的 `error` / `warning`（差异清单第 2 条）。
        entry.insert("severity", Json::str(level_name(issue.level())));
        entry.insert("code", Json::str(issue.key()));
        // 第 234-238 行：没有内容 id 时写 JSON null，而不是省掉这个键。
        let content_id = match issue.content_id() {
            Some(content_id) => Json::str(content_id),
            None => Json::Null,
        };
        entry.insert("contentId", content_id);
        entry.insert("message", Json::str(issue.message()));
        let mut arguments: Vec<Json> = Vec::new();
        for argument in issue.args() {
            arguments.push(argument_json(argument));
        }
        entry.insert("args", Json::Arr(arguments));
        entries.push(Json::Obj(entry));
    }
    report.insert("issues", Json::Arr(entries));
    // 第 248-251 行。
    let absolute = absolute_unnormalized(target);
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&absolute, format!("{}\n", write_compact(&Json::Obj(report))))
}

/// Java `argument(Object)`（第 255-266 行）的四分支，逐支对应 [`Arg`] 的四个变体。
///
/// - 第 256-258 行 `value == null` → `JsonNull.INSTANCE` ⇒ [`Json::Null`]。Java 侧当前没有站点
///   显式传 `null`，这一支与下面的 `Bool` 一样**暂时不可达**，但分支形状必须与 Java 一致。
/// - 第 259-261 行 `value instanceof Number number` → `new JsonPrimitive(number)`，Gson 写的是
///   `number.toString()`（`Integer`/`Long` 都是十进制字面量）⇒ [`Json::num`] 交
///   `i64::to_string()`，与 Java 的 `String.valueOf(int/long)` 逐字符相同。这一支**可达**：
///   `RecordCheck.java:142`、`TeamCheck.java:57`、`PlanetCheck.java:77/96`、
///   `DrillTierCheck.java:58/73`、`BulletReachCheck.java:34/38`、`MinGameVersionCheck.java:66`、
///   `RecipeCapacityCheck.java:47` 共 9 个站点交的是 `int` / `long` / `Integer`。
/// - 第 262-264 行 `value instanceof Boolean bool` → `new JsonPrimitive(bool)` ⇒ [`Json::Bool`]。
///   Java 侧无站点传 `Boolean`，故不可达。
/// - 第 265 行 `new JsonPrimitive(String.valueOf(value))` ⇒ [`Json::str`]，文本已经在
///   [`Arg::Text`] 里，与 Java 的 `String.valueOf(String)` 同值（枚举、`List` 等兜底类型在
///   `Arg` 收表时就由 `From` 转成了 `Text`）。
fn argument_json(value: &Arg) -> Json {
    match value {
        Arg::Null => Json::Null,
        Arg::Int(number) => Json::num(number.to_string()),
        Arg::Bool(flag) => Json::Bool(*flag),
        Arg::Text(text) => Json::str(text.clone()),
    }
}

/// Java 的 `Issue.Level` 常量名是小写 `error` / `warning`，第 218 行（`"" + level`）与
/// 第 232 行（`level.name()`）取的都是它；Rust 变体名不同，故在这里映射（模块注释第 2 条）。
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
    }
}

/// Java `Options`（第 270-272 行的 `private record Options(Path, Path, Path, Path, boolean, String)`），
/// 六个成员按同一顺序搬过来。`null` 成员换成 [`Option`]。
struct Options {
    project: PathBuf,
    out: Option<PathBuf>,
    manifest: Option<PathBuf>,
    json: Option<PathBuf>,
    strict: bool,
    profile: Option<String>,
}

/// `clap` 认 token 的那张表：一个位置参数 + 五个业务长选项 + 一个隐身的 `--help`。
///
/// 五个业务选项在**两个**子命令上是同一张表，这正是 Java 第 284-312 行那个 `for` + `switch`
/// 的形态：认得哪些选项与「当前是不是 build」是两件事，后者由 [`parse`] 自己判（差异清单第 1 条）。
#[derive(Parser)]
#[command(name = "modmaker", disable_help_flag = true, disable_version_flag = true)]
struct CliArgs {
    /// Java 第 278 行的 `Path.of(args[1])`。
    project: PathBuf,
    /// Java 第 287 行。
    #[arg(long)]
    strict: bool,
    /// Java 第 288-291 行。
    #[arg(long)]
    out: Option<PathBuf>,
    /// Java 第 292-295 行。
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Java 第 296-301 行。
    #[arg(long)]
    json: Option<PathBuf>,
    /// Java 第 302-308 行。
    #[arg(long)]
    profile: Option<String>,
    /// Java 第 309 行：命令内的 `--help` / `-h` 不是帮助，是一句 `see --help` 的用法错误，
    /// 所以把 `clap` 自己的帮助关掉，这里收进来交给 [`parse`] 判。
    #[arg(long = "help", short = 'h', hide = true)]
    help_requested: bool,
}

/// Java `parse(String[], boolean)`（第 274-314 行）。
fn parse(args: &[String], building: bool) -> Result<Options, UsageError> {
    if args.len() < 2 {
        // 第 275-277 行。
        let message = if building {
            "build needs <project> and --out"
        } else {
            "validate needs <project>"
        };
        return Err(UsageError::new(message));
    }
    // Java 第 278 行从 `args[1]` 起手写循环，`args[0]` 是子命令本身；`clap` 却把迭代器的
    // **首元素当作 argv[0]（程序名）**吃掉，所以这里必须在 `args[1..]` 前面补一个占位 token，
    // 否则工程位置参数会被当成程序名丢掉。占位串与 `#[command(name = ...)]` 同值，只影响
    // `clap` 自己生成的用法文案，而那份文案本案只保留剥掉 `error: ` 前缀的正文。
    let parsed = CliArgs::try_parse_from(
        std::iter::once("modmaker").chain(args[1..].iter().map(String::as_str)),
    )
    .map_err(|failure| UsageError::new(clap_message(&failure)))?;
    if parsed.help_requested {
        // 第 309 行。
        return Err(UsageError::new("see --help"));
    }
    // 第 288-295 行：`--out` 与 `--manifest` 只属于 build。
    if parsed.out.is_some() {
        require_building(building, "--out")?;
    }
    if parsed.manifest.is_some() {
        require_building(building, "--manifest")?;
    }
    if building && parsed.json.is_some() {
        // 第 296-299 行：反过来，`--json` 只属于 validate。
        return Err(UsageError::new("--json is only supported by validate"));
    }
    let mut profile = None;
    if let Some(version) = &parsed.profile {
        // 第 303-307 行：`isBlank()` 等于没写，非空则 `trim()` 后再存。
        if version.trim().is_empty() {
            return Err(UsageError::new("--profile needs a version"));
        }
        profile = Some(version.trim().to_string());
    }
    Ok(Options {
        project: parsed.project,
        out: parsed.out,
        manifest: parsed.manifest,
        json: parsed.json,
        strict: parsed.strict,
        profile,
    })
}

/// Java `requireBuilding(boolean, String)`（第 316-320 行）。
fn require_building(building: bool, option: &str) -> Result<(), UsageError> {
    if building {
        Ok(())
    } else {
        Err(UsageError::new(format!("{option} is only supported by build")))
    }
}

/// 把 `clap` 的用法错误收成一行，接上调用方的 `modmaker: ` 前缀后读起来与 Java 同构。
/// 措辞本身由 `clap` 决定，见模块注释差异清单第 1 条。
fn clap_message(failure: &clap::Error) -> String {
    let text = failure.to_string();
    let trimmed = text.trim();
    trimmed.strip_prefix("error: ").unwrap_or(trimmed).to_string()
}

/// Java `printHelp(PrintStream)`（第 329-344 行）：12 行文案，逐字符照抄。
///
/// 第 330 行 Java 写的是字面量 `"Mindustry ModMaker"`（不是 `BuildManifest.TOOL`），这里照抄
/// 字面量；两者当前同值，但只有 Java 用哪个就用哪个。
fn print_help<W: Write>(mut out: W) {
    for line in [
        format!("Mindustry ModMaker {} - headless tools", build_manifest::TOOL_VERSION),
        String::new(),
        "Usage:".to_string(),
        format!(
            "  modmaker validate {DEFAULT_PROJECT} [--json <report>] [--strict] [--profile <ver>]"
        ),
        format!(
            "  modmaker build {DEFAULT_PROJECT} --out {DEFAULT_OUT} [--manifest <file>] [--strict] [--profile <ver>]"
        ),
        "  modmaker --help | --version".to_string(),
        String::new(),
        "Exit codes:".to_string(),
        "  0  success".to_string(),
        "  2  validation failed (errors, or warnings with --strict)".to_string(),
        "  3  build failed".to_string(),
        "  5  bad arguments or unreadable project".to_string(),
    ] {
        // Java 用 `PrintStream.println`，它吞掉写失败；这里同样不向上抛。
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
}

/// Java 第 85-88 行 catch 的那个 `private static final class UsageException extends Exception`
/// （第 347-351 行）：Rust 用 [`Result`] 传递，消息字段与 Java 的 `getMessage()` 对应。
struct UsageError {
    message: String,
}

impl UsageError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

/// Java `requireManifestOutsidePayload(...)`（第 358-375 行，包级可见）。
///
/// 它存在的理由写在第 353-357 行的注释里：清单一旦落进 mod 目录就变成 payload，
/// 「项目内容 = 构建内容」这句话当场作废，而导入器还会把它当成 mod 里的一个假内容。
fn require_manifest_outside_payload(
    project_root: &Path,
    out: &Path,
    internal: &str,
    zip: bool,
    manifest: &Path,
) -> io::Result<()> {
    let target = absolute_normalized(manifest);
    let root = absolute_normalized(project_root);
    // 第 363 行的消息体是 `"..." + target`，即 `Path.toString()`；Rust 对应 `display()`，
    // 绝不能用 `{:?}`（那会多出引号并把 `\` 转义成 `\\`）。
    if path_starts_with(&target, &root) {
        return Err(invalid(format!(
            "the manifest must be outside the project source tree: {}",
            target.display()
        )));
    }
    if zip {
        // 第 365-369 行：zip 是单个文件，清单压在它身上就等于把它删掉。
        if path_eq(&target, &absolute_normalized(out)) {
            return Err(invalid(format!(
                "the manifest path must differ from the zip output: {}",
                target.display()
            )));
        }
        return Ok(());
    }
    // 第 371-374 行。
    let mod_dir = absolute_normalized(&out.join(internal));
    if path_starts_with(&target, &mod_dir) {
        return Err(invalid(format!(
            "the manifest must be outside the built mod folder: {}",
            target.display()
        )));
    }
    Ok(())
}

/// 造一个只带消息的 [`io::Error`]，对应 Java 第 363、367、373 行那种 `new IOException(String)`：
/// Rust 侧 `InvalidInput` 只是为了有个 kind，消息文本才是打印出来的东西。
fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Java `defaultManifestPath(Path)`（第 378-383 行，`public static`）。
pub fn default_manifest_path(out: &Path) -> PathBuf {
    let name = match out.file_name() {
        Some(file_name) => file_name.to_string_lossy().to_string(),
        None => String::new(),
    };
    // 第 380 行同样按 `Locale.ROOT` 小写比较：`.ZIP` 与 `.zip` 走同一条路。
    if name.to_lowercase().ends_with(".zip") {
        PathBuf::from(format!("{}.manifest.json", out.display()))
    } else {
        out.join("manifest.json")
    }
}
