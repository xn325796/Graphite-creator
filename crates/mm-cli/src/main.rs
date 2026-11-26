//! `ModMakerCli.main`（第 46-48 行）的等价物：Java 是 `System.exit(run(Arrays... args))`，
//! 这里把进程的退出码交给 [`std::process::exit`]。
//!
//! 与 Java 的一处表达差异：`args[0]`。Java 的 `main(String[] args)` 里 `args[0]` 就是子命令
//! （`validate` / `build`），Rust 的 [`std::env::args`] 第 0 个是程序名，故这里 `skip(1)`
//! 之后交给 [`mm_cli::run`]，两边传进 `run` 的切片完全一致。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(mm_cli::mod_maker_cli::run(&args));
}
