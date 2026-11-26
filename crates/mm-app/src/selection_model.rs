//! `ui/SelectionModel.java`（43 行）的等价移植：当前选中的那一条内容记录。
//!
//! 与 Java 的唯一建模差异（P5 计划 §三，已签字）：**Rust 侧不持监听器**。
//! Java `:18` 的 `List<Consumer<ContentRecord>>` 是在 GUI 进程内直接回调面板；而决策 D5 要求
//! 核心逻辑零 tauri，所以这里把 `select`/`clear` 改成**返回「要广播的 payload」**，广播本身
//! 由外层做：生产路径是 `src-tauri` 里 `app.emit("selection-changed", …)`，前端
//! `listen("selection-changed", …)` 注册监听器——Java `onChange`(:40-42) 的等价物因此天然落在
//! 前端，不需要在 Rust 侧存回调。
//!
//! 那条不对称语义原样保留，且是 Java 特意写注释强调的：
//! * `select`(:25-29) **无条件**通知，注释「even when the record did not change」——面板每次
//!   显式选中都重读记录（`SelectionModelTest.java:39` 断言它）；
//! * `clear`(:31-37) **静默**，注释（:32-34）：项目正被整体替换时，让监听器去渲染一条已经不
//!   存在的项目里的记录是错的（`SelectionModelTest.java:51` 断言它）。
//!
//! `select` 的入参在 M4（2026-11-14）从 `ContentRecord` 改成 `Option<ContentRecord>`：
//! Java 的调用方有两类——`controller.select(record)` 与 **`controller.select(null)`**
//! （`NodeCanvasPane.java:1189`、`:1383` 用它取消选择）。`:26-29` 的 notify 不检查 null，
//! 所以那条事件照发，载荷是 `null`。这是 M1 移植时漏掉的一支：当时只看到「选中一条记录」，
//! 没把 null 那支当作模型语义。壳（`src-tauri`）的 `selection-changed` 要如实转发它，
//! 所以这里补上，登记在方案文档 M4 签字条目。

use mm_model::ContentRecord;

/// 对应 Java `SelectionModel`(:16)：`selected`(:19) 一个字段。
#[derive(Default)]
pub struct SelectionModel {
    selected: Option<ContentRecord>,
}

impl SelectionModel {
    /// 对应 Java `new SelectionModel()`：`selected` 初始为 `null`（Rust `None`）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 对应 Java `selected()`(:21-23)：可空，未选中返回 `None`（Java 返回 `null`）。
    pub fn selected(&self) -> Option<&ContentRecord> {
        self.selected.as_ref()
    }

    /// 对应 Java `select(ContentRecord)`(:26-29)：先存，再**无条件**给出要广播的记录。
    ///
    /// 返回的是模型内存储本体的 `&ContentRecord`，不是副本——于是「广播的记录」与
    /// 「`selected()` 读到的记录」在类型层面必然同为一个，这正是 Java
    /// `assertEquals(record, model.selected())`（`SelectionModelTest.java:26`，断引用同一性，
    /// 因为 `ContentRecord` 没重写 `equals`）想要保证的那件事。
    ///
    /// `select(None)` 就是 Java 的 `select(null)`：存空并照样给出「要发」这个信号，
    /// 此时返回 `None`，即事件的 `null` 载荷。**发不发**由调用点用的是哪条方法决定
    /// （`select` 必发、[`Self::clear`] 不发），不看这个返回值。
    pub fn select(&mut self, record: Option<ContentRecord>) -> Option<&ContentRecord> {
        self.selected = record;
        self.selected.as_ref()
    }

    /// 对应 Java `clear()`(:35-37)：忘记选中，且**静默**——所以没有返回值，也就没有
    /// 「要广播的 payload」可给。Java 那边是「不调用 notify」，Rust 侧用「不给返回值」
    /// 表达同一件事；这正是它与 `select(None)` 的全部区别。
    pub fn clear(&mut self) {
        self.selected = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_model::ContentType;

    /// 对应 Java 夹具 `SelectionModelTest.seen`(:17) + `model.onChange(seen::add)`(:21)。
    /// Rust 侧模型不持监听器，所以「收到广播」在测试里就是把 `select` 返回的 payload 收进
    /// `seen`——这一步在生产路径上换成 `src-tauri` 的 `emit`。
    /// 载荷类型收成 `Option`：Java 的 `Consumer.accept(null)` 同样是「收到了一条事件」。
    fn broadcast(
        model: &mut SelectionModel,
        seen: &mut Vec<Option<ContentRecord>>,
        record: Option<ContentRecord>,
    ) {
        seen.push(model.select(record).cloned());
    }

    /// 对应 Java `ContentRecord.create("c1", ContentType.item, "thing")`
    /// （`SelectionModelTest.java:22`；`ContentType` 在 Java 是 `core.fmt.ContentType`）。
    fn record() -> ContentRecord {
        ContentRecord::create("c1", ContentType::Item, "thing")
    }

    /// 对应 Java `SelectionModelTest.java:19-28` `selectPublishesTheRecordAndNotifies`。
    #[test]
    fn select_publishes_the_record_and_notifies() {
        let mut model = SelectionModel::new();
        let mut seen: Vec<Option<ContentRecord>> = Vec::new();
        let record = record();

        broadcast(&mut model, &mut seen, Some(record.clone()));

        // Java :26 assertEquals(record, model.selected())：断「发布的就是传进去的那条记录」。
        let stored = model.selected().expect("选中记录已发布");
        assert_eq!(stored.id, Some(String::from("c1")));
        assert_eq!(stored.name, "thing");
        assert_eq!(stored.ctype, ContentType::Item);
        // Java :27 assertEquals(List.of(record), seen)：一条广播，且就是那条记录。
        assert_eq!(seen.len(), 1);
        let payload = seen[0].as_ref().expect("select 必发一条带记录的广播");
        assert_eq!(payload.id, record.id);
        assert_eq!(payload.name, record.name);
    }

    /// 对应 Java `SelectionModelTest.java:30-40` `selectingTheSameRecordAgainStillNotifies`。
    #[test]
    fn selecting_the_same_record_again_still_notifies() {
        let mut model = SelectionModel::new();
        let mut seen: Vec<Option<ContentRecord>> = Vec::new();
        let record = record();

        // Java :33：此时还没注册监听器，所以这一次 select 不产生任何广播。
        model.select(Some(record.clone()));
        seen.clear();

        // Java :37 再选**同一条**记录一次。Rust 的值语义下用同一条记录的副本表达「记录没变」，
        // 通知规则不受影响。
        broadcast(&mut model, &mut seen, Some(record.clone()));

        assert_eq!(seen.len(), 1, "面板每次显式 select 都要重读记录"); // Java :39
        let payload = seen[0].as_ref().expect("广播载荷就是那条记录");
        assert_eq!(payload.id, record.id);
        assert_eq!(payload.name, record.name);
    }

    /// 对应 Java `SelectionModelTest.java:42-52`
    /// `clearingForgetsTheSelectionWithoutTellingThePanes`。
    #[test]
    fn clearing_forgets_the_selection_without_telling_the_panes() {
        let mut model = SelectionModel::new();
        let mut seen: Vec<Option<ContentRecord>> = Vec::new();

        broadcast(&mut model, &mut seen, Some(record()));
        seen.clear();

        model.clear();

        assert!(model.selected().is_none()); // Java :50 assertNull(model.selected())
        assert!(seen.is_empty(), "项目被整体替换时，不能让面板去渲染一条已经死掉的记录"); // Java :51
    }

    /// M4 新增：`select(null)` 那一支（Java 没有单元测试，依据是
    /// `SelectionModel.java:26-29` 的 notify 不检查 null，调用点见 `NodeCanvasPane.java:1189`）。
    /// 壳的 `selection-changed` 必须能收到这条 `null` 载荷的事件，否则取消选择在前端是静默的。
    #[test]
    fn selecting_null_still_broadcasts_a_null_payload() {
        let mut model = SelectionModel::new();
        let mut seen: Vec<Option<ContentRecord>> = Vec::new();

        broadcast(&mut model, &mut seen, Some(record()));
        broadcast(&mut model, &mut seen, None);

        assert_eq!(seen.len(), 2, "取消选择同样是一条事件，不像 clear 那样静默");
        assert!(seen[1].is_none(), "载荷为 null");
        assert!(model.selected().is_none());
    }
}
