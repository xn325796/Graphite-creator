//! Locates the exact character span of a value inside a JSON/HJSON document and splices a
//! replacement in. Port of `core.json.JsonTextPatch`.
//!
//! Content files are hand-written HJSON: braceless or braced root, quoteless keys and strings,
//! optional commas, `//`, `#` and block comments. Re-serialising loses every comment and every bit
//! of alignment, so a change confined to one member is applied to the original bytes and everything
//! else is copied through untouched. This is a locator, not a parser: it finds where a value starts
//! and ends and skips strings/comments correctly, nothing more. When a path cannot be located
//! [`replace`] returns `None` and the caller falls back to re-serialising.

use crate::path::steps;
use crate::reader::{looks_like_number, read};
use crate::value::Json;

/// Half-open character range `[start, end)` into the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// The parsed tree of `text`, or `None` when it does not parse.
pub fn parse_or_null(text: &str) -> Option<Json> {
    read(text).ok()
}

/// Replaces the whole value at `path` with `replacement`, keeping every other character of `source`.
pub fn replace(source: &str, path: &str, replacement: &str) -> Option<String> {
    let chars: Vec<char> = source.chars().collect();
    let span = value_span_chars(&chars, path)?;
    let head: String = chars[..span.start].iter().collect();
    let tail: String = chars[span.end..].iter().collect();
    Some(head + &reindent(&chars, span, replacement) + &tail)
}

/// The span of the value at `path`, or `None` when it cannot be located. Port of `valueSpan`.
pub fn value_span(source: &str, path: &str) -> Option<Span> {
    let chars: Vec<char> = source.chars().collect();
    value_span_chars(&chars, path)
}

/// The exact source text of `span` (a half-open character range). Rust's `String` cannot be sliced
/// at a char index the way Java's `substring` can, so this reconstructs the located value — used by
/// callers and tests that need the bytes a span names.
pub fn span_text(source: &str, span: Span) -> String {
    source.chars().skip(span.start).take(span.end - span.start).collect()
}

fn value_span_chars(chars: &[char], path: &str) -> Option<Span> {
    let steps = steps(path);
    if steps.is_empty() {
        return None;
    }
    let length = chars.len();
    let mut start = if chars.first() == Some(&'\u{FEFF}') { 1 } else { 0 };
    start = skip_trivia(chars, start, length);
    if start >= length {
        return None;
    }
    // A braced root holds its members between the braces; a braceless one runs to the end.
    let mut container = if chars[start] == '{' {
        let after = skip_value(chars, start, length, false, 0);
        if after < 0 {
            return None;
        }
        Span { start: start + 1, end: after as usize - 1 }
    } else {
        Span { start, end: length }
    };
    let mut array = false;
    for (i, step) in steps.iter().enumerate() {
        // An index only exists in an array and a name only in an object; a path that gets that
        // wrong is not there, and guessing would splice into the wrong place.
        if step.is_index != array {
            return None;
        }
        let value = if step.is_index {
            find_element(chars, container, step.index)?
        } else {
            find_member(chars, container, step.key.as_deref().unwrap_or(""))?
        };
        if i == steps.len() - 1 {
            return Some(value);
        }
        let first = chars[value.start];
        if first != '{' && first != '[' {
            return None;
        }
        // Descend: the next step lives inside the value's braces/brackets, not around them.
        let after = skip_value(chars, value.start, length, false, 0);
        if after < 0 || after as usize - 1 <= value.start {
            return None;
        }
        container = Span { start: value.start + 1, end: after as usize - 1 };
        array = first == '[';
    }
    None
}

// --- locating ----------------------------------------------------------------------------------

fn find_member(chars: &[char], container: Span, key: &str) -> Option<Span> {
    let end = container.end;
    let mut p = container.start;
    loop {
        p = skip_trivia(chars, p, end);
        if p >= end || chars[p] == '}' {
            return None;
        }
        let key_end = skip_key(chars, p, end);
        if key_end < 0 {
            return None;
        }
        let key_end = key_end as usize;
        let found = unquote(&chars[p..key_end].iter().collect::<String>());
        p = skip_trivia(chars, key_end, end);
        if p >= end || chars[p] != ':' {
            return None;
        }
        p = skip_trivia(chars, p + 1, end);
        if p >= end {
            return None;
        }
        let value_end = skip_value(chars, p, end, false, 0);
        if value_end < 0 {
            return None;
        }
        let value_end = value_end as usize;
        if found == key {
            return Some(Span { start: p, end: value_end });
        }
        p = skip_trivia(chars, value_end, end);
        if p < end && chars[p] == ',' {
            p += 1;
        }
    }
}

fn find_element(chars: &[char], container: Span, index: usize) -> Option<Span> {
    let end = container.end;
    let mut p = container.start;
    let mut position = 0usize;
    loop {
        p = skip_trivia(chars, p, end);
        if p >= end || chars[p] == ']' {
            return None;
        }
        let value_end = skip_value(chars, p, end, true, 0);
        if value_end < 0 {
            return None;
        }
        let value_end = value_end as usize;
        if position == index {
            return Some(Span { start: p, end: value_end });
        }
        position += 1;
        p = skip_trivia(chars, value_end, end);
        if p < end && chars[p] == ',' {
            p += 1;
        }
    }
}

fn skip_key(chars: &[char], p: usize, end: usize) -> i64 {
    let c = chars[p];
    if c == '"' {
        return skip_quoted(chars, p, end, '"');
    }
    if c == '\'' {
        return if starts_with(chars, "'''", p) {
            skip_raw_string(chars, p, end)
        } else {
            skip_quoted(chars, p, end, '\'')
        };
    }
    let mut i = p;
    while i < end {
        let current = chars[i];
        if current == ':' || current == '\n' || current == '\r' || current == '{' || current == '}' {
            break;
        }
        i += 1;
    }
    let seg: String = chars[p..i].iter().collect();
    if is_blank(&seg) {
        -1
    } else {
        i as i64
    }
}

// --- value extents -----------------------------------------------------------------------------

/// 与 reader 的 MAX_DEPTH 同值：定位器作用于未经解析校验的原文（splice_inline），
/// 深嵌套坏文件不设限会栈溢出。超限按「定位失败」回 -1，调用方回落重序列化。
const MAX_DEPTH: usize = 256;

/// Index just past the value starting at `p` (trivia already skipped), or -1.
fn skip_value(chars: &[char], p: usize, end: usize, in_array: bool, depth: usize) -> i64 {
    if depth > MAX_DEPTH {
        return -1;
    }
    let c = chars[p];
    match c {
        '{' => skip_object(chars, p, end, depth + 1),
        '[' => skip_array(chars, p, end, depth + 1),
        '"' => skip_quoted(chars, p, end, '"'),
        '\'' => {
            if starts_with(chars, "'''", p) {
                skip_raw_string(chars, p, end)
            } else {
                skip_quoted(chars, p, end, '\'')
            }
        }
        _ => skip_bare(chars, p, end, in_array),
    }
}

fn skip_object(chars: &[char], p: usize, end: usize, depth: usize) -> i64 {
    let mut i = p + 1;
    loop {
        i = skip_trivia(chars, i, end);
        if i >= end {
            return -1;
        }
        if chars[i] == '}' {
            return i as i64 + 1;
        }
        let key_end = skip_key(chars, i, end);
        if key_end < 0 {
            return -1;
        }
        i = skip_trivia(chars, key_end as usize, end);
        if i >= end || chars[i] != ':' {
            return -1;
        }
        i = skip_trivia(chars, i + 1, end);
        if i >= end {
            return -1;
        }
        let next = skip_value(chars, i, end, false, depth);
        if next < 0 {
            return -1;
        }
        i = skip_trivia(chars, next as usize, end);
        if i < end && chars[i] == ',' {
            i += 1;
        }
    }
}

fn skip_array(chars: &[char], p: usize, end: usize, depth: usize) -> i64 {
    let mut i = p + 1;
    loop {
        i = skip_trivia(chars, i, end);
        if i >= end {
            return -1;
        }
        if chars[i] == ']' {
            return i as i64 + 1;
        }
        let next = skip_value(chars, i, end, true, depth);
        if next < 0 {
            return -1;
        }
        i = skip_trivia(chars, next as usize, end);
        if i < end && chars[i] == ',' {
            i += 1;
        }
    }
}

/// A quoteless value — mirror of `HjsonReader.parse_bare`: in an array a comma ends the value, in
/// an object it only ends it when the span up to the comma is a literal.
fn skip_bare(chars: &[char], p: usize, end: usize, in_array: bool) -> i64 {
    let mut i = p;
    while i < end {
        let c = chars[i];
        let comment = c == '#'
            || (c == '/' && i + 1 < end && (chars[i + 1] == '/' || chars[i + 1] == '*'));
        let line_end = c == '\n' || c == '\r' || c == ']' || c == '}' || comment;
        if line_end || c == ',' {
            if is_literal(chars, p, i) {
                return trim_end(chars, p, i) as i64;
            }
            if line_end || in_array {
                return trim_end(chars, p, i) as i64;
            }
        }
        i += 1;
    }
    trim_end(chars, p, end) as i64
}

fn is_literal(chars: &[char], start: usize, stop: usize) -> bool {
    let mut s = start;
    let mut e = stop;
    while s < e && (chars[s] as u32) <= (' ' as u32) {
        s += 1;
    }
    while e > s && (chars[e - 1] as u32) <= (' ' as u32) {
        e -= 1;
    }
    let token: String = chars[s..e].iter().collect();
    let first = token.chars().next().unwrap_or(' ');
    match first {
        'f' => token == "false",
        'n' => token == "null",
        't' => token == "true",
        _ => (first == '-' || first.is_ascii_digit()) && looks_like_number(&token),
    }
}

fn trim_end(chars: &[char], start: usize, stop: usize) -> usize {
    let mut e = stop;
    while e > start && (chars[e - 1] as u32) <= (' ' as u32) {
        e -= 1;
    }
    e
}

// --- strings and trivia ------------------------------------------------------------------------

fn skip_quoted(chars: &[char], p: usize, end: usize, quote: char) -> i64 {
    let mut i = p + 1;
    while i < end {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == quote {
            return i as i64 + 1;
        }
        i += 1;
    }
    -1
}

fn skip_raw_string(chars: &[char], p: usize, end: usize) -> i64 {
    match find_str(chars, "'''", p + 3) {
        Some(close) if close + 3 <= end => close as i64 + 3,
        _ => -1,
    }
}

fn skip_trivia(chars: &[char], p: usize, end: usize) -> usize {
    let mut i = p;
    while i < end {
        let c = chars[i];
        if (c as u32) <= (' ' as u32) {
            i += 1;
            continue;
        }
        if c == '#' || (c == '/' && i + 1 < end && chars[i + 1] == '/') {
            i = skip_to_line_end(chars, i + 1, end);
            continue;
        }
        if c == '/' && i + 1 < end && chars[i + 1] == '*' {
            match find_str(chars, "*/", i + 2) {
                Some(close) if close + 2 <= end => i = close + 2,
                _ => i = end,
            }
            continue;
        }
        break;
    }
    i
}

fn skip_to_line_end(chars: &[char], p: usize, end: usize) -> usize {
    let mut i = p;
    while i < end && chars[i] != '\n' {
        i += 1;
    }
    i
}

// --- misc --------------------------------------------------------------------------------------

fn unquote(key: &str) -> String {
    let trimmed = trim_le_le(key);
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() >= 2
        && (chars[0] == '"' || chars[0] == '\'')
        && chars[chars.len() - 1] == chars[0]
    {
        return chars[1..chars.len() - 1].iter().collect();
    }
    trimmed.to_string()
}

fn reindent(chars: &[char], span: Span, replacement: &str) -> String {
    let text = strip_trailing(replacement);
    let line_start = match rfind_newline(chars, span.start.saturating_sub(1)) {
        Some(i) if span.start > 0 => i + 1,
        _ => 0,
    };
    let indent: String = chars[line_start..span.start].iter().collect();
    if !is_blank(&indent) {
        // The value starts mid-line (ammoTypes: { name: {...} }); the text keeps its own layout.
        return text.to_string();
    }
    if indent.is_empty() {
        text.to_string()
    } else {
        text.replace('\n', &format!("\n{indent}"))
    }
}

fn starts_with(chars: &[char], needle: &str, at: usize) -> bool {
    let nc: Vec<char> = needle.chars().collect();
    at + nc.len() <= chars.len() && chars[at..at + nc.len()] == nc[..]
}

fn find_str(chars: &[char], needle: &str, from: usize) -> Option<usize> {
    let nc: Vec<char> = needle.chars().collect();
    if nc.is_empty() {
        return Some(from);
    }
    let mut i = from;
    while i + nc.len() <= chars.len() {
        if chars[i..i + nc.len()] == nc[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn rfind_newline(chars: &[char], at: usize) -> Option<usize> {
    if chars.is_empty() {
        return None;
    }
    let limit = at.min(chars.len() - 1);
    let mut i = limit + 1;
    while i > 0 {
        i -= 1;
        if chars[i] == '\n' {
            return Some(i);
        }
    }
    None
}

/// Java `String.trim()` — strips leading/trailing code points `<= ' '`.
fn trim_le_le(s: &str) -> &str {
    let b = s.as_bytes();
    let mut start = 0;
    while start < b.len() && b[start] <= b' ' {
        start += 1;
    }
    let mut end = b.len();
    while end > start && b[end - 1] <= b' ' {
        end -= 1;
    }
    &s[start..end]
}

/// Java `String.stripTrailing()` — removes trailing Unicode whitespace.
fn strip_trailing(s: &str) -> &str {
    let chars: Vec<char> = s.chars().collect();
    let mut end = chars.len();
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let count = chars[..end].iter().map(|c| c.len_utf8()).sum::<usize>();
    &s[..count]
}

/// Java `String.isBlank()`.
fn is_blank(s: &str) -> bool {
    s.chars().all(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_nesting_fails_locate_instead_of_overflowing() {
        // 300 层 > MAX_DEPTH(256)：定位必须失败回落，而不是栈溢出。
        let mut text = String::new();
        for _ in 0..300 {
            text.push_str("{\"a\":");
        }
        text.push('1');
        for _ in 0..300 {
            text.push('}');
        }
        assert_eq!(value_span(&text, "a"), None);
        assert_eq!(replace(&text, "a", "2"), None);
    }

    #[test]
    fn shallow_nesting_still_locates() {
        let text = "{\"a\":{\"b\":7}}";
        let span = value_span(text, "a.b").expect("span");
        assert_eq!(span_text(text, span), "7");
        assert_eq!(replace(text, "a.b", "8").unwrap(), "{\"a\":{\"b\":8}}");
    }
}
