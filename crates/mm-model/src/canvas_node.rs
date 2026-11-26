//! `CanvasNode.java` 的等价移植：世界画布上的一个节点。

/// 节点类型：内容 / 便签 / 分组框。
pub const KIND_CONTENT: &str = "content";
pub const KIND_NOTE: &str = "note";
pub const KIND_GROUP: &str = "group";

#[derive(Debug, Clone)]
pub struct CanvasNode {
    pub id: Option<String>,
    pub kind: String,
    pub content_id: Option<String>,
    pub ref_name: Option<String>,
    pub ref_type: Option<String>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub text: String,
    pub color: String,
}

impl Default for CanvasNode {
    fn default() -> Self {
        Self {
            id: None,
            kind: KIND_CONTENT.to_string(),
            content_id: None,
            ref_name: None,
            ref_type: None,
            x: 0.0,
            y: 0.0,
            w: 260.0,
            h: 130.0,
            text: String::new(),
            color: String::new(),
        }
    }
}

impl CanvasNode {
    pub fn snapshot(&self) -> CanvasNode {
        self.clone()
    }
}
