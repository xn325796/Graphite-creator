//! `AssetRef.java` 的等价移植：一个被模式携带的文件（贴图、声音、bundle 文本）。

/// 资源引用。
#[derive(Debug, Clone, Default)]
pub struct AssetRef {
    /// 相对模式根的路径。
    pub path: String,
    /// 文件字节数。
    pub size: i64,
    /// 资源类别，如 `sprite` / `sound` / `bundle`。
    pub kind: String,
}

impl AssetRef {
    pub fn new(path: &str, size: i64, kind: &str) -> Self {
        Self { path: path.to_string(), size, kind: kind.to_string() }
    }

    pub fn snapshot(&self) -> AssetRef {
        self.clone()
    }
}
