//! `TextTemplate.java` 的等价移植：填充 `{0}`、`{1}` 占位符，不带 MessageFormat 的引号处理。
//!
//! MessageFormat 把单引号视为引用字面量的开始，于是 "the game's maximum" 这样的普通英文句子会被
//! 渲染成 "the games maximum"。本项目所有模式都只用简单的 `{n}` 形式，因此直接替换足够、可预测，
//! 并让校验器渲染出的英文回退与 UI 显示的逐字节一致。

use std::fmt::Display;

pub fn fill<T: Display>(pattern: &str, args: &[T]) -> String {
    if args.is_empty() {
        return pattern.to_string();
    }
    let mut out = pattern.to_string();
    for (i, value) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), &value.to_string());
    }
    out
}

/// Java 侧 `List<?>` 里可能放 `null`（渲染成空串）时的字符串入口。
pub fn fill_str(pattern: &str, args: &[&str]) -> String {
    if args.is_empty() {
        return pattern.to_string();
    }
    let mut out = pattern.to_string();
    for (i, value) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_positional_placeholders() {
        assert_eq!(fill_str("a {0} b {1} c", &["x", "y"]), "a x b y c");
    }

    #[test]
    fn no_args_returns_the_pattern_unchanged() {
        assert_eq!(fill_str("{0} stays", &[]), "{0} stays");
    }

    #[test]
    fn keeps_apostrophes_that_messageformat_would_eat() {
        assert_eq!(fill_str("the game's maximum is {0}", &["10"]), "the game's maximum is 10");
    }

    #[test]
    fn repeated_placeholder_is_filled_everywhere() {
        assert_eq!(fill_str("{0}/{0}", &["k"]), "k/k");
    }
}
