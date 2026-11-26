//! `Ids.java` 的等价移植：项目记录、节点与边的短小、相对稳定标识符。

use std::sync::atomic::{AtomicI64, Ordering};

static COUNTER: AtomicI64 = AtomicI64::new(0);

/// Java `System.nanoTime()` 的对应物：某个任意原点的单调纳秒数。
fn nanos() -> i64 {
    std::time::UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Java `Long.toString(v, 36)`：负数带前导 `-`，0 输出 "0"。
fn base36(mut v: i64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let negative = v < 0;
    if negative {
        v = -v;
    }
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(DIGITS[(v % 36) as usize]);
        v /= 36;
    }
    if negative {
        buf.push(b'-');
    }
    buf.reverse();
    String::from_utf8(buf).expect("base36 digits are ASCII")
}

/// `prefix + nanoTime(36) + counter(36)`。
pub fn uid(prefix: &str) -> String {
    let mut out = String::from(prefix);
    out.push_str(&base36(nanos()));
    out.push_str(&base36(COUNTER.fetch_add(1, Ordering::Relaxed) + 1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_are_prefixed_and_distinct() {
        let a = uid("n-");
        let b = uid("n-");
        assert!(a.starts_with("n-"));
        assert_ne!(a, b);
    }

    #[test]
    fn base36_matches_java_long_to_string() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(9), "9");
        assert_eq!(base36(10), "a");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(1295), "zz");
        assert_eq!(base36(-36), "-10");
    }
}
