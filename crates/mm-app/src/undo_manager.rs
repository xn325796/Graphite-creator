//! `ui/UndoManager.java`（91 行）的等价移植：撤销/重做历史本身。
//!
//! Java 的 `UndoManager.Command`(:16-20) 是**闭包**——`undo()`/`redo()` 捕获 project。Rust 侧
//! 不许用 `Rc<RefCell<ModProject>>`，也不许 `unsafe`，所以按 P5 计划 §三（已签字）改成
//! 「**命令即数据**」：`Command<S>` 的 `undo`/`redo` 接受 `&mut S`，被改的状态由调用方持有
//! ——生产路径是 `src-tauri` 的 `State<Mutex<…>>`，本模块的测试里是 `Vec<String>` 调用日志。
//!
//! 栈语义与 Java 逐条对齐：`ArrayDeque.push`(:34) 加在**头部**，所以头部是最近的动作、尾部是
//! 最旧的；超过 `LIMIT` 时 `removeLast`(:37-39) 丢最旧；`poll`(:54/:65) 取头部。
//! Rust 的 `Vec` 下标 0 就是 Java `Deque` 的头部。

/// 命令执行失败的分类（2026-11-25 新增，贴图导入纳入 undo 栈）。
///
/// Java 的 `Command.undo()/redo()` 是 void——纯内存编辑从不失败。Rust 侧的
/// [`SpriteImportCommand`](crate::project_controller::SpriteImportCommand) 要写磁盘，
/// 磁盘会失败，所以 [`Command`]  trait 改成带 `Result`。这是**授权的新设计**
/// （Java modmaker 没有应用内贴图导入），不是 1:1 移植的偏离；既有全部内存命令
/// 恒返回 `Ok(())`，行为逐字不变。壳（`src-tauri/src/bridge.rs`）把变体映射到
/// 稳定码：`NotFound` → not-found，`Io` → io，`Domain` → domain。
#[derive(Debug)]
pub enum CommandError {
    /// 目标记录已不在项目里（被带外删除）。条目留在栈上可重试。
    NotFound(String),
    /// 磁盘读写失败。条目留在栈上可重试。
    Io(std::io::Error),
    /// 领域守卫拒收：项目没有磁盘根、目标路径越出项目根等。
    Domain(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "找不到记录：{id}"),
            Self::Io(err) => write!(f, "贴图读写失败：{err}"),
            Self::Domain(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// 对应 Java `UndoManager.Command`(:16-20)：一次可撤销的用户动作；`redo` 重新应用用户做过的事。
///
/// 与 Java 的唯一差异：两个方法带 `Result`（见 [`CommandError`] 的设计登记）。
/// 失败时 [`UndoManager`] 把命令放回它来的那条栈的头部——历史不被破坏，可重试。
pub trait Command<S> {
    /// Java `Command.undo()`(:17)。
    fn undo(&mut self, state: &mut S) -> Result<(), CommandError>;

    /// Java `Command.redo()`(:19)。
    fn redo(&mut self, state: &mut S) -> Result<(), CommandError>;
}

/// 对应 Java `UndoManager.LIMIT`(:27，package-private `static final int` = 200)。
/// Java 的注释(:22-26)说明了理由：命令闭包持有它改过的 project，无界历史会把一次长会话里
/// 每一次编辑都留着；优先丢掉最旧的，才是用户对「有界撤销」的预期。
pub(crate) const LIMIT: usize = 200;

/// 对应 Java `UndoManager`(:13)：两条栈，`undo_stack` / `redo_stack`(:29-30)。
/// 状态类型 `S` 不绑在结构体上（Java 的 project 是被命令闭包捕获的，Rust 侧改成每次调用显式
/// 传入），所以只在 `undo`/`redo` 两个方法上引入。
pub struct UndoManager<C> {
    undo_stack: Vec<C>,
    redo_stack: Vec<C>,
}

impl<C> Default for UndoManager<C> {
    /// 对应 Java :29-30 两个 `new ArrayDeque<>()`。
    fn default() -> Self {
        Self { undo_stack: Vec::new(), redo_stack: Vec::new() }
    }
}

/// 状态类型 `S` 既不绑在结构体上、也不绑在整个 impl 块上（Rust 的 impl 级泛型参数必须被
/// 自身类型约束，`S` 做不到），而是**只在 `undo`/`redo` 两个方法上引入**——由「这条 `C`
/// 实现了哪个 `Command<S>`」在调用现场决定。这正对应 Java：`UndoManager` 本身完全不知道
/// project，是命令闭包(:16-20)把它捕获走的。
impl<C> UndoManager<C> {
    pub fn new() -> Self {
        Self::default()
    }

    /// 对应 Java `push(Command)`(:33-40)：记录一条新命令，并像任何新编辑那样丢掉 redo 分支。
    pub fn push(&mut self, command: C) {
        // Java `undoStack.push(command)` = addFirst。
        self.undo_stack.insert(0, command);
        self.redo_stack.clear();
        // Java :36 的注释：push 加在头部，所以尾部才是最旧的动作。
        while self.undo_stack.len() > LIMIT {
            self.undo_stack.pop(); // Java `undoStack.removeLast()`
        }
    }

    /// 对应 Java `peekUndo()`(:43-45)：不执行地拿到「`undo` 将要运行的那条命令」，
    /// 用于合并重复编辑（Java `:42` 注释）。
    ///
    /// 返回栈内本体的 `&mut`，所以拿到的就是栈里那一个，不是副本；空栈返回 `None`
    /// （Java `Deque.peek()` 返回 `null`）。
    pub fn peek_undo(&mut self) -> Option<&mut C> {
        self.undo_stack.first_mut() // Java `undoStack.peek()`
    }

    /// 对应 Java `clearRedo()`(:48-50)：只丢 redo 分支，不动 undo 栈。
    pub fn clear_redo(&mut self) {
        self.redo_stack.clear();
    }

    /// 对应 Java `undo()`(:53-61)：`poll` 取头 → 执行 → `push` 到 redo 栈。
    /// 空历史返回 `Ok(None)`（Java :55-57 返回 `null`），不是错误。
    /// 成功时返回值就是刚被执行、现在位于 redo 栈顶的那条命令（Java 返回的同一个 `Command`）。
    ///
    /// 失败口径（2026-11-25 新增，见 [`CommandError`]）：命令执行失败时把它**放回 undo 栈头部**，
    /// 两条栈都回到执行前的形状——历史不被破坏，同一条命令可以重试。
    pub fn undo<S>(&mut self, state: &mut S) -> Result<Option<&mut C>, CommandError>
    where
        C: Command<S>,
    {
        if self.undo_stack.is_empty() {
            return Ok(None);
        }
        let mut command = self.undo_stack.remove(0); // Java `Deque.poll()` = removeFirst
        if let Err(err) = command.undo(state) {
            self.undo_stack.insert(0, command); // 失败回栈，条目留在原位可重试
            return Err(err);
        }
        self.redo_stack.insert(0, command); // Java `redoStack.push(command)`
        Ok(self.redo_stack.first_mut())
    }

    /// 对应 Java `redo()`(:64-72)：与 `undo` 严格对称（失败时放回 redo 栈头部）。
    pub fn redo<S>(&mut self, state: &mut S) -> Result<Option<&mut C>, CommandError>
    where
        C: Command<S>,
    {
        if self.redo_stack.is_empty() {
            return Ok(None);
        }
        let mut command = self.redo_stack.remove(0);
        if let Err(err) = command.redo(state) {
            self.redo_stack.insert(0, command); // 失败回栈，条目留在原位可重试
            return Err(err);
        }
        self.undo_stack.insert(0, command);
        Ok(self.undo_stack.first_mut())
    }

    /// 对应 Java `canUndo()`(:74-76)。
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// 对应 Java `canRedo()`(:78-80)。
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// 对应 Java `depth()`(:82-84)：undo 栈深度。
    pub fn depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// 对应 Java `clear()`(:86-90)：两条栈都忘记——新打开的项目没有历史。
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 对应 Java 夹具 `UndoManagerTest.command(String)`(:20-32)：匿名 `Command` 把
    /// `"undo:"+name` / `"redo:"+name` 记进 `calls`(:18)。
    /// Rust 侧命令是数据，所以 `calls` 不再被闭包捕获，而是作为 `Command<S>` 的
    /// `state: &mut S` 显式传入——断言内容与 Java 完全一致。
    struct RecordedCommand {
        name: String,
    }

    impl Command<Vec<String>> for RecordedCommand {
        fn undo(&mut self, calls: &mut Vec<String>) -> Result<(), CommandError> {
            calls.push(format!("undo:{}", self.name));
            Ok(())
        }

        fn redo(&mut self, calls: &mut Vec<String>) -> Result<(), CommandError> {
            calls.push(format!("redo:{}", self.name));
            Ok(())
        }
    }

    type History = UndoManager<RecordedCommand>;

    fn command(name: &str) -> RecordedCommand {
        RecordedCommand { name: name.to_string() }
    }

    /// 对应 Java `UndoManagerTest.java:34-51` `undoesAndRedoesInStackOrder`。
    #[test]
    fn undoes_and_redoes_in_stack_order() {
        let mut history = History::new();
        let mut calls: Vec<String> = Vec::new();

        let first = command("first");
        let second = command("second");
        history.push(first);
        history.push(second);
        assert_eq!(history.depth(), 2); // Java :40

        history.undo(&mut calls).unwrap();
        history.undo(&mut calls).unwrap();
        // Java :44 assertEquals(List.of("undo:second", "undo:first"), calls)
        assert_eq!(calls, vec!["undo:second".to_string(), "undo:first".to_string()]);
        assert!(!history.can_undo()); // Java :45
        assert!(history.can_redo()); // Java :46

        history.redo(&mut calls).unwrap();
        assert_eq!(calls[calls.len() - 1], "redo:first"); // Java :49
        assert_eq!(history.depth(), 1); // Java :50
    }

    /// 对应 Java `UndoManagerTest.java:53-62` `aNewCommandDropsTheRedoBranch`。
    #[test]
    fn a_new_command_drops_the_redo_branch() {
        let mut history = History::new();
        let mut calls: Vec<String> = Vec::new();

        history.push(command("old"));
        history.undo(&mut calls).unwrap();
        assert!(history.can_redo()); // Java :57

        history.push(command("new"));
        assert!(!history.can_redo(), "一条新编辑作废 redo 分支"); // Java :60
        assert_eq!(history.depth(), 1); // Java :61
    }

    /// 对应 Java `UndoManagerTest.java:64-70` `peekDoesNotRunTheCommand`。
    ///
    /// Java :68 的 `assertSame(command, history.peekUndo())` 断的是**引用同一性**。Rust 侧
    /// `push` 已经拿走所有权，栈里的那个对象与测试里的局部变量不可能再是同一个指针，所以这里
    /// 证一条更强的性质：`peek_undo` 给的是**栈内本体的 `&mut`**——经它改名后，真正被执行的
    /// 命令名跟着变了。若 `peek` 返回的是副本，改名不会影响 `undo`。
    /// Java :69 `assertEquals(List.of(), calls)`（peek 不执行）这条断言原样保留。
    #[test]
    fn peek_does_not_run_the_command() {
        let mut history = History::new();
        let mut calls: Vec<String> = Vec::new();

        history.push(command("peeked"));
        let peeked = history.peek_undo().expect("栈里有命令");
        peeked.name = String::from("peeked-renamed");
        assert!(calls.is_empty(), "peek 不执行命令"); // Java :69

        history.undo(&mut calls).unwrap();
        assert_eq!(calls, vec!["undo:peeked-renamed".to_string()]);
    }

    /// 对应 Java `UndoManagerTest.java:72-85` `theHistoryDropsTheOldestCommandsPastTheLimit`。
    #[test]
    fn the_history_drops_the_oldest_commands_past_the_limit() {
        let mut history = History::new();
        let mut calls: Vec<String> = Vec::new();

        for i in 0..LIMIT + 3 {
            history.push(command(&format!("c{}", i)));
        }
        assert_eq!(history.depth(), LIMIT, "历史保持有界"); // Java :77

        // Java :79-82：最旧的三条已经没了，最新的那条还在顶上。
        for _ in 0..3 {
            history.undo(&mut calls).unwrap();
        }
        // Java :83-84 assertEquals(List.of("undo:c202", "undo:c201", "undo:c200"), calls)
        assert_eq!(
            calls,
            vec![
                format!("undo:c{}", LIMIT + 2),
                format!("undo:c{}", LIMIT + 1),
                format!("undo:c{}", LIMIT)
            ]
        );
    }

    /// 对应 Java `UndoManagerTest.java:87-97` `clearingForgetsBothBranches`。
    #[test]
    fn clearing_forgets_both_branches() {
        let mut history = History::new();
        let mut calls: Vec<String> = Vec::new();

        history.push(command("kept"));
        history.undo(&mut calls).unwrap();
        history.clear();
        assert!(!history.can_undo()); // Java :92
        assert!(!history.can_redo()); // Java :93
        assert_eq!(history.depth(), 0); // Java :94
        assert!(history.undo(&mut calls).unwrap().is_none(), "空历史撤销是无操作，不是错误"); // Java :95
        assert!(history.redo(&mut calls).unwrap().is_none()); // Java :96
    }

    /// 可失败的命令夹具（2026-11-25 新增）：两个方向各一个开关，模拟磁盘写失败。
    /// 与 `RecordedCommand` 分开，Java 夹具的逐字对照不受影响。
    /// `Debug` 只为 `Result::unwrap_err`（它要求 `Ok` 侧 `Debug`，`&mut C` 因此要有 `C: Debug`）。
    #[derive(Debug)]
    struct FlakyCommand {
        fail_undo: bool,
        fail_redo: bool,
    }

    impl Command<Vec<String>> for FlakyCommand {
        fn undo(&mut self, calls: &mut Vec<String>) -> Result<(), CommandError> {
            if self.fail_undo {
                return Err(CommandError::Io(std::io::Error::other("磁盘拒绝写入")));
            }
            calls.push("undo:flaky".to_string());
            Ok(())
        }

        fn redo(&mut self, calls: &mut Vec<String>) -> Result<(), CommandError> {
            if self.fail_redo {
                return Err(CommandError::Io(std::io::Error::other("磁盘拒绝写入")));
            }
            calls.push("redo:flaky".to_string());
            Ok(())
        }
    }

    /// 失败口径（[`CommandError`] 的设计登记）：undo 失败时命令放回 undo 栈头部，
    /// 两条栈形状不变；排除故障后重试同一条命令成功，历史照常推进。
    #[test]
    fn a_failed_undo_keeps_the_command_on_top_and_retries() {
        let mut history = UndoManager::new();
        let mut calls: Vec<String> = Vec::new();
        history.push(FlakyCommand { fail_undo: true, fail_redo: false });

        let err = history.undo(&mut calls).unwrap_err();
        assert!(matches!(err, CommandError::Io(_)), "实际：{err:?}");
        assert!(calls.is_empty(), "失败的命令不该留下执行痕迹");
        assert_eq!(history.depth(), 1, "条目留在栈上");
        assert!(history.can_undo());
        assert!(!history.can_redo(), "失败不进 redo 栈");

        // 排除故障（经 peek_undo 拿到的就是栈内本体）后重试：同一条命令成功。
        history.peek_undo().expect("条目还在").fail_undo = false;
        history.undo(&mut calls).unwrap();
        assert_eq!(calls, vec!["undo:flaky".to_string()]);
        assert_eq!(history.depth(), 0);
        assert!(history.can_redo());
    }

    /// 与上一条对称：redo 失败时命令放回 redo 栈头部，undo 栈不受影响，可反复重试。
    #[test]
    fn a_failed_redo_keeps_the_command_on_the_redo_stack() {
        let mut history = UndoManager::new();
        let mut calls: Vec<String> = Vec::new();
        history.push(FlakyCommand { fail_undo: false, fail_redo: true });
        history.undo(&mut calls).unwrap();
        assert_eq!(history.depth(), 0);

        for _ in 0..2 {
            let err = history.redo(&mut calls).unwrap_err();
            assert!(matches!(err, CommandError::Io(_)), "实际：{err:?}");
            assert_eq!(history.depth(), 0, "失败不进 undo 栈");
            assert!(history.can_redo(), "条目留在 redo 栈上可重试");
        }
        assert_eq!(calls, vec!["undo:flaky".to_string()], "redo 从未执行成功");
    }
}
