//! `GameLauncher` 的等价移植（Java `dev.modmaker.ui.GameLauncher`，实测 **298 行**）。
//!
//! 对应 Java 类注释（`GameLauncher.java:11-21`）：这一类管的是**验证闭环**——把模组构建进一个
//! 一次性数据目录，再用 `MINDUSTRY_DATA_DIR` 起游戏（`:14-16` 点名 `ClientLauncher.java:42`
//! 认这个变量），于是「测试跑一遍」永远碰不到玩家真实的 profile，而游戏自己写下的
//! `last_log.txt` 能读回来报告 content error（`:17-21`）。
//!
//! ## 与 Java 的结构性差异（逐条登记，均在对应落点再注一次）
//!
//! 1. **`WATCHERS` 线程池（`:31-41`）→ 具名线程 + 停止标志**。Java 用
//!    `Executors.newCachedThreadPool` 造 daemon 线程池，为的是「线程名字一致、不会吊住 JVM、
//!    窗口关闭时 `shutdownWatchers()` 能停掉它们」。Rust 的 `std::thread` **没有 daemon 概念**：
//!    线程会一直等到它自己结束。这里落地的是名字（`std::thread::Builder::name`，逐字照抄
//!    `:38` 的 `"modmaker-game-watcher"`）与 [`shutdown_watchers`]（对应 `:44-46` 的
//!    `shutdownNow()`）——一个进程级标志，读循环每处理完一行检查一次，等价于 Java 那个
//!    interrupt 在「下一次回到 `readLine()` 边界」时才生效的实际效果；**它无法打断一个正阻塞在
//!    管道读上的线程**，正如 Java 的 interrupt 也只能在同一个边界上生效。真正的「无论如何都要收摊」
//!    由宿主进程退出完成（Rust 侧 `std::process::exit` 与 Java 侧 daemon 线程随 JVM 消失等价）。
//! 2. **`redirectErrorStream(true)`（`:120`）→ 两条管道、两个 watcher 线程**。std 的
//!    `Stdio` 在稳定 API 里没法把子进程的 stderr 复制进 stdout 那根管道
//!    （`DuplicatedOutputHandle` 至今未稳定），所以这里两条流各起一个同名线程读。两条流的每一行
//!    都会进 `console.log`、都会被 [`is_problem_line`] 扫到，语义覆盖一致；**不保证两线程写入
//!    顺序与 Java 单流逐行顺序完全相同**，`console.log` 由一个 `Arc<Mutex<..>>` 串行写。
//!    收尾（等退出码 ＋ 读 `last_log.txt`，`:141-156`）只由 stdout 那一线做，stderr 那线纯转发。
//! 3. **`Process` → [`ProcessHandle`] trait**。Java 的记录组件是 `java.lang.Process`，测试
//!    （`GameLauncherTest.java:88-113`）靠**继承**它造 `FakeProcess`。Rust 的
//!    [`std::process::Child`] 没有公开构造器，外部无法伪裝，所以这里把用到的四件事
//!    （`isAlive()` `:53`、`exitValue()` `:142`、`waitFor()` `:144`、`destroy()`
//!    AppShell `:1772`）收成一个小 trait，真实实现包住 `Child`，测试实现可以只是一个布尔。
//!    **destroy 的平台差异（2026-11-20 M7a 补移植时登记）**：Java `Process.destroy()` 在
//!    Windows 上是 `TerminateProcess`（强杀），在 Unix 上发 `SIGTERM`（可被捕获）；
//!    Rust `Child::kill()` 在 Windows 上同为 `TerminateProcess`，在 Unix 上发 `SIGKILL`。
//!    本项目目标平台是 Windows（gnu 工具链），该平台上两者逐字等价；Unix 上 Rust 侧更强硬，
//!    方向是「停止按钮必须真的停得下来」，不复制 SIGTERM 的优雅退出语义。
//! 4. **`Consumer<String> log`（`:99`）→ `Arc<dyn Fn(&str) + Send + Sync>`**。Java 的回调
//!    本来就在两个线程间共享（FX 的 `Platform.runLater` 负责回主线程），Rust 要跨线程必须显式
//!    `Arc + Send + Sync`，这是类型层面的强制，不是行为差异。
//! 5. **`Files.createTempDirectory(prefix)`（`:100`）→ 进程号 ＋ 进程内序号**。std 没有安全随机
//!    临时名，这里用 `create_dir` 的「已存在则失败」做独占性保证并递增序号重试；前缀逐字照抄
//!    Java 的 `"modmaker-game-"`。落点同样是系统临时目录（Java 的 `java.io.tmpdir`）。
//! 6. **逐行解码宽松度**。`readProblems`（`:200-216`）在 Java 里特意装了 `REPLACE` 解码器，
//!    Rust 侧统一用 `String::from_utf8_lossy`，与之完全一致；stdout 那条流 Java 用的是默认字符集
//!    的**严格**解码（坏字节会抛 IOException 而丢掉整次转发），Rust 侧沿用 lossy——比 Java 更宽容，
//!    方向上是「宁可多报也不要整段丢掉」。
//! 7. **行分隔符**。Java 的 `PrintWriter.println`（`:132`）写平台行分隔符（Windows 上是 `\r\n`），
//!    Rust 的 `writeln!` 只写 `\n`，所以这里显式按平台写。
//! 8. **`last_log.txt` 到底落在哪儿（M3 实机门实测，证据写在 [`Session::log_file`] 的文档上）**：
//!    160.7 这个 jar 在 `ClientLauncher.setup()` 应用数据目录**之前**就打开了 `last_log.txt`，于是它
//!    落在默认 profile 目录、**不在** `MINDUSTRY_DATA_DIR` 下。本文件对这一点**不做任何语义改动**：
//!    `logFile` 逐字保留 Java `:161` 的 `dataDir.resolve("last_log.txt")`，[`read_problems`] 逐字保留
//!    `:200-216`。「游戏输出里有没有 content error」这条实际取证走 watcher 自己写的
//!    `console.log`（`:119`、`:128`）——那是同一份游戏输出的另一份拷贝，且正是 Java 那套
//!    [`is_problem_line`] 扫描器的用武之地。

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use mm_io::export::build_pipeline::Plan;
use mm_io::files::search_paths;

use crate::project_controller::{BoxError, ProjectController};

/// 对应 `JAVA_EXECUTABLE`（`:24-26`）：平台决定 JDK 里启动器的文件名。
/// Java 判的是 `os.name` 里有没有 `"win"`，Rust 侧 `cfg!(windows)` 是同一件事的编译期形式
/// （差异见 [`is_windows`] 的注释）。
const JAVA_EXECUTABLE: &str = if cfg!(windows) { "java.exe" } else { "java" };

/// 对应 `GAME_JAR`（`:27`）：游戏 jar 的文件名。
const GAME_JAR: &str = "Mindustry.jar";

/// 对应 `JAVA_SCAN_DEPTH`（`:28-29`）：从安装根往下找 `bin/java` 的层数。
const JAVA_SCAN_DEPTH: i32 = 3;

/// 对应 `WATCHERS`（`:31-41`）那套 daemon 线程池的「停止」语义（模块头第 1 点）。
/// 线程名字逐字照抄 `:38`。
const WATCHER_THREAD_NAME: &str = "modmaker-game-watcher";

/// 对应 `shutdownNow()`（`:45`）置的那个中断状态的替代物：进程级标志，读循环每行检查一次。
static WATCHERS_STOPPED: AtomicBool = AtomicBool::new(false);

/// 对应 `shutdownWatchers()`（`:43-46`）：「由应用的关闭钩子调用」。
///
/// Java 那一行是 `WATCHERS.shutdownNow()`。Rust 侧它做不到「立刻掐死正在阻塞读的线程」，
/// 语义是「让每个 watcher 在下一次回到行边界时退出循环」；与 Java 的 interrupt 在同一边界生效。
/// 差异登记见模块头第 1 点。
pub fn shutdown_watchers() {
    WATCHERS_STOPPED.store(true, Ordering::SeqCst); // :45
}

/// 读循环的停止检查点（Java 那边由 `readLine()` 抛 `InterruptedException` 表达）。
fn watchers_stopped() -> bool {
    WATCHERS_STOPPED.load(Ordering::SeqCst)
}

/// 对应 Java 的 `Consumer<String> log`（`:99`）：日志汇，跨线程共享（模块头第 4 点）。
pub type LogSink = Arc<dyn Fn(&str) + Send + Sync>;

/// 对应 Java `Process` 被用到的那四个动作（模块头第 3 点；前三个在本类，
/// `destroy()` 在 AppShell 的 `stopGame`，`:1772`）。
pub trait ProcessHandle: Send + Sync {
    /// `Process.isAlive()`（`:53`、`:141`）。
    fn is_alive(&self) -> bool;
    /// `Process.exitValue()`（`:142`）：Java 在进程还活着时抛
    /// `IllegalThreadStateException`，调用点（`:141`）已先判过 `isAlive()`，
    /// 这里只负责「已经退出时给出退出码」；拿不到码（被信号杀死）时给 `-1`。
    fn exit_value(&self) -> i32;
    /// `Process.waitFor()`（`:144`）：阻塞到进程结束并给出退出码。
    fn wait_exit(&self) -> i32;
    /// `Process.destroy()`（AppShell `:1772`）：尽力终止，已退出的进程上是空操作。
    /// Java 不抛受检异常，这里同样吞掉 IO 错误（平台差异见模块头第 3 点）。
    fn destroy(&self);
}

/// [`ProcessHandle`] 的真实实现：包住 `std::process::Child`。
///
/// `Mutex` 是因为 Java 的 `Process` 天生可以被多个线程同时用（`:121` 交给 watcher、
/// `:160` 交给记录），而 `Child::wait`／`try_wait` 要 `&mut self`。
pub struct GameProcess {
    child: Mutex<Child>,
}

impl GameProcess {
    /// 由 [`build_and_launch`] 用刚 spawn 出来的 `Child` 构造。
    pub fn new(child: Child) -> Self {
        GameProcess { child: Mutex::new(child) }
    }

    /// `try_wait()` 的结果：`Ok(None)` 才是「还活着」。锁拿不到（别的线程 panic 过）或
    /// `try_wait` 报错时按「还活着」处理——Java 那边这两件事根本不会发生，宁可保守：
    /// 宁可拒绝清理临时目录，也不能把游戏正在用的数据目录抽走（`:56-59` 注释的理由）。
    fn polled(&self) -> Option<Option<i32>> {
        let mut guard = self.child.lock().ok()?;
        match guard.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            Ok(None) => Some(None),
            Err(_) => None,
        }
    }
}

impl ProcessHandle for GameProcess {
    fn is_alive(&self) -> bool {
        match self.polled() {
            Some(Some(_)) => false, // 已退出，且有退出码
            Some(None) => true,     // 还在跑
            None => true,           // 取不到状态：保守地当作还在跑
        }
    }

    fn exit_value(&self) -> i32 {
        match self.polled() {
            Some(Some(code)) => code,
            // Some(None) 意味着 Java 会抛 IllegalThreadStateException；调用点已避免这种情形。
            _ => -1,
        }
    }

    fn wait_exit(&self) -> i32 {
        match self.child.lock() {
            Ok(mut guard) => match guard.wait() {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => -1,
            },
            Err(_) => -1,
        }
    }

    fn destroy(&self) {
        if let Ok(mut guard) = self.child.lock() {
            let _ = guard.kill();
        }
    }
}

/// 对应 `record Session(Process process, Path dataDir, Path logFile, Path modFolder)`
/// （`:48-66`）。四个组件按本 crate 既有风格（`project_actions::ImportResult`）直接做成公有字段。
///
/// Java 的 `running()`（`:51-54`）要判 `process != null`；Rust 侧 `process` 是 `Arc`，
/// 类型上不可能为 null，于是那半个条件由类型系统承担。
pub struct Session {
    /// 对应记录组件 `process`（`:49`）。
    pub process: Arc<dyn ProcessHandle>,
    /// 对应记录组件 `dataDir`（`:49`）：这次运行独占的一次性数据目录。
    pub data_dir: PathBuf,
    /// 对应记录组件 `logFile`（`:49`）：游戏自己写的 `last_log.txt`（`:161`）。
    ///
    /// **M3 实机门实测（2026-11-14，Mindustry 160.7）登记一条与 `:14-16` 类注释假设不符的事实**：
    /// `last_log.txt` **不落在 `MINDUSTRY_DATA_DIR` 指的一次性目录里**。游戏自己的堆栈：
    /// `arc.files.Fi.writer` ← `mindustry.Vars.loadFileLogger(Vars.java:470)` ←
    /// `mindustry.desktop.DesktopLauncher.main(DesktopLauncher.java:48)` —— 开日志文件这件事发生在
    /// `ClientLauncher.setup()`（`String dir = System.getProperty("mindustry.data.dir",
    /// OS.env("MINDUSTRY_DATA_DIR"))`，实测自该 class 的字节码）应用数据目录**之前**，路径于是回落到
    /// 默认 profile 目录。同一趟运行里一次性目录确实拿到了 `cache/`、`previews/`、`settings.bin`、
    /// `server_list.json`、`mods/`，所以「跑测试碰不到玩家 profile」这条前提对**内容加载**成立、
    /// 对**日志文件位置**不成立。这里仍逐字保留 Java 的 `dataDir.resolve("last_log.txt")`（`:161`）
    /// ——移植不改语义；真实游戏输出的等价来源是 watcher 自己写的 `console.log`（`:119`、`:128`）。
    pub log_file: PathBuf,
    /// 对应记录组件 `modFolder`（`:49`）：产物所在目录，来自构建报告的 `target`（`:162`）。
    pub mod_folder: PathBuf,
}

impl Session {
    /// 对应 `running()`（`:51-54`）：游戏进程还在不在。
    pub fn running(&self) -> bool {
        self.process.is_alive() // :53
    }

    /// 对应 `close()`（`:56-65`）：删掉一次性数据目录（mods、saves、logs）。
    ///
    /// `:56-58` 的注释写明为什么「游戏还在跑就直接 return」：把数据目录从它底下抽走会把这次
    /// 运行搞坏。删除是尽力而为（见 [`delete_tree`]），Java 那边也忽略一切结果。
    pub fn close(&self) {
        if self.running() {
            return; // :61
        }
        delete_tree(&self.data_dir); // :63
    }
}

/// 私有构造器 `GameLauncher()`（`:67-69`）在 Rust 由「本模块只有自由函数 ＋ 类型」表达。
///
/// 对应 `deleteTree(Path root)`（`:70-91`）：删一棵临时目录树，**绝不跟符号链接走出去**。
///
/// `:70-74` 的注释给出 Java 的三条依据：`Files.walk` 默认不跟链接、删链接删的是链接本身、
/// 所以数据目录里被人放一个 symlink 也够不到用户的真实文件。`:84-87` 的 catch 说明这是
/// 尽力而为——游戏还攥着的文件会留在原地。
///
/// ## 为什么不用 `std::fs::remove_dir_all`（M3 计划要求实测，不许假设）
///
/// **实测记录（2026-11-14 本机，Administrator 账户；一次性探针跑完取数即删，故结论登记在这里）**
/// ——同一棵树里放四个「往外指」的条目，两份同样的树分别交给 `std::fs::remove_dir_all`
/// 与本函数删：
/// - 真正的符号链接**建不出来**：`symlink_dir` 与 `symlink_file` 都是
///   `Os { code: 1314, message: "客户端没有所需的特权。" }`（开发者模式未开），因此 POSIX
///   那条路径本机无法实测，只能靠「本函数用 `entry.file_type()`（不跟链接）判定目录」与
///   Java `:70-74` 的 `Files.walk` 默认不跟链接这两条依据对齐；
/// - 目录联接（`cmd /C mklink /J`）无需特权，建得成（exit=0）；指向树外文件的硬链接
///   （`fs::hard_link`）也建得成；
/// - 删完的实测结果**两版一致**：`data` 整棵消失（`remove_dir_all` 返回 `Ok`、两者的残留
///   集合都为空），树外 `outside/keep.txt` 仍在、内容 `"important"` 未变。也就是说
///   **std 的 `remove_dir_all` 在本机并不跟 junction 出树**——「不跟链接」这一条上两版等价，
///   计划里担心的那个不等价**没有发生**。
///
/// 那为什么仍然不用它：**剩下的差异在错误口径，不在链接**。`remove_dir_all` 是「一步成功或
/// 报错」的原子口径（首个失败即 `Err`，已删的不回滚、后面的条目不再处理），Java 这版是「每个
/// 条目各自尽力、失败就地吞掉」（`:84-87` 的 catch 注释写明游戏还攥着的文件会留在原地）。
/// 被占用的文件、只读文件这类情形下两者的**残留集合**并不相同，而 `Session.close()` 要的正是
/// Java 那种「能删多少删多少」的行为。该差异本探针**未**复现（需要真的占住一个文件），
/// 如实记为待证；实现按 Java 的逐条尽力口径。
pub fn delete_tree(root: &Path) {
    // :77-79  `root == null || !Files.exists(root, NOFOLLOW_LINKS)` -> return
    // Rust 没有 null；`symlink_metadata` 正是 NOFOLLOW 的那一个（指向不存在的目标的链接仍算存在）。
    if fs::symlink_metadata(root).is_err() {
        return;
    }
    delete_children(root);
    // `Files.walk` 的条目**含根自身**（`:81`），reverseOrder 把它排到最后：等价于先清空再删根。
    let _ = fs::remove_dir(root);
}

/// [`delete_tree`] 的递归内核：对应 Java 的 `Files.walk(root)` ＋ `sorted(reverseOrder())`
/// ＋ 逐条 `deleteIfExists`（`:80-88`）。
fn delete_children(dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // :89-90  目录已经没了或读不了：没什么可清理的。
        Err(_) => return,
    };
    for entry in entries {
        let entry = match entry {
            // Java 里 walk 的迭代器抛 IOException 会传到 `:89` 那个 catch 并被吞掉，
            // 也就是**整个清理中止**（后面的条目不再处理），这里保持一致。
            Err(_) => break,
            Ok(entry) => entry,
        };
        let path = entry.path();
        // `DirEntry::file_type` 不跟随链接（POSIX 的 d_type ／ Windows 的查找数据），
        // 正是 `:70-74` 要的语义：链接的 `is_dir()` 是 false，于是绝不递归进目标。
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue, // 连类型都取不到：这一条按「删不掉」处理，与 Java 的 ignored 一致。
        };
        if file_type.is_dir() {
            delete_children(&path);
        }
        let _ = delete_entry(&path, &file_type);
    }
}

/// 删一个条目本身。链接永远只删链接：先按文件删（POSIX 上对指向目录的链接必须这么删），
/// 失败再按目录删（Windows 上的目录链接／junction 要用目录删除才摘得掉链接本身）。
/// 两个调用都不会进入链接的目标。
fn delete_entry(path: &Path, file_type: &fs::FileType) -> std::io::Result<()> {
    if file_type.is_symlink() {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(first) => fs::remove_dir(path).map_err(|_| first),
        }
    } else if file_type.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

/// 对应 `buildAndLaunch(ProjectController, BuildPipeline.Plan, Path gameJar, Path javaExe,
/// Consumer<String> log)`（`:93-166`）：把已保存的计划构建进
/// `scratch/mods/<name>/`（`:102-105`）并让游戏跑在它上面。
///
/// `:95-96` 的注释写明分工：计划是在持有模型的线程上产出的，这个方法**只碰文件系统与进程**。
/// `:163-165` 的 catch 是「任何失败都先把临时目录删掉再抛」，Rust 没有 try/finally，
/// 于是拆成外层函数（负责清理）与内层函数（负责干活）两段。
pub fn build_and_launch(
    controller: &mut ProjectController,
    plan: &Plan,
    game_jar: &Path,
    java_exe: &Path,
    log: LogSink,
) -> Result<Session, BoxError> {
    let data_dir = create_temp_directory("modmaker-game-")?; // :100
    match launch_inside(controller, plan, game_jar, java_exe, &log, &data_dir) {
        Ok(session) => Ok(session),
        Err(failure) => {
            delete_tree(&data_dir); // :164
            Err(failure)
        }
    }
}

/// `buildAndLaunch` 的 `try` 体（`:101-162`）。
fn launch_inside(
    controller: &mut ProjectController,
    plan: &Plan,
    game_jar: &Path,
    java_exe: &Path,
    log: &LogSink,
    data_dir: &Path,
) -> Result<Session, BoxError> {
    let mods_dir = data_dir.join("mods"); // :102
    fs::create_dir_all(&mods_dir)?; // :103

    // :105  controller.writeBuildFolder(plan, modsDir)
    let report = controller.write_build_folder(plan, &mods_dir)?;
    // :106  "installed " + report.files().size() + " files into " + report.target()
    log(&format!(
        "installed {} files into {}",
        report.files.len(),
        report.target
    ));
    log(&format!("game data directory: {}", data_dir.display())); // :107

    // :109-114  先把命令行拼成一个 List<String>，:122 的播报与 :116 的 ProcessBuilder 都用它。
    let command_line: Vec<String> = vec![
        java_exe.to_string_lossy().into_owned(), // :110
        "-Xms512m".to_string(),                  // :111
        "-Xmx2G".to_string(),                    // :112
        "-jar".to_string(),                      // :113
        game_jar.to_string_lossy().into_owned(), // :114
    ];

    // :117  builder.directory(gameJar.toAbsolutePath().getParent().toFile())
    let jar_home = search_paths::normalize_absolute(game_jar);
    let working_dir = jar_home
        .parent()
        .ok_or_else(|| io_error("game jar has no parent directory"))?;

    let mut process_command = Command::new(&command_line[0]);
    for argument in &command_line[1..] {
        process_command.arg(argument);
    }
    process_command.current_dir(working_dir); // :117
    // :118  builder.environment().put("MINDUSTRY_DATA_DIR", dataDir.toAbsolutePath().toString())
    process_command.env(
        "MINDUSTRY_DATA_DIR",
        search_paths::normalize_absolute(data_dir),
    );
    let console_log = data_dir.join("console.log"); // :119
    // :120  redirectErrorStream(true) 的稳定 API 替身：两条管道两个读线程（模块头第 2 点）。
    process_command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = process_command.spawn()?; // :121  builder.start()

    // :122  "launched: " + String.join(" ", command)
    log(&format!("launched: {}", command_line.join(" ")));

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let process: Arc<dyn ProcessHandle> = Arc::new(GameProcess::new(child));

    // :128  openConsoleLog(consoleLog, log)：Java 在 watcher 内部打开，这里因为两条流要共用
    // 同一个写入端，改到 spawn 之前打开（失败同样只播报、不致命，模块头第 2 点）。
    let console = ConsoleLog::open(&console_log, log);

    spawn_watcher(stdout, Arc::clone(&process), &console, data_dir, Arc::clone(log), true)?;
    spawn_watcher(stderr, Arc::clone(&process), &console, data_dir, Arc::clone(log), false)?;

    // :160-162  return new Session(process, dataDir, dataDir.resolve("last_log.txt"),
    //                              Path.of(report.target()))
    Ok(Session {
        process,
        data_dir: data_dir.to_path_buf(),
        log_file: data_dir.join("last_log.txt"),
        mod_folder: PathBuf::from(&report.target),
    })
}

/// 起一个名为 `modmaker-game-watcher` 的线程读一条子进程输出流（`:126-158`）。
///
/// `report_exit` 只给 stdout 那条线：它负责等退出码并回读 `last_log.txt`（模块头第 2 点）。
fn spawn_watcher<R>(
    stream: Option<R>,
    process: Arc<dyn ProcessHandle>,
    console: &ConsoleLog,
    data_dir: &Path,
    log: LogSink,
    report_exit: bool,
) -> Result<(), BoxError>
where
    R: Read + Send + 'static,
{
    let data_dir = data_dir.to_path_buf();
    let console = console.clone();
    std::thread::Builder::new() // :36-41  那套线程工厂的等价物：只保留名字
        .name(WATCHER_THREAD_NAME.to_string())
        .spawn(move || watch(stream, process, console, data_dir, log, report_exit))
        .map_err(|failure| {
            io_error(&format!("could not start the output watcher: {failure}"))
        })?;
    Ok(())
}

/// `WATCHERS.execute(...)` 的 lambda 体（`:126-158`）。
fn watch<R>(
    stream: Option<R>,
    process: Arc<dyn ProcessHandle>,
    console: ConsoleLog,
    data_dir: PathBuf,
    log: LogSink,
    report_exit: bool,
) where
    R: Read,
{
    if let Some(stream) = stream {
        let mut reader = BufReader::new(stream);
        let mut buffer: Vec<u8> = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break, // readLine() 返回 null：EOF
                Ok(_) => {}
                // :157-158  "could not read the game output: " + failure.getMessage()
                Err(failure) => {
                    log(&format!("could not read the game output: {failure}"));
                    break;
                }
            }
            trim_newline(&mut buffer);
            let line = String::from_utf8_lossy(&buffer); // 模块头第 6 点
            console.write_line(&line); // :131-134  writer != null 时 println + flush
            if is_problem_line(&line) {
                log(&format!("[game] {}", line.trim())); // :135-137
            }
            if watchers_stopped() {
                break; // 模块头第 1 点：shutdownNow 的 interrupt 边界
            }
        }
    }

    if !report_exit {
        return;
    }

    // :141-142  int exit = process.isAlive() ? -1 : process.exitValue();
    let mut exit = if process.is_alive() { -1 } else { process.exit_value() };
    // :143-147  try { exit = process.waitFor(); } catch (InterruptedException) { 复位中断标志 }
    // 被 shutdown 打断时 Java 保留上面那个值，这里用同一个停止标志表达。
    if !watchers_stopped() {
        exit = process.wait_exit();
    }
    log(&format!("game exited with code {exit}")); // :148

    // :149-156  dataDir/last_log.txt 存在则读回问题行。
    let game_log = data_dir.join("last_log.txt");
    if game_log.exists() {
        match read_problems(&game_log) {
            Ok(problems) => {
                for problem in problems {
                    log(&problem); // :152  readProblems(gameLog).forEach(log)
                }
            }
            Err(failure) => {
                log(&format!("could not read the game log: {failure}")); // :154
            }
        }
    }
}

/// 去掉行尾的 `\n` 与 `\r`（对应 `BufferedReader.readLine()` 的行为）。
fn trim_newline(buffer: &mut Vec<u8>) {
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }
    }
}

/// 对应 `openConsoleLog(Path consoleLog, Consumer<String> log)`（`:168-182`）。
///
/// `:168-171` 的注释是这段设计的重点：**打不开就返回「没有写入端」，而不是让 watcher 失败**，
/// 因为 stdout 必须继续被抽干——管道一旦写满，游戏就会冻在那里。
/// Java 返回 `null`，Rust 用 [`Option`]；两条流共用所以要 `Mutex`（模块头第 2 点）。
#[derive(Clone)]
struct ConsoleLog {
    writer: Arc<Mutex<Option<File>>>,
}

impl ConsoleLog {
    fn open(console_log: &Path, log: &LogSink) -> Self {
        let opened = match File::create(console_log) {
            Ok(file) => Some(file),
            // :178-180  "could not write the console log, output is not saved: " + message
            Err(failure) => {
                log(&format!(
                    "could not write the console log, output is not saved: {failure}"
                ));
                None
            }
        };
        ConsoleLog { writer: Arc::new(Mutex::new(opened)) }
    }

    /// `writer.println(line); writer.flush();`（`:132-133`）。`PrintWriter` 吞掉一切写入失败，
    /// 这里同样忽略：写不进日志文件也不能停抽 stdout。
    fn write_line(&self, line: &str) {
        let Ok(mut guard) = self.writer.lock() else { return };
        let Some(file) = guard.as_mut() else { return };
        let _ = file.write_all(line.as_bytes());
        let _ = file.write_all(if cfg!(windows) { b"\r\n" } else { b"\n" }); // 模块头第 7 点
        let _ = file.flush();
    }
}

/// 对应 `isProblemLine(String line)`（`:184-192`）：游戏输出里标志内容／加载问题的行。
///
/// 七个判据逐字照抄，包括 `:191` 那条注释特意说明的——游戏自己的崩溃横幅拼错了「occured」。
/// Java 用 `toLowerCase(Locale.ROOT)`，Rust 的 `str::to_lowercase` 同样与区域设置无关。
pub fn is_problem_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("error loading content") // :186
        || lower.contains("sprite not found") // :186
        || lower.contains("failed to load") // :187
        || lower.contains("no parsers for content type") // :187
        || lower.contains("has errored") // :188
        || lower.contains("exception") // :188
        || lower.contains("an error has occured") // :189-191
}

/// 对应 `readProblems(Path logFile)`（`:194-218`）：从已结束的游戏日志里取问题行，
/// 供游戏退出后的那段总结。`:194-195` 的注释说明为什么要**限量**：一个 GB 的错误日志
/// （某个模组在刷警告）不能把堆撑爆。
///
/// 解码口径见 `:200-204` 的注释：日志通常是 UTF-8，但非 UTF-8 控制台上的区域相关输出会混进
/// 别的字节，严格解码会抛异常而丢掉**整段**总结，所以坏字节换成替换字符——Rust 侧的
/// `from_utf8_lossy` 就是这条策略。
///
/// 实机门实测（模块头第 8 点）：160.7 这个 jar 不把 `last_log.txt` 写进 `MINDUSTRY_DATA_DIR`，
/// 于是按 Java `:161` 传进来的这个路径可能压根不存在，本函数就照 Java 那样把 `NotFound`
/// 一路交回调用方（`:196` 的 `Files.exists` 短路在 Java 侧对应的是同一件事），不做兜底改写。
pub fn read_problems(log_file: &Path) -> std::io::Result<Vec<String>> {
    let mut problems: Vec<String> = Vec::new(); // :196
    let mut reader = BufReader::new(File::open(log_file)?); // :205-207
    let mut buffer: Vec<u8> = Vec::new();
    let mut scanned: usize = 0; // :209
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) => break, // :210  readLine() == null
            Ok(_) => {}
            Err(failure) => {
                // :219 的 `throws IOException`：调用方（watcher）负责播报并继续。
                return Err(failure);
            }
        }
        trim_newline(&mut buffer);
        // :210  `scanned++ < 200_000`：最多处理 200_000 行。
        scanned += 1;
        let line = String::from_utf8_lossy(&buffer);
        if is_problem_line(&line) {
            problems.push(format!("[game] {}", line.trim())); // :211-213
        }
        if scanned >= 200_000 {
            break;
        }
    }
    if problems.is_empty() {
        // :215-217  一条都没有也要给一行「没问题」。
        problems.push(format!(
            "[game] no content errors reported in {}",
            log_file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
    }
    Ok(problems)
}

/// 对应 `detectJava()`（`:220-253`）：先 `JAVA_HOME`，再 `PATH`，最后本 OS 的常见安装根。
/// `:221-223` 的注释写明意图：这里不绑任何一台机器的目录布局，所以从任何 checkout 都能用。
pub fn detect_java() -> Option<PathBuf> {
    // :225-231  JAVA_HOME
    if let Some(home) = non_blank_env("JAVA_HOME") {
        let candidate = java_in(Path::new(&home));
        if candidate.is_some() {
            return candidate; // :229
        }
    }
    // :232-242  PATH，按平台分隔符切开，条目先 strip 再判空
    if let Some(path) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&path) {
            let entry = entry.to_string_lossy();
            if !entry.trim().is_empty() {
                let candidate = java_in(Path::new(entry.trim())); // :236
                if candidate.is_some() {
                    return candidate; // :238
                }
            }
        }
    }
    // :243-251  常见安装根，每根往下浅扫 JAVA_SCAN_DEPTH 层
    for root in java_search_roots() {
        if !root.is_dir() {
            continue; // :244-246
        }
        let found = search_paths::find(Some(&root), JAVA_SCAN_DEPTH, &is_java_executable); // :247
        if found.is_some() {
            return found; // :248-250
        }
    }
    None // :252
}

/// 对应 `javaIn(Path home)`（`:255-259`）：`home/bin/java[.exe]` 是个常规文件就给出绝对路径。
fn java_in(home: &Path) -> Option<PathBuf> {
    let executable = home.join("bin").join(JAVA_EXECUTABLE); // :257
    if executable.is_file() {
        Some(search_paths::normalize_absolute(&executable)) // :258  toAbsolutePath()
    } else {
        None
    }
}

/// 对应 `isJavaExecutable(Path path)`（`:261-266`）：文件名是 `java[.exe]`、父目录名是 `bin`、
/// 且本身是常规文件。
fn is_java_executable(path: &Path) -> bool {
    // Java 靠 `Path.getNameCount()` 与 `getFileName()` 的 null 短路；Rust 侧用 match 提前返回，
    // 结果与 `:262-265` 的 `&&` 链逐条一致。
    let name = match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => return false,
    };
    let parent = match path.parent() {
        Some(parent) => parent,
        None => return false,
    };
    let parent_name = match parent.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => return false,
    };
    name == JAVA_EXECUTABLE // :262
        && parent_name == "bin" // :263-264
        && path.is_file() // :265
}

/// 对应 `javaSearchRoots()`（`:268-281`）：本平台常见的 JDK 安装根。
///
/// `user.home`（`:269`）在 Rust 侧是 `USERPROFILE`（Windows）／`HOME`（其它），
/// 取不到时用 `"."`，与 Java 的 `System.getProperty("user.home", ".")` 默认值一致。
fn java_search_roots() -> Vec<PathBuf> {
    let home = user_home();
    if is_windows() {
        // :271-275
        return vec![
            PathBuf::from("C:/Program Files/Java"),
            PathBuf::from("C:/Program Files/Eclipse Adoptium"),
            PathBuf::from("C:/Program Files/Microsoft"),
            PathBuf::from("C:/Program Files/Zulu"),
            home.join(".jdks"),
        ];
    }
    // :277-280
    vec![
        PathBuf::from("/usr/lib/jvm"),
        PathBuf::from("/Library/Java/JavaVirtualMachines"),
        home.join(".sdkman/candidates/java"),
        home.join(".jdks"),
    ]
}

/// 对应 `isWindows()`（`:283-285`）。
///
/// Java 在运行时把 `os.name` 转小写找 `"win"`；Rust 这里是编译期 `cfg!(windows)`。
/// 差异登记：交叉编译时两者可能不一致（在 Linux 上编出 Windows 包），本机不存在这种构建。
fn is_windows() -> bool {
    cfg!(windows)
}

/// `System.getProperty("user.home", ".")`（`:269`）。
fn user_home() -> PathBuf {
    let key = if is_windows() { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 非空（且不是纯空白）的环境变量，对应 `:225-226` 的 `home != null && !home.isBlank()`。
fn non_blank_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.trim().is_empty())
}

/// 对应 `detectGameJar()`（`:287-297`）：先显式设置，再项目周边的常见位置。
///
/// `:289` 的 `SearchPaths.override("modmaker.gameJar", "MINDUSTRY_JAR")` 在 Rust 侧
/// 只读环境变量那一路（`mm_io::files::search_paths::override_path` 的既有口径：Java 的属性文件
/// 那一路在本迁移里不落盘）。`:290` 特意**要求显式值指向的文件真实存在**，否则继续往下找。
pub fn detect_game_jar() -> Option<PathBuf> {
    let explicit = search_paths::override_path("modmaker.gameJar", "MINDUSTRY_JAR"); // :289
    if let Some(explicit) = explicit
        && explicit.is_file()
    {
        // :290-292
        return Some(explicit);
    }
    // :293-296  三个相对位置（相对当前工作目录，由 search_paths::first_file 负责归一）
    search_paths::first_file(&[
        PathBuf::from("..").join("Mindustry").join(GAME_JAR),
        PathBuf::from("Mindustry").join(GAME_JAR),
        PathBuf::from("..")
            .join("..")
            .join("Mindustry")
            .join(GAME_JAR),
    ])
}

/// 对应 `Files.createTempDirectory(String prefix)`（`:100`）：见模块头第 5 点。
fn create_temp_directory(prefix: &str) -> std::io::Result<PathBuf> {
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let base = std::env::temp_dir();
    loop {
        let serial = SERIAL.fetch_add(1, Ordering::SeqCst);
        let candidate = base.join(format!("{prefix}{}-{serial}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(failure) if failure.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(failure) => return Err(failure),
        }
    }
}

/// `Path` 版的一次性 IO 错误（对应 Java 那边直接抛的 `IOException`／`NullPointerException`）。
fn io_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个会活满 30 秒的子进程（输出全部丢弃，避免管道缓冲把进程吊住）。
    fn sleeper() -> Child {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };
        command.stdout(Stdio::null()).stderr(Stdio::null());
        command.spawn().expect("起测试子进程")
    }

    /// M7a 补移植的 destroy：运行中 → 终止 → 退出码可读 → 再 destroy 是空操作。
    /// 对应 AppShell.stopGame（`:1770-1773`）依赖的那条 `process.destroy()` 语义。
    #[test]
    fn destroy_stops_a_running_process() {
        let process = GameProcess::new(sleeper());
        assert!(process.is_alive(), "子进程应当还在跑");
        process.destroy();
        let _ = process.wait_exit();
        assert!(!process.is_alive(), "destroy 之后进程必须已退出");
        process.destroy(); // 已退出的进程上再来一次：空操作，不 panic
        assert!(!process.is_alive());
    }
}
