//! 保插入顺序的字符串键映射，镜像 Java 的 `LinkedHashMap`（`put` 覆盖时保留原位置）。
//!
//! 用来替代 `indexmap` 之类依赖：本 crate 只需少数量级很小的有序 map 操作，零外部依赖更稳。

use std::collections::HashMap;
use std::fmt;

/// 超过这个键数才建哈希索引：小表线性扫更快且省分配；大表（bundle 字符串表、
/// fields_of 合并出的字段表）不建索引则 put/get 是 O(n²)。与 mm-json Object 同策略。
const INDEX_THRESHOLD: usize = 32;

/// 有序键值表：`entries` 保序，键唯一。
pub struct KeyedMap<T> {
    entries: Vec<(String, T)>,
    /// 键 → entries 下标，纯派生数据；相等性只看 entries（见 PartialEq）。
    index: Option<HashMap<String, usize>>,
}

impl<T> Default for KeyedMap<T> {
    fn default() -> Self {
        Self { entries: Vec::new(), index: None }
    }
}

impl<T: Clone> Clone for KeyedMap<T> {
    fn clone(&self) -> Self {
        Self { entries: self.entries.clone(), index: self.index.clone() }
    }
}

impl<T: fmt::Debug> fmt::Debug for KeyedMap<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<T: PartialEq> PartialEq for KeyedMap<T> {
    /// Java `AbstractMap.equals`：大小相同且每个键映射到的值相等，与顺序无关。
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .all(|(k, v)| other.get(k).is_some_and(|o| o == v))
    }
}

#[allow(dead_code)]
impl<T> KeyedMap<T> {
    pub fn new() -> Self {
        Self::default()
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        match &self.index {
            Some(map) => map.get(key).copied(),
            None => self.entries.iter().position(|(k, _)| k == key),
        }
    }

    fn rebuild_index(&mut self) {
        self.index = Some(
            self.entries.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect(),
        );
    }

    /// 追加新键后维护索引：已有索引则登记新下标，刚越过阈值则整体建一次。
    fn note_appended(&mut self) {
        let idx = self.entries.len() - 1;
        match &mut self.index {
            Some(map) => {
                map.insert(self.entries[idx].0.clone(), idx);
            }
            None if self.entries.len() >= INDEX_THRESHOLD => self.rebuild_index(),
            None => {}
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.index_of(key).is_some()
    }

    pub fn get(&self, key: &str) -> Option<&T> {
        self.index_of(key).map(|i| &self.entries[i].1)
    }

    /// `LinkedHashMap.remove`：删除并返回旧值。
    pub fn remove(&mut self, key: &str) -> Option<T> {
        let i = self.index_of(key)?;
        let value = self.entries.remove(i).1;
        // remove 使其后所有下标左移，索引整体重建（删除是冷路径）。
        if self.index.is_some() {
            self.rebuild_index();
        }
        Some(value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut T> {
        self.index_of(key).map(|i| &mut self.entries[i].1)
    }

    /// `LinkedHashMap.put`：已存在则原地替换（保留位置），否则追加。返回旧值。
    pub fn put(&mut self, key: impl Into<String>, value: T) -> Option<T> {
        let key = key.into();
        match self.index_of(&key) {
            Some(i) => Some(std::mem::replace(&mut self.entries[i].1, value)),
            None => {
                self.entries.push((key, value));
                self.note_appended();
                None
            }
        }
    }

    /// `LinkedHashMap.putIfAbsent`（忽略返回值语义）：仅当键不存在时插入。
    pub fn put_if_absent(&mut self, key: impl Into<String>, value: T) {
        let key = key.into();
        if self.index_of(&key).is_none() {
            self.entries.push((key, value));
            self.note_appended();
        }
    }

    /// `LinkedHashMap.computeIfAbsent`：不存在则用 `f` 构造并插入，返回可变引用。
    pub fn entry_or_insert_with(&mut self, key: impl Into<String>, f: impl FnOnce() -> T) -> &mut T {
        let key = key.into();
        if let Some(i) = self.index_of(&key) {
            return &mut self.entries[i].1;
        }
        self.entries.push((key, f()));
        self.note_appended();
        &mut self.entries.last_mut().unwrap().1
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<T: Clone> KeyedMap<T> {
    /// `LinkedHashMap.putAll`：按 `other` 的顺序，对每个条目做 `put`（覆盖保位 / 追加）。
    pub fn put_all_from(&mut self, other: &KeyedMap<T>) {
        for (k, v) in other.iter() {
            self.put(k.clone(), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with(n: usize) -> KeyedMap<usize> {
        let mut m = KeyedMap::new();
        for i in 0..n {
            m.put(format!("k{i}"), i);
        }
        m
    }

    #[test]
    fn indexed_put_keeps_position_and_returns_old() {
        let mut m = map_with(INDEX_THRESHOLD + 5);
        assert_eq!(m.put("k3", 999), Some(3));
        assert_eq!(m.keys().nth(3).unwrap(), "k3");
        assert_eq!(m.get("k3"), Some(&999));
        assert_eq!(m.put("fresh", 1), None);
        assert_eq!(m.keys().last().unwrap(), "fresh");
    }

    #[test]
    fn indexed_entry_and_remove_stay_correct() {
        let mut m = map_with(INDEX_THRESHOLD + 5);
        *m.entry_or_insert_with("k2", || 0) += 100;
        assert_eq!(m.get("k2"), Some(&102));
        m.entry_or_insert_with("brand-new", || 7);
        assert_eq!(m.get("brand-new"), Some(&7));
        assert_eq!(m.remove("k0"), Some(0));
        assert_eq!(m.keys().next().unwrap(), "k1");
        for i in 1..INDEX_THRESHOLD + 4 {
            m.remove(&format!("k{i}"));
        }
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&format!("k{}", INDEX_THRESHOLD + 4)), Some(&(INDEX_THRESHOLD + 4)));
        assert_eq!(m.get("brand-new"), Some(&7));
    }

    #[test]
    fn equal_content_equal_across_construction_paths() {
        let a = map_with(INDEX_THRESHOLD);
        let mut b = map_with(INDEX_THRESHOLD + 1);
        b.remove(&format!("k{}", INDEX_THRESHOLD));
        assert_eq!(a, b);
        assert_eq!(a.clone(), b);
    }
}
