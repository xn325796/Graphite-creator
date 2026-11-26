//! mm-cli：`dev.modmaker.cli`（1 类 384 行）的等价移植——headless 命令行入口。
//!
//! Java 侧只有一个 `ModMakerCli` 类，它的 `main` 只做 `System.exit(run(args))`，因此这里同样
//! 只有一条链：[`mod_maker_cli`] 是全部逻辑，`src/main.rs` 是那行 `exit`。把逻辑放在 lib 里
//! 而不塞进 bin，是为了让集成测试能像 Java 那样直接断言 `handles()` 的返回值
//! （Java `ModMakerCliTest.java:45-48`），`run()` 的退出码则通过 [`env!("CARGO_BIN_EXE_modmaker")`]
//! 起子进程观察，理由写在 `tests/mod_maker_cli.rs` 的模块注释里。

pub mod mod_maker_cli;

pub use mod_maker_cli::{handles, run, BUILD_FAILED, OK, USAGE, VALIDATION_FAILED};
