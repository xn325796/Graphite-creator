//! `Board.java` 的等价移植：一个画布，含节点、边、视口与边样式。

use crate::{canvas_edge::CanvasEdge, canvas_node::CanvasNode};

/// `Board.Viewport`：画布的平移/缩放状态。
#[derive(Debug, Clone)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, zoom: 1.0 }
    }
}

#[derive(Debug, Clone)]
pub struct Board {
    pub id: Option<String>,
    pub name: String,
    pub file: String,
    pub nodes: Vec<CanvasNode>,
    pub edges: Vec<CanvasEdge>,
    pub viewport: Viewport,
    pub edge_style: String,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            id: None,
            name: "Main".to_string(),
            file: "main.canvas".to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: Viewport::default(),
            edge_style: "bezier".to_string(),
        }
    }
}

impl Board {
    pub fn snapshot(&self) -> Board {
        self.clone()
    }
}
