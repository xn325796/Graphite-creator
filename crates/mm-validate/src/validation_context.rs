//! `ValidationContext.java` 的等价移植：一次校验共用的一份只读状态——工程、schema 注册表、
//! 以及累积下来的发现。
//!
//! 对应 Java `ValidationContext.java:12-19` 的类注释：几个规则都要读的派生值（内部名、工程贴图
//! 产出的图集区域）在这里算一次，而不是每个规则各算一遍。
//!
//! Java `ValidationContext.java:16-18` 那句「**The context is not thread-safe**; a pass owns one
//! instance, and `Validator` never shares it」照搬：context 不是线程安全的，一轮校验独占一个实例，
//! `Validator` 从不共享它。移植后这条约束依旧成立，理由也依旧——惰性缓存和普通字段一样没有任何同步。
//!
//! 与 Java 的语言差异：
//! - Java 的字段 `ModProject project` / `SchemaRegistry registry` 存的是**引用**（`Validator.java`
//!   持有它们，context 只是共享同一份对象，从不拷贝）。Rust 用生命周期 `'a` 把这件事写进类型：
//!   `ValidationContext<'a>` 借出 `'a` 的工程与注册表。好处是 [`ValidationContext::project`]
//!   交出的 `&'a ModProject` 并**不**挂在 `&self` 上，借用随 `'a` 而非随本次调用存续，于是 Java
//!   里那种「先取 project、边遍历边 report」的写法在这里照样成立。
//! - Java 的惰性缓存字段 `Set<String> spriteRegions`（`ValidationContext.java:25`、`60-65` 的
//!   `== null` 判空）对应 `OnceLock<Vec<String>>`：同样只在首次使用时算一次，
//!   同样不需要 `&mut`（Java 的 `spriteRegions()` 也是非 `synchronized` 的读方法）。之所以不用
//!   `Option<Vec<String>>` + `&mut self`：那会把可空性变成方法签名上的可变借用，Java 那个只读
//!   视图的调用形态就保不住了。

use std::collections::HashMap;
use std::sync::OnceLock;

use mm_model::ModProject;
use mm_schema::SchemaRegistry;

use crate::issue::Issue;

/// Java `final class ValidationContext`（`ValidationContext.java:20`）。
pub struct ValidationContext<'a> {
    /// Java `ValidationContext.java:22` 的 `private final ModProject project`。
    project: &'a ModProject,
    /// Java `ValidationContext.java:23` 的 `private final SchemaRegistry registry`。
    registry: &'a SchemaRegistry,
    /// Java `ValidationContext.java:24` 的 `private final List<Issue> issues = new ArrayList<>()`。
    issues: Vec<Issue>,
    /// Java `ValidationContext.java:25` 的 `private Set<String> spriteRegions`（惰性、可为 null）。
    ///
    /// Java 的 `Set<String>` 由 `SpriteRegions.regionsOf` 返回（那边是保序去重的集合：既去重又保持
    /// 首次出现顺序），Rust 侧 `mm_fmt::sprite::regions_of` 已经把它移植成同样去重、同样保序的
    /// `Vec<String>`，所以这里缓存的是 `Vec<String>`。
    sprite_regions: OnceLock<Vec<String>>,
    /// 名字 → 非内联记录在 `project.contents` 里的下标（保原顺序），首次使用时建一次。
    /// 引用校验对每个引用名都要按名字找记录，不建索引则每个名字全量扫一遍内容表。
    content_by_name: OnceLock<HashMap<String, Vec<usize>>>,
}

impl<'a> ValidationContext<'a> {
    /// 对应 Java `ValidationContext.java:27-30` 的构造器。
    pub fn new(project: &'a ModProject, registry: &'a SchemaRegistry) -> Self {
        ValidationContext {
            project,
            registry,
            issues: Vec::new(),
            sprite_regions: OnceLock::new(),
            content_by_name: OnceLock::new(),
        }
    }

    /// 对应 Java `ValidationContext.java:32-34` 的 `project()`。
    pub fn project(&self) -> &'a ModProject {
        self.project
    }

    /// 对应 Java `ValidationContext.java:36-38` 的 `registry()`。
    pub fn registry(&self) -> &'a SchemaRegistry {
        self.registry
    }

    /// 对应 Java `ValidationContext.java:40-43` 的 `internalName()`。
    ///
    /// 本模组每个内容名、贴图区域和引用都带着的前缀。Java 的 `project.internalName()` 每次现算，
    /// Rust 侧 `ModProject::internal_name()` 同样现算并交出一个 `String`。
    pub fn internal_name(&self) -> String {
        self.project.internal_name()
    }

    /// 对应 Java `ValidationContext.java:45-48` 的 `report(Issue issue)`。
    ///
    /// 记下一条发现；规则从不自己拼结果表。
    pub fn report(&mut self, issue: Issue) {
        self.issues.push(issue);
    }

    /// 对应 Java `ValidationContext.java:50-52` 的 `reportAll(List<Issue> findings)`。
    ///
    /// Java 的 `List<Issue>` ↔ Rust 的 `Vec<Issue>`：`issues.addAll(findings)` 把来表按顺序整段
    /// 追加进结果表，这里 `extend` 同样按顺序追加。唯一差异在所有权：Java 追加的是元素引用、调用方
    /// 自己的 findings 列表还在；Rust 直接把元素本身搬走，调用方此后不再持有这批 Issue。Java 侧传
    /// 进来的都是规则当场构造的临时列表，搬走与「复制引用后丢弃临时列表」在观察效果上没有区别。
    pub fn report_all(&mut self, findings: Vec<Issue>) {
        self.issues.extend(findings);
    }

    /// 对应 Java `ValidationContext.java:54-57` 的 `issues()`。
    ///
    /// 到目前为止的发现，按规则运行的顺序。Java 返回 `Collections.unmodifiableList(issues)`，
    /// Rust 交出不具写权的 `&[Issue]` 切片视图，同样是「只读、随源表变化」。
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    /// 对应 Java `ValidationContext.java:59-65` 的 `spriteRegions()`。
    ///
    /// 本工程的贴图资源实际产出的图集区域，首次使用时才算。
    ///
    /// Java 交出的就是缓存里那个 `Set` 本身（调用方只读）；Rust 交出它的只读切片视图
    /// `&[String]`。该视图挂在 `&self` 上，因此规则若要「一边迭代区域一边 report」，需要先把要用的
    /// 那几项取出来（`to_vec()` 等）——Java 无需这一步，因为那边没有借用检查器。
    pub fn sprite_regions(&self) -> &[String] {
        self.sprite_regions.get_or_init(|| {
            mm_fmt::sprite::regions_of(self.project, &self.internal_name())
        })
    }

    /// 名字精确等于 `name` 的全部非内联记录的下标（指向 [`Self::project`] 的 `contents`，
    /// 按内容表原顺序）。名字不存在时交空切片。与 sprite_regions 同一惰性缓存口径。
    pub fn content_indices_by_name(&self, name: &str) -> &[usize] {
        let map = self.content_by_name.get_or_init(|| {
            let mut map: HashMap<String, Vec<usize>> = HashMap::new();
            for (i, record) in self.project.contents.iter().enumerate() {
                if !record.is_inline() {
                    map.entry(record.name.clone()).or_default().push(i);
                }
            }
            map
        });
        map.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }
}
