//! `Workspace.java` 的等价移植：每种内容类型在主区哪个工作区里编辑。
//!
//! 方块构建器把节点画布留作流程编辑器（以及一切参与引用边的东西）；简单资产得到资源网格；单位有
//! 自己的网格视图；星球与星区得到排布视图；科技树则是全局的「所有记录的 research 索引」视图，而不是
//! 逐类型的。

use crate::ContentType;
use mm_schema::ContentType::{
    Block, Item, Liquid, Planet, Sector, Status, Team, Unit, Weather,
};

/// 主区工作区。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Workspace {
    /// 资源网格：物品、流体与其它简单资产。
    Assets,
    /// 节点画布上的流程编辑：方块，以及一切与引用相关的东西。
    Canvas,
    /// 单位列表：单位在方块构建器旁边有自己的页签。
    Units,
    /// 排布视图：星球及其星区。
    Cosmos,
    /// 所有记录的 research 索引：`research` 字段所描述的科技树。
    TechTree,
    /// 为（尚未实现的）脚本工作区保留；不会路由到任何内容类型。
    Script,
}

impl Workspace {
    pub fn of(type_: ContentType) -> Workspace {
        match type_ {
            Item | Liquid | Status | Weather | Team => Workspace::Assets,
            Block => Workspace::Canvas,
            Unit => Workspace::Units,
            Planet | Sector => Workspace::Cosmos,
            _ => Workspace::Canvas,
        }
    }

    /// 资源网格列出的类型——每个「有文件支撑且规范归属就是资产页签」的类型。其余类型拥有专属工作区
    /// （canvas / units / cosmos），不能被列两次；仅内联的类型经由其宿主记录到达。放在 [`Workspace::of`]
    /// 旁边，使类型到工作区的映射恰好只存在一处。
    pub fn asset_types() -> Vec<ContentType> {
        ContentType::all()
            .into_iter()
            .filter(|t| t.file_backed() && Workspace::of(*t) == Workspace::Assets)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_content_types_to_workspaces() {
        assert_eq!(Workspace::Assets, Workspace::of(Item));
        assert_eq!(Workspace::Assets, Workspace::of(Liquid));
        assert_eq!(Workspace::Cosmos, Workspace::of(Planet));
        assert_eq!(Workspace::Cosmos, Workspace::of(Sector));
        assert_eq!(Workspace::Canvas, Workspace::of(Block));
        // 单位在方块构建器旁边有自己的页签（参考 (1)/(2).svg）。
        assert_eq!(Workspace::Units, Workspace::of(Unit));
        // 状态、天气、队伍重新成为简单资产：它们的归属是资源网格——2026-10-17 的 UI 审计把所有
        // 没有自己页签的东西收敛为它的唯一列表。
        assert_eq!(Workspace::Assets, Workspace::of(Status));
        assert_eq!(Workspace::Assets, Workspace::of(Weather));
        assert_eq!(Workspace::Assets, Workspace::of(Team));
        // 仅内联的类型留在画布上：它们是宿主记录内部的流程参与者。
        assert_eq!(Workspace::Canvas, Workspace::of(ContentType::Bullet));
    }

    #[test]
    fn every_file_backed_type_has_exactly_one_canonical_home() {
        assert_eq!(
            vec![Item, Liquid, Status, Weather, Team],
            Workspace::asset_types(),
            "资源网格恰好列出那些没有自己页签的类型"
        );
        for type_ in ContentType::all() {
            if !type_.file_backed() {
                continue;
            }
            let listed_in_assets = Workspace::asset_types().contains(&type_);
            assert_eq!(
                listed_in_assets,
                Workspace::of(type_) == Workspace::Assets,
                "{type_} 要么列在资产网格里，要么路由到自己的页签，不能两者都是"
            );
        }
    }

    #[test]
    fn inline_types_are_not_listed_in_the_assets_grid() {
        for type_ in ContentType::all() {
            if type_.file_backed() {
                continue;
            }
            assert!(
                !Workspace::asset_types().contains(&type_),
                "仅内联的 {type_} 经由其宿主记录到达"
            );
            assert_eq!(
                Workspace::of(type_),
                Workspace::Canvas,
                "仅内联的 {type_} 路由到画布"
            );
        }
    }

    #[test]
    fn unit_command_and_stance_route_to_the_canvas() {
        assert_eq!(Workspace::Canvas, Workspace::of(ContentType::UnitCommand));
        assert_eq!(Workspace::Canvas, Workspace::of(ContentType::UnitStance));
    }
}
