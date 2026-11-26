//! A JSON tree that preserves object key order and the exact text of numbers.
//!
//! Mirrors the Gson `JsonElement` family the Java core uses. Order matters because
//! [`crate::writer`] re-emits members in insertion order, and number text matters because the
//! game keeps literals like `0.50` or values beyond `i64` verbatim (`JsonPrimitive.getAsString`
//! returns the source digits, not a normalised double).

use std::collections::HashMap;
use std::fmt;

/// One JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    /// A number kept as its exact source spelling (`"0.50"`, `"123456"`, `"1e9"`).
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Object),
}

/// 超过这个键数才建哈希索引：小对象线性扫比哈希更快，也省一次 HashMap 分配；
/// 大表（如数千键的 bundle 字符串表）不建索引则 insert/get 是 O(n²)。
const INDEX_THRESHOLD: usize = 32;

/// An object with insertion-ordered members, matching Gson's `JsonObject` (a `LinkedTreeMap`).
#[derive(Clone, Default)]
pub struct Object {
    entries: Vec<(String, Json)>,
    /// 键 → entries 下标。纯派生数据：相等性与 Debug 都只看 entries，
    /// 两条不同构造路径（如越过阈值后 remove 回落）得到的同内容对象必须仍相等。
    index: Option<HashMap<String, usize>>,
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Eq for Object {}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Object").field("entries", &self.entries).finish()
    }
}

impl Object {
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

    /// Adds a member. Re-adding an existing key replaces its value but keeps its position,
    /// exactly like `JsonObject.add`.
    pub fn insert(&mut self, key: impl Into<String>, value: Json) {
        let key = key.into();
        if let Some(idx) = self.index_of(&key) {
            self.entries[idx].1 = value;
        } else {
            self.entries.push((key, value));
            self.note_appended();
        }
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        self.index_of(key).map(|i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.index_of(key).map(|i| &mut self.entries[i].1)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.index_of(key).is_some()
    }

    pub fn remove(&mut self, key: &str) -> Option<Json> {
        let idx = self.index_of(key)?;
        let value = self.entries.remove(idx).1;
        // remove 使其后所有下标左移，索引整体重建（删除是冷路径）。
        if self.index.is_some() {
            self.rebuild_index();
        }
        Some(value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Json)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl Json {
    pub fn object() -> Json {
        Json::Obj(Object::new())
    }

    pub fn num(text: impl Into<String>) -> Json {
        Json::Num(text.into())
    }

    pub fn str(text: impl Into<String>) -> Json {
        Json::Str(text.into())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Json::Bool(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Json::Num(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Json::Str(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Json::Arr(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Json::Obj(_))
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Json>> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Json::Obj(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Json::Obj(o) => Some(o),
            _ => None,
        }
    }

    /// The number's source spelling, or `None` for a non-number.
    pub fn as_number(&self) -> Option<&str> {
        match self {
            Json::Num(s) => Some(s),
            _ => None,
        }
    }

    /// Equivalent of Gson `JsonPrimitive.getAsString`: `null`/`"true"`/`"false"` for the literal
    /// cases, the stored digits for a number, the text for a string. Panics on an array/object
    /// like Gson does.
    pub fn get_as_string(&self) -> String {
        match self {
            Json::Null => "null".to_string(),
            Json::Bool(true) => "true".to_string(),
            Json::Bool(false) => "false".to_string(),
            Json::Num(s) => s.clone(),
            Json::Str(s) => s.clone(),
            Json::Arr(_) | Json::Obj(_) => {
                panic!("cannot call getAsString on a composite value")
            }
        }
    }

    /// `getAsDouble` — parses a number or numeric string.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(s) | Json::Str(s) => s.parse::<f64>().ok(),
            Json::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// `getAsInt`/`getAsLong` — Gson `LazilyParsedNumber` 的级联口径：先精确整数解析，
    /// 失败才回落 double 截断。只走 f64 会让 >2^53 的整数失真（Gson 的 Long.parseLong
    /// 那一支是精确的）；小数仍按 Gson 的 `(int) Double.parseDouble` 截断。
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(s) | Json::Str(s) => s
                .parse::<i64>()
                .ok()
                .or_else(|| s.parse::<f64>().ok().map(|v| v as i64)),
            Json::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn as_i64_exact(&self) -> Option<i64> {
        match self {
            Json::Num(s) | Json::Str(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Num(s) => f.write_str(s),
            Json::Str(s) => f.write_str(s),
            Json::Arr(a) => write!(f, "array[{}]", a.len()),
            Json::Obj(o) => write!(f, "object[{}]", o.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj_with(n: usize) -> Object {
        let mut o = Object::new();
        for i in 0..n {
            o.insert(format!("k{i}"), Json::num(i.to_string()));
        }
        o
    }

    #[test]
    fn indexed_insert_keeps_position_and_replaces() {
        let mut o = obj_with(INDEX_THRESHOLD + 5);
        o.insert("k3", Json::str("new"));
        assert_eq!(o.keys().nth(3).unwrap(), "k3");
        assert_eq!(o.get("k3"), Some(&Json::str("new")));
        assert_eq!(o.len(), INDEX_THRESHOLD + 5);
        o.insert("fresh", Json::Null);
        assert_eq!(o.get("fresh"), Some(&Json::Null));
        assert_eq!(o.keys().last().unwrap(), "fresh");
    }

    #[test]
    fn indexed_remove_shifts_and_stays_correct() {
        let mut o = obj_with(INDEX_THRESHOLD + 5);
        assert_eq!(o.remove("k0"), Some(Json::num("0")));
        assert_eq!(o.get("k1"), Some(&Json::num("1")));
        assert_eq!(o.keys().next().unwrap(), "k1");
        assert!(!o.contains("k0"));
        // 一路删到阈值以下，索引仍须正确。
        for i in 1..INDEX_THRESHOLD + 4 {
            o.remove(&format!("k{i}"));
        }
        assert_eq!(o.len(), 1);
        assert_eq!(o.get(&format!("k{}", INDEX_THRESHOLD + 4)), Some(&Json::num((INDEX_THRESHOLD + 4).to_string())));
    }

    #[test]
    fn equal_content_equal_across_construction_paths() {
        // 直接建到阈值 vs 越过阈值再删回来：索引存在与否不得影响相等性。
        let a = obj_with(INDEX_THRESHOLD);
        let mut b = obj_with(INDEX_THRESHOLD + 1);
        b.remove(&format!("k{}", INDEX_THRESHOLD));
        assert_eq!(a, b);
        assert_eq!(a.clone(), b);
    }

    #[test]
    fn as_i64_keeps_big_integers_exact_and_truncates_decimals() {
        // >2^53：f64 会失真，精确解析支必须接住（Gson Long.parseLong 口径）。
        assert_eq!(Json::num("9007199254740993").as_i64(), Some(9007199254740993));
        assert_eq!(Json::num("2.7").as_i64(), Some(2));
        assert_eq!(Json::str("42").as_i64(), Some(42));
        assert_eq!(Json::num("abc").as_i64(), None);
    }
}
