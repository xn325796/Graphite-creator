//! `CanvasEdge.java` 的等价移植：连接两个节点的一条边。

/// 一条边。
#[derive(Debug, Clone, Default)]
pub struct CanvasEdge {
    pub id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// 这条边所代表的依赖字段（如 `requirements`），可选。
    pub field_key: Option<String>,
    pub label: String,
    /// 用户拖动过的控制点，按 `[x, y]` 顺序。
    pub reroutes: Vec<[f64; 2]>,
}

impl CanvasEdge {
    pub fn snapshot(&self) -> CanvasEdge {
        self.clone()
    }
}
