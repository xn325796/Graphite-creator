//! P1-4「原生关窗绕过未保存守卫」的壳侧集成测试。
//!
//! 两层覆盖，口径与既有测试一致：
//! 1. **内核**（`close_guard`，tauri-free，直接从 lib 引用，同 `sprite_import.rs`）：
//!    代际状态机的全部判定——confirm 恰好一次、重复调用摘不到请求、兜底与 confirm
//!    互斥、新一代顶掉旧计时器、并发下恰好一方赢、兜底秒数钉住契约的 3s。
//!    2026-11-25 契约扩展（`window_close_hold`）同层覆盖：hold 摘除兜底计时但不摘取
//!    请求（hold 后 confirm 必须成功）、重复 hold 摘不到、新代顶掉旧 hold、
//!    hold/confirm/兜底三方并发下关窗权恰好一方赢。
//! 2. **接线针**（纯文本扫描，同 `layering.rs` 的针法）：命令注册进 `generate_handler!`
//!    且走 `bridge.send` 桥层、`CloseRequested` 被 `prevent_close` 拦下并广播契约事件、
//!    关窗用 `destroy` 而非 `close`（防循环）、错误口径用稳定码 `not-found`；
//!    hold 命令同针法，另钉住「hold 不碰窗口、不走模型健康守卫」。
//!
//! ## 边界（如实说明，别读成它做不到的事）
//!
//! `CloseRequested` 的真实 GUI 行为（原生 X/Alt+F4 → 事件到前端 → 真窗销毁）与
//! 3s 兜底的真实计时**无法在无头测试里构造**（需要事件循环与真窗口，tauri 的
//! lib 单元测试可执行文件在本机加载即死，见 `dto_contract.rs` 模块头）。这部分
//! 留给前端接线后的真实壳验收；这里钉住的是判定逻辑与接线不被改丢。

use std::fs;
use std::path::Path;
use std::sync::Barrier;

use graphitecreator_lib::close_guard::{CLOSE_FALLBACK_SECS, CloseGuard};

// ===== 内核：代际状态机 =====

/// 无待确认请求时 confirm 摘不到东西（壳映射为稳定码 `not-found`）：
/// 覆盖「未经原生关闭就调命令」与「兜底已放行后再调」两种来路。
#[test]
fn confirm_without_a_pending_request_yields_none() {
    let guard = CloseGuard::new();
    assert_eq!(guard.confirm(), None);
}

/// confirm 恰好赢一次：第一次摘到本代请求，重复调用摘到 0 → None。
#[test]
fn confirm_wins_once_and_duplicate_calls_yield_none() {
    let guard = CloseGuard::new();
    let generation = guard.request();
    assert_eq!(guard.confirm(), Some(generation));
    assert_eq!(guard.confirm(), None, "重复调用必须摘不到请求");
    assert_eq!(guard.confirm(), None);
}

/// 无人应答时兜底赢：清掉请求，之后的 confirm 与同一计时器都作废。
#[test]
fn fallback_wins_when_unanswered_and_clears_the_request() {
    let guard = CloseGuard::new();
    let generation = guard.request();
    assert!(guard.fallback_due(generation));
    assert_eq!(guard.confirm(), None, "兜底放行后 confirm 不得再关一次");
    assert!(!guard.fallback_due(generation), "同一计时器不得赢两次");
}

/// 前端已确认时，到期的兜底计时器必须静默作废（不能双关窗）。
#[test]
fn fallback_is_superseded_by_confirm() {
    let guard = CloseGuard::new();
    let generation = guard.request();
    assert_eq!(guard.confirm(), Some(generation));
    assert!(!guard.fallback_due(generation));
}

/// 待确认期间用户又按了一次 X：新一代顶掉旧一代，旧计时器作废，confirm 摘到新一代。
#[test]
fn a_second_close_request_supersedes_the_old_timer() {
    let guard = CloseGuard::new();
    let first = guard.request();
    let second = guard.request();
    assert_ne!(first, second);
    assert!(!guard.fallback_due(first), "旧计时器必须作废");
    assert_eq!(guard.confirm(), Some(second));
}

/// confirm（壳线程）与兜底（计时线程）并发抢同一个请求：恰好一方赢。
/// 屏障对齐后真并发跑 200 轮，钉住原子摘取的互斥性。
#[test]
fn confirm_and_fallback_race_has_exactly_one_winner() {
    for round in 0..200 {
        let guard = CloseGuard::new();
        let generation = guard.request();
        let barrier = Barrier::new(2);
        let (confirmed, fell_back) = std::thread::scope(|scope| {
            let confirm = scope.spawn(|| {
                barrier.wait();
                guard.confirm().is_some()
            });
            let fallback = scope.spawn(|| {
                barrier.wait();
                guard.fallback_due(generation)
            });
            (
                confirm.join().expect("confirm 线程"),
                fallback.join().expect("兜底线程"),
            )
        });
        assert!(
            confirmed != fell_back,
            "第 {round} 轮：confirm={confirmed} fallback={fell_back}，必须恰好一方赢"
        );
    }
}

/// 兜底秒数钉住共享契约的 3s（理由在 `close_guard::CLOSE_FALLBACK_SECS` 的注释）。
#[test]
fn fallback_window_is_three_seconds_per_contract() {
    assert_eq!(CLOSE_FALLBACK_SECS, 3);
}

// ===== 内核：hold（2026-11-25 契约扩展，window_close_hold）=====

/// 无待确认请求时 hold 摘不到计时（壳映射为稳定码 `not-found`，口径同 confirm）。
#[test]
fn hold_without_a_pending_request_yields_none() {
    let guard = CloseGuard::new();
    assert_eq!(guard.hold(), None);
}

/// hold 的核心语义：摘除该代兜底计时，但**不摘取请求**——兜底不得再赢，
/// confirm 必须仍能摘到同一代（对话框「确认关闭」在无限决策时间后依然有效）。
#[test]
fn hold_cancels_the_fallback_but_keeps_the_request_for_confirm() {
    let guard = CloseGuard::new();
    let generation = guard.request();
    assert_eq!(guard.hold(), Some(generation));
    assert!(!guard.fallback_due(generation), "hold 后兜底不得赢");
    assert_eq!(guard.confirm(), Some(generation), "hold 不摘取请求，confirm 必须仍成功");
    assert_eq!(guard.confirm(), None, "confirm 仍恰好赢一次");
}

/// 重复 hold 摘不到（not-found）；confirm 摘走后、兜底放行后 hold 同样摘不到。
#[test]
fn duplicate_hold_and_hold_after_resolution_yield_none() {
    let guard = CloseGuard::new();
    let generation = guard.request();
    assert_eq!(guard.hold(), Some(generation));
    assert_eq!(guard.hold(), None, "重复 hold 不得再摘一次计时");
    assert_eq!(guard.confirm(), Some(generation));
    assert_eq!(guard.hold(), None, "confirm 已摘走请求，hold 摘不到");
    let next = guard.request();
    assert!(guard.fallback_due(next));
    assert_eq!(guard.hold(), None, "兜底已放行，hold 摘不到");
}

/// hold 期间用户又按了一次 X：新代顶掉旧代一切状态——旧计时器作废，新代计时重新
/// 活跃、可再次 hold（前端对每次事件各调一次 hold），confirm 摘到新一代。
#[test]
fn a_second_close_request_supersedes_the_hold() {
    let guard = CloseGuard::new();
    let first = guard.request();
    assert_eq!(guard.hold(), Some(first));
    let second = guard.request();
    assert_ne!(first, second);
    assert!(!guard.fallback_due(first), "旧计时器必须作废");
    assert_eq!(guard.hold(), Some(second), "新代计时重新活跃，可再次 hold");
    assert!(!guard.fallback_due(second), "新代 hold 后兜底同样不得赢");
    assert_eq!(guard.confirm(), Some(second));
}

/// hold（壳线程）与兜底（计时线程）并发抢同一代的计时处置权：恰好一方赢。
/// 屏障对齐后真并发跑 200 轮，钉住同一原子字上 CAS 的互斥性。
#[test]
fn hold_and_fallback_race_has_exactly_one_winner() {
    for round in 0..200 {
        let guard = CloseGuard::new();
        let generation = guard.request();
        let barrier = Barrier::new(2);
        let (held, fell_back) = std::thread::scope(|scope| {
            let hold = scope.spawn(|| {
                barrier.wait();
                guard.hold().is_some()
            });
            let fallback = scope.spawn(|| {
                barrier.wait();
                guard.fallback_due(generation)
            });
            (
                hold.join().expect("hold 线程"),
                fallback.join().expect("兜底线程"),
            )
        });
        assert!(
            held != fell_back,
            "第 {round} 轮：hold={held} fallback={fell_back}，必须恰好一方赢"
        );
    }
}

/// hold／confirm／兜底三方并发：关窗权恰好一方赢，且 hold 不吞掉 confirm。
/// 合法结局只有三种（由第一个完成状态迁移的操作决定）：
/// - confirm 先 swap：confirm 赢，hold 与兜底都摘不到；
/// - hold 先置标志：hold 赢计时处置，confirm 随后仍摘到代（必须成功），兜底作废；
/// - 兜底先 CAS：兜底赢，hold 与 confirm 都摘不到。
///
/// 不变量：兜底赢 ⇔ 前端路径（hold/confirm）全没赢——不会双关窗，也不会无人处置。
#[test]
fn hold_confirm_and_fallback_three_way_race_has_exactly_one_winner() {
    for round in 0..200 {
        let guard = CloseGuard::new();
        let generation = guard.request();
        let barrier = Barrier::new(3);
        let (held, confirmed, fell_back) = std::thread::scope(|scope| {
            let hold = scope.spawn(|| {
                barrier.wait();
                guard.hold().is_some()
            });
            let confirm = scope.spawn(|| {
                barrier.wait();
                guard.confirm()
            });
            let fallback = scope.spawn(|| {
                barrier.wait();
                guard.fallback_due(generation)
            });
            (
                hold.join().expect("hold 线程"),
                confirm.join().expect("confirm 线程"),
                fallback.join().expect("兜底线程"),
            )
        });
        assert!(
            fell_back != (held || confirmed.is_some()),
            "第 {round} 轮：hold={held} confirm={confirmed:?} fallback={fell_back}，关窗权必须恰好一方赢"
        );
        if held {
            assert_eq!(
                confirmed,
                Some(generation),
                "第 {round} 轮：hold 赢后 confirm 必须仍摘到同一代"
            );
        }
        if let Some(won) = confirmed {
            assert_eq!(won, generation, "第 {round} 轮：confirm 摘到的代际须掩掉 hold 标志位");
        }
    }
}

// ===== 接线针：命令注册、桥层路由、事件与防循环 =====

fn source(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("读 {} 失败：{err}", path.display()))
}

/// 从 `start` 标记起到 `end` 标记止的一段源码（含首不含尾的标记本身）。
fn block<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text
        .find(start)
        .unwrap_or_else(|| panic!("找不到标记 {start:?}"));
    let rest = &text[from..];
    let to = rest
        .find(end)
        .unwrap_or_else(|| panic!("在 {start:?} 之后找不到结束标记 {end:?}"));
    &rest[..to]
}

/// 事件名常量与共享契约逐字相同（前端 `listen` 的字符串就是它）。
#[test]
fn event_name_constant_matches_the_contract() {
    assert_eq!(
        graphitecreator_lib::bridge::EVENT_WINDOW_CLOSE_REQUESTED,
        "window-close-requested"
    );
}

/// `window_confirm_close` 注册进 `generate_handler!`，且命令体走 `bridge.send`
/// 桥层到 `Actor::window_confirm_close`——与其余 44 条命令同一路由口径。
#[test]
fn command_is_registered_and_routes_through_the_bridge() {
    let lib = source("lib.rs");
    let handler = block(&lib, "generate_handler![", "]");
    assert!(
        handler.contains("window_confirm_close"),
        "window_confirm_close 没有注册进 generate_handler!"
    );
    let command = block(&lib, "async fn window_confirm_close(", "\n}");
    assert!(
        command.contains("bridge.send(") && command.contains("actor.window_confirm_close()"),
        "命令必须走桥层投给壳线程，实际：{command}"
    );
}

/// 主窗口 `CloseRequested` 被拦下（prevent_close）、广播契约事件（空载荷）、
/// 并起兜底计时；事件名进 `_EVENT_NAMES` 编译期锚点（6 条）。
#[test]
fn close_requested_is_prevented_broadcast_and_backstopped() {
    let lib = source("lib.rs");
    let handler = block(&lib, "window.on_window_event(", "});");
    for needle in [
        "WindowEvent::CloseRequested { api, .. }",
        "api.prevent_close()",
        "event_guard.request()",
        "event_handle.emit(bridge::EVENT_WINDOW_CLOSE_REQUESTED, ())",
        "spawn_close_fallback(",
    ] {
        assert!(handler.contains(needle), "on_window_event 缺 {needle:?}");
    }
    let anchor = block(&lib, "const _EVENT_NAMES:", "];");
    assert!(
        anchor.contains("; 6]") && anchor.contains("EVENT_WINDOW_CLOSE_REQUESTED"),
        "事件锚点必须扩到 6 条并含新事件：{anchor}"
    );
}

/// 防循环与错误口径：confirm 与兜底都用 `destroy`（`close` 会再触发 CloseRequested）；
/// 重复调用／无窗口回稳定码 `not-found`；兜底秒数取自契约常量而非第二份魔数。
#[test]
fn close_paths_destroy_and_report_stable_not_found() {
    let bridge = source("bridge.rs");
    let confirm = block(&bridge, "pub fn window_confirm_close(", "\n    }");
    assert!(
        confirm.contains(".destroy()"),
        "confirm 必须 destroy：{confirm}"
    );
    assert!(
        !confirm.contains(".close()"),
        "confirm 不得用 close（会再触发 CloseRequested 循环）"
    );
    assert_eq!(
        confirm.matches("\"not-found\"").count(),
        2,
        "重复调用与无窗口两处都必须回稳定码 not-found：{confirm}"
    );

    let lib = source("lib.rs");
    let destroy = block(&lib, "fn destroy_main_window(", "\n}");
    assert!(destroy.contains("window.destroy()"));
    assert!(!destroy.contains(".close()"), "兜底同样不得用 close");
    let fallback = block(&lib, "fn spawn_close_fallback(", "\n}");
    assert!(
        fallback.contains("close_guard::CLOSE_FALLBACK_SECS"),
        "兜底秒数必须单点取自契约常量"
    );
    assert!(
        fallback.contains("catch_unwind"),
        "兜底线程的 panic 防线口径与 actor／导入线程一致"
    );
}

/// `window_close_hold`（契约扩展）注册进 `generate_handler!`，且命令体走 `bridge.send`
/// 桥层到 `Actor::window_close_hold`——与其余 44 条命令同一路由口径。
#[test]
fn hold_command_is_registered_and_routes_through_the_bridge() {
    let lib = source("lib.rs");
    let handler = block(&lib, "generate_handler![", "]");
    assert!(
        handler.contains("window_close_hold"),
        "window_close_hold 没有注册进 generate_handler!"
    );
    let command = block(&lib, "async fn window_close_hold(", "\n}");
    assert!(
        command.contains("bridge.send(") && command.contains("actor.window_close_hold()"),
        "hold 命令必须走桥层投给壳线程，实际：{command}"
    );
}

/// hold 的壳侧口径：只经代际状态机摘计时（`close_guard.hold()`），**不碰窗口**
/// （不得 destroy/close——取消关闭时窗口必须保持打开）；摘不到回稳定码 `not-found`；
/// 与 confirm 同口径，不走 `require_project`／poisoned 模型健康守卫（壳受损也要能停掉自动放行）。
#[test]
fn hold_actor_only_cancels_the_timer_and_reports_stable_not_found() {
    let bridge = source("bridge.rs");
    let hold = block(&bridge, "pub fn window_close_hold(", "\n    }");
    assert!(
        hold.contains("close_guard.hold()"),
        "hold 必须经代际状态机摘计时：{hold}"
    );
    assert!(
        !hold.contains(".destroy()") && !hold.contains(".close()"),
        "hold 不得碰窗口（取消关闭时窗口保持打开）：{hold}"
    );
    assert!(
        hold.contains("\"not-found\""),
        "重复 hold／无待确认请求必须回稳定码 not-found：{hold}"
    );
    assert!(
        !hold.contains("require_project") && !hold.contains("poisoned"),
        "hold 与 confirm 同口径：不走模型健康守卫，壳受损也要能停掉自动放行：{hold}"
    );
}
