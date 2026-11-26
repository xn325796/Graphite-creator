/// tauri-build 生成 `libresource.a`（含 `1 24` RT_MANIFEST：Common-Controls v6 依赖 +
/// VERSIONINFO + 图标），但只用 `cargo:rustc-link-arg-bins` 链给 **bin**。
///
/// 2026-11-20 M7a 实测：集成测试 exe 会链入整份 lib（tauri/wry），因而导入 comctl32 的
/// v6-only 入口（`TaskDialogIndirect`/`SetWindowSubclass` 等）；没有 v6 manifest 时加载器
/// 解析到 System32 的 v5 comctl32，进程创建即 `STATUS_ENTRYPOINT_NOT_FOUND`（0xC0000139），
/// 测试一条都跑不起来。这里把同一份资源补链给测试目标（与 bin 完全同源，纯 wiring）。
fn main() {
    tauri_build::build();

    if cfg!(windows) {
        let Ok(out_dir) = std::env::var("OUT_DIR") else {
            return;
        };
        let resource = std::path::Path::new(&out_dir).join("libresource.a");
        if resource.exists() {
            println!("cargo:rustc-link-arg-tests={}", resource.display());
        }
    }
}
