//! `SchemaRegistry` 的等价移植：加载并查询 `schemas/` 里的类/字段知识。
//!
//! 两层来源：
//! 1. `schemas/fields.json`（由 Java 源树生成）——类名、父类、内容类型、每个字段的原始类型。
//! 2. `schemas/schema.json`（手工精选）——按字段覆盖生成层，提供控件 kind、枚举值、默认值。
//!
//! 此外 `schemas/vanilla-content.json` 记录游戏自带内容的名字，用于补丁识别与引用校验。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mm_json::{Json, Object};

use crate::{
    class_schema::ClassSchema, content_type::ContentType, field::FieldDef,
    field_type::FieldType, map::KeyedMap,
};

/// schema 加载错误。
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
    #[error("cannot parse {path}: {message}")]
    Parse { path: String, message: String },
}

/// `stackKindOf(rawType)`：冒号后的子串（"struct:unitReq" → "unitReq"），无冒号为 None。
fn stack_kind_of(raw_type: Option<&str>) -> Option<String> {
    let v = raw_type?;
    let idx = v.find(':')?;
    Some(v[idx + 1..].to_string())
}

/// 把 JSON 字符串数组读成 `Vec<String>`；非数组/缺失 → 空。
fn json_string_array(j: Option<&Json>) -> Vec<String> {
    match j.and_then(|x| x.as_array()) {
        Some(arr) => arr.iter().filter_map(|el| el.as_str().map(|s| s.to_string())).collect(),
        None => Vec::new(),
    }
}

/// 读一个 schema 文件（UTF-8 + 去 BOM + 容错读为对象），等价于 Java `Json.parseObject(Json.readFile(..))`。
fn read_object_file(path: &Path) -> Result<Object, SchemaError> {
    let text = std::fs::read_to_string(path).map_err(|e| SchemaError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    mm_json::read_object(&text).map_err(|e| SchemaError::Parse {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

/// 类与字段的知识注册表。
pub struct SchemaRegistry {
    classes: KeyedMap<ClassSchema>,
    vanilla: Vec<(ContentType, Vec<String>)>,
    /// 与 `vanilla` 同内容的集合视图，供 is_vanilla_name 做 O(1) 判定
    /// （引用校验对每个引用名×每个类型都要问一次）。
    vanilla_sets: Vec<(ContentType, HashSet<String>)>,
    unit_types: Vec<String>,
    /// fields_of 的加载期缓存：注册表加载后不再变，继承链合并只需算一次。
    /// 校验/重写/卡片几何对每条记录都调 fields_of，不缓存则每次重复合并+深克隆。
    fields_cache: KeyedMap<Rc<Vec<FieldDef>>>,
}

impl SchemaRegistry {
    fn new() -> Self {
        Self {
            classes: KeyedMap::new(),
            vanilla: Vec::new(),
            vanilla_sets: Vec::new(),
            unit_types: Vec::new(),
            fields_cache: KeyedMap::new(),
        }
    }

    /// 从给定目录加载三层数据（缺文件则跳过对应层）。
    pub fn load(dir: &Path) -> Result<Self, SchemaError> {
        let mut reg = Self::new();

        let generated = dir.join("fields.json");
        if generated.exists() {
            reg.load_generated(&read_object_file(&generated)?);
        }
        let curated = dir.join("schema.json");
        if curated.exists() {
            reg.load_curated(&read_object_file(&curated)?);
        }
        let vanilla = dir.join("vanilla-content.json");
        if vanilla.exists() {
            reg.load_vanilla(&read_object_file(&vanilla)?);
        }
        let names = reg.class_names();
        for name in names {
            let fields = Rc::new(reg.compute_fields_of(&name));
            reg.fields_cache.put(name, fields);
        }
        Ok(reg)
    }

    /// 定位 `schemas` 目录：`MODMAKER_SCHEMAS` → `./schemas` → `../schemas`。
    pub fn load_default() -> Result<Self, SchemaError> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("MODMAKER_SCHEMAS") {
            candidates.push(PathBuf::from(p));
        }
        candidates.push(PathBuf::from("./schemas"));
        candidates.push(PathBuf::from("../schemas"));

        for dir in candidates {
            if dir.join("fields.json").exists() || dir.join("schema.json").exists() {
                return Self::load(&dir);
            }
        }
        Err(SchemaError::Io {
            path: "schemas".to_string(),
            message: "no schemas directory found".to_string(),
        })
    }

    fn load_generated(&mut self, root: &Object) {
        if let Some(arr) = root.get("unitTypes").and_then(|j| j.as_array()) {
            for el in arr {
                if let Some(s) = el.as_str()
                    && !self.unit_types.iter().any(|x| x == s)
                {
                    self.unit_types.push(s.to_string());
                }
            }
        }

        let classes = match root.get("classes").and_then(|j| j.as_object()) {
            Some(c) => c,
            None => return,
        };
        for (name, value) in classes.iter() {
            let obj = match value.as_object() {
                Some(o) => o,
                None => continue,
            };
            let schema = self
                .classes
                .entry_or_insert_with(name.clone(), || ClassSchema::new(name.clone()));

            let extends: Option<&Json> = obj.get("extends").filter(|j| !j.is_null());
            if schema.hierarchy_parent.is_none() && schema.parent.is_none()
                && let Some(v) = extends.and_then(|e| e.as_str())
            {
                schema.parent = Some(v.to_string());
            }
            if schema.hierarchy_parent.is_none()
                && let Some(v) = extends.and_then(|e| e.as_str())
            {
                schema.hierarchy_parent = Some(v.to_string());
            }

            if schema.ctype.is_none()
                && let Some(c) = obj.get("ctype").and_then(|j| j.as_str())
            {
                schema.ctype = ContentType::from_name(c);
            }

            if let Some(fields) = obj.get("fields").and_then(|j| j.as_array()) {
                for f in fields {
                    let fo = match f.as_object() {
                        Some(o) => o,
                        None => continue,
                    };
                    let n = fo
                        .get("n")
                        .and_then(|j| j.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let raw_type = fo.get("t").and_then(|j| j.as_str());
                    let ft = FieldType::parse(raw_type);
                    let enum_values = json_string_array(fo.get("v"));
                    let content_ctype =
                        if ft == FieldType::Content { Some("any".to_string()) } else { None };
                    let stack_kind = stack_kind_of(raw_type);
                    schema.fields.put_if_absent(
                        n.clone(),
                        FieldDef::new(
                            n,
                            ft,
                            None,
                            enum_values,
                            content_ctype,
                            stack_kind,
                            None,
                            false,
                        ),
                    );
                }
            }
        }

        for schema in self.classes.values_mut() {
            if let Some(ct) = schema.ctype
                && schema.name == ct.default_class()
            {
                schema.base = true;
            }
        }
    }

    fn load_curated(&mut self, root: &Object) {
        let classes = match root.get("classes").and_then(|j| j.as_object()) {
            Some(c) => c,
            None => return,
        };
        for (name, value) in classes.iter() {
            let obj = match value.as_object() {
                Some(o) => o,
                None => continue,
            };
            let schema = self
                .classes
                .entry_or_insert_with(name.clone(), || ClassSchema::new(name.clone()));
            schema.curated = true;

            if let Some(c) = obj.get("ctype").and_then(|j| j.as_str()) {
                schema.ctype = ContentType::from_name(c);
            }
            if let Some(b) = obj.get("base").and_then(|j| j.as_bool()) {
                schema.base = b;
            }
            if let Some(e) = obj
                .get("extends")
                .filter(|j| !j.is_null())
                .and_then(|j| j.as_str())
            {
                schema.parent = Some(e.to_string());
            }

            if let Some(fields_obj) = obj.get("fields").and_then(|j| j.as_object()) {
                for (fname, fval) in fields_obj.iter() {
                    let fo = match fval.as_object() {
                        Some(o) => o,
                        None => continue,
                    };
                    let raw_type = fo.get("type").and_then(|j| j.as_str());
                    let ft = FieldType::parse(raw_type);
                    let description =
                        fo.get("desc").and_then(|j| j.as_str()).map(|s| s.to_string());
                    let enum_values = json_string_array(fo.get("enumValues"));
                    let content_ctype = match fo.get("contentCtype").and_then(|j| j.as_str()) {
                        Some(s) => Some(s.to_string()),
                        None => {
                            if ft == FieldType::Content {
                                Some("any".to_string())
                            } else {
                                None
                            }
                        }
                    };
                    let stack_kind = match fo.get("stackKind").and_then(|j| j.as_str()) {
                        Some(s) => Some(s.to_string()),
                        None => stack_kind_of(raw_type),
                    };
                    let default_value = fo.get("default").cloned();
                    schema.fields.put(
                        fname.clone(),
                        FieldDef::new(
                            fname.clone(),
                            ft,
                            description,
                            enum_values,
                            content_ctype,
                            stack_kind,
                            default_value,
                            true,
                        ),
                    );
                }
            }
        }
    }

    fn load_vanilla(&mut self, root: &Object) {
        let content = match root.get("content").and_then(|j| j.as_object()) {
            Some(c) => c,
            None => return,
        };
        for (key, value) in content.iter() {
            let ct = match ContentType::from_name(key) {
                Some(c) => c,
                None => continue,
            };
            let names = json_string_array(Some(value));
            self.vanilla_sets.push((ct, names.iter().cloned().collect()));
            self.vanilla.push((ct, names));
        }
    }

    /// 合并 `className` 自身与其全部祖先（沿 `trueParent()`）的字段，按名去重。
    ///
    /// `ItemTurret` 的精选 `extends: "Block"` 仅对表单是捷径；真实的 `→ Turret →
    /// ReloadTurret → BaseTurret` 链仍提供继承字段，因为 `trueParent()` 忽略精选覆盖。
    ///
    /// 结果在 [`Self::load`] 末尾按类缓存，这里只交共享引用：调用方对字段只读，
    /// 与原先每次交一份新克隆无可观察差异（reference_check.rs 模块头第 9 条同口径）。
    pub fn fields_of(&self, class_name: &str) -> Rc<Vec<FieldDef>> {
        match self.fields_cache.get(class_name) {
            Some(fields) => Rc::clone(fields),
            None => Rc::new(Vec::new()),
        }
    }

    fn compute_fields_of(&self, class_name: &str) -> Vec<FieldDef> {
        let mut merged = KeyedMap::new();
        let mut seen: Vec<String> = Vec::new();
        self.collect(class_name, &mut merged, &mut seen);
        merged.values().cloned().collect()
    }

    fn collect(&self, name: &str, merged: &mut KeyedMap<FieldDef>, seen: &mut Vec<String>) {
        let schema = match self.classes.get(name) {
            Some(s) => s,
            None => return,
        };
        if seen.iter().any(|x| x == name) {
            return;
        }
        seen.push(name.to_string());
        if let Some(parent) = schema.true_parent().map(|p| p.to_string()) {
            self.collect(&parent, merged, seen);
        }
        merged.put_all_from(&schema.fields);
    }

    pub fn schema(&self, class_name: &str) -> Option<&ClassSchema> {
        self.classes.get(class_name)
    }

    /// 已加载的类名，按插入顺序（保序）。
    pub fn class_names(&self) -> Vec<String> {
        self.classes.keys().cloned().collect()
    }

    pub fn is_curated(&self, class_name: &str) -> bool {
        self.classes.get(class_name).map(|s| s.curated).unwrap_or(false)
    }

    /// `className` 沿 `trueParent()` 的全部祖先，自身在前、根在后。
    fn ancestor_chain(&self, name: &str) -> Vec<String> {
        let mut chain: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut current: Option<String> = Some(name.to_string());
        while let Some(cur) = current {
            if seen.contains(&cur) {
                break;
            }
            seen.push(cur.clone());
            chain.push(cur.clone());
            current = match self.classes.get(&cur) {
                Some(s) => s.true_parent().map(|p| p.to_string()),
                None => None,
            };
        }
        chain
    }

    /// 某类型全部已声明类的名字，其基础类（"Block"）排第一，其余按字母序。
    pub fn classes_for_type(&self, ctype: ContentType) -> Vec<String> {
        let base = self.base_class_of(ctype);
        let mut names: Vec<String> = self
            .classes
            .values()
            .filter(|s| s.ctype == Some(ctype))
            .map(|s| s.name.clone())
            .collect();
        names.sort_by(|a, b| {
            let ka = Some(a.as_str()) != base.as_deref();
            let kb = Some(b.as_str()) != base.as_deref();
            ka.cmp(&kb).then_with(|| a.cmp(b))
        });
        names
    }

    /// 与 `current` 处于类型层次同一分支、因而编辑体验相同的兄弟类（含 `current` 的祖先与自身）。
    pub fn related_classes(&self, ctype: ContentType, current: Option<&str>) -> Vec<String> {
        let root = self.base_class_of(ctype);
        let root_str = root.as_deref();
        if current.is_none() || current == Some("") || current == root_str {
            return self.classes_for_type(ctype);
        }

        // 截断 `current` 的链：选中 Battery 时给表单 Battery→…→PowerState 而非每个 Drills。
        let mut chain: Vec<String> = Vec::new();
        for anc in self.ancestor_chain(current.unwrap()) {
            chain.push(anc.clone());
            if Some(anc.as_str()) == root_str {
                break;
            }
        }

        // out 初始化为 chain（LinkedHashSet），再把链上任一祖先等于某类祖先的类并入。
        let mut out: Vec<String> = Vec::new();
        for c in &chain {
            if !out.contains(c) {
                out.push(c.clone());
            }
        }
        for s in self.classes.values() {
            if s.ctype != Some(ctype) || out.iter().any(|x| x == &s.name) {
                continue;
            }
            if Self::meets_below_root(&self.ancestor_chain(&s.name), &chain, root_str)
                && !out.contains(&s.name)
            {
                out.push(s.name.clone());
            }
        }
        out
    }

    /// `candidateChain` 最近的已建模祖先是否严格低于 `root`（否则该类在另一分支或就是 root）。
    fn meets_below_root(
        candidate_chain: &[String],
        current_chain: &[String],
        root: Option<&str>,
    ) -> bool {
        for anc in candidate_chain {
            if current_chain.iter().any(|x| x == anc) {
                return Some(anc.as_str()) != root;
            }
        }
        false
    }

    /// 类型根类名（"Block"），schema 缺失时回退到默认类名；默认类名为空则 None。
    pub fn base_class_of(&self, ctype: ContentType) -> Option<String> {
        for s in self.classes.values() {
            if s.base && s.ctype == Some(ctype) {
                return Some(s.name.clone());
            }
        }
        let fallback = ctype.default_class();
        if fallback.is_empty() {
            None
        } else {
            Some(fallback.to_string())
        }
    }

    /// 把（可能手输的）"type" 值解析成可比较的 schema 类名。
    ///
    /// `UnitType` 是移动类型名而非类，对单位强制使用默认类，使 `resolveClass(unit,"Duo")` 与
    /// 省略 type 的 `resolveClass(unit,null)` 都映射到 UnitType（与 `CoreMap.units.get` 键一致）。
    pub fn resolve_class(&self, ctype: ContentType, declared: Option<&str>) -> Option<String> {
        if ctype == ContentType::Unit {
            let fallback = ctype.default_class();
            return if fallback.is_empty() {
                None
            } else {
                Some(fallback.to_string())
            };
        }
        if let Some(d) = declared
            && !d.trim().is_empty()
        {
            return Some(Self::canonical_class_name(d));
        }
        let fallback = ctype.default_class();
        if fallback.is_empty() {
            None
        } else {
            Some(fallback.to_string())
        }
    }

    pub fn unit_types(&self) -> &[String] {
        &self.unit_types
    }

    pub fn is_unit_type(&self, value: &str) -> bool {
        self.unit_types.iter().any(|x| x == value)
    }

    /// 把声明的类型串归一成 schema 类名（短名，首字母大写）。
    pub fn canonical_class_name(declared: &str) -> String {
        let simple = match declared.rfind('.') {
            Some(i) => &declared[i + 1..],
            None => declared,
        };
        let first = match simple.chars().next() {
            Some(c) => c,
            None => return simple.to_string(),
        };
        if first.is_lowercase() {
            let mut out = first.to_uppercase().to_string();
            out.push_str(&simple[first.len_utf8()..]);
            out
        } else {
            simple.to_string()
        }
    }

    /// 某类型游戏自带内容的名字（补丁识别、引用校验用）。
    pub fn vanilla_names(&self, ctype: ContentType) -> Vec<String> {
        self.vanilla
            .iter()
            .find(|(t, _)| *t == ctype)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    pub fn is_vanilla_name(&self, ctype: ContentType, name: &str) -> bool {
        let lower = name.to_lowercase();
        self.vanilla_sets
            .iter()
            .find(|(t, _)| *t == ctype)
            .map(|(_, set)| set.contains(&lower))
            .unwrap_or(false)
    }
}
