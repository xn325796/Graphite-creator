//! M7a 科技森林的下发映射：领域层森林（[`mm_io::canvas::tech_tree_index`]，Java
//! `TechTreeIndex` 的等价移植）→ 前端 DTO。`display_name`/research 归一化要问 controller
//! 与 `mm_fmt::tech`，所以这段放在「调用方」层而不是 `dto.rs`（与 `bridge::card_table`
//! 同一理由：dto.rs 不碰 controller）。
//!
//! ## 为什么独立成模块，而不留在 `bridge.rs`
//!
//! 本模块**不依赖 tauri**——只收 `&ProjectController`，不收 `Actor`/`AppHandle`。
//! 2026-11-20 实测：链接了 tauri 的**测试**可执行文件在本机（Win11 Insider 26300，
//! x86_64-pc-windows-gnu）加载即死 `STATUS_ENTRYPOINT_NOT_FOUND`（0xC0000139），
//! 而同一套 DLL 导入的产物 exe 正常启动；`tests/layering.rs` 不引用 lib 本体所以能跑。
//! 把纯映射挪进这个无 tauri 的模块后，集成测试只链接本模块 + 领域 crate，
//! 绕开那条加载故障，也让「森林下发契约」可以被 `tests/tech_forest.rs` 直接钉住。
//! `bridge::Actor::tech_forest` 只是委托到这里。

use mm_app::project_controller::ProjectController;
use mm_io::canvas::tech_tree_index::{self, Entry};
use mm_model::ModProject;

use crate::dto;

/// 森林下发的内核：无项目时回空森林（与 `Actor::contents` 同口径），不 panic。
pub fn tech_forest_dto(controller: &ProjectController) -> dto::TechForestDto {
    let empty = || dto::TechForestDto {
        roots: Vec::new(),
        unassigned: Vec::new(),
        refused: Vec::new(),
    };
    let Some(project) = controller.project() else {
        return empty();
    };
    let forest = tech_tree_index::build(project, controller.registry());
    let nodes = |entries: &[Entry]| -> Vec<dto::TechNodeDto> {
        entries
            .iter()
            .filter_map(|entry| tech_entry_dto(entry, project, controller).record)
            .collect()
    };
    dto::TechForestDto {
        roots: forest
            .roots
            .iter()
            .map(|entry| tech_entry_dto(entry, project, controller))
            .collect(),
        unassigned: nodes(&forest.unassigned),
        refused: nodes(&forest.patches_with_research),
    }
}

/// 森林条目 → DTO。原版组节点 `content_id == None`，record 留空；
/// research 摘要走 [`mm_fmt::tech::read`]（`"item/amount"` 归一），前端不再解析 JSON 文本。
fn tech_entry_dto(
    entry: &Entry,
    project: &ModProject,
    controller: &ProjectController,
) -> dto::TechEntryDto {
    let record = entry
        .content_id
        .as_deref()
        .and_then(|id| project.content_by_id(id));
    let node = record.map(|record| {
        let research = record
            .fields
            .get(mm_fmt::tech::FIELD)
            .filter(|value| !value.is_null())
            .map(|value| dto::research_dto(&mm_fmt::tech::read(Some(value))));
        dto::tech_node_dto(record, &controller.display_name(record), research)
    });
    dto::TechEntryDto {
        record: node,
        key: entry.label.clone(),
        vanilla: entry.vanilla,
        problem: dto::problem_key(entry.problem).map(str::to_string),
        children: entry
            .children
            .iter()
            .map(|child| tech_entry_dto(child, project, controller))
            .collect(),
    }
}
