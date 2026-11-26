//! Serialise a [`Json`] tree as HJSON — the dialect Mindustry mods are hand-written in.
//!
//! Port of `core.json.HjsonWriter`. The invariant is round trip:
//! `read(write(tree)) == tree`. Strings that merely look like a number/bool/null, or that contain a
//! comma, hash or comment opener, are quoted because they would otherwise re-parse as something
//! else. A whole file is a braceless root object; [`write_nested`] keeps the braces so a value can
//! be spliced into `key: ...` without leaking its members.

use crate::reader::looks_like_number;
use crate::value::Json;

/// A whole file: the root object is written braceless.
pub fn write(element: &Json) -> String {
    let mut out = String::new();
    if let Json::Obj(object) = element {
        write_members(object, &mut out, "", true);
    } else {
        write_value(element, &mut out, "");
    }
    out
}

/// A value as it appears inside a file, so an object keeps its braces.
pub fn write_nested(element: &Json) -> String {
    let mut out = String::new();
    write_value(element, &mut out, "");
    out
}

fn write_members(object: &crate::value::Object, out: &mut String, indent: &str, braceless: bool) {
    let mut first = true;
    for (key, value) in object.iter() {
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(indent);
        out.push_str(&write_key(key));
        out.push_str(": ");
        match value {
            Json::Obj(nested) => {
                if nested.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push_str("{\n");
                    let child_indent = format!("{indent}  ");
                    write_members(nested, out, &child_indent, false);
                    out.push('\n');
                    out.push_str(indent);
                    out.push('}');
                }
            }
            other => write_value(other, out, indent),
        }
    }
    if braceless && !out.is_empty() {
        out.push('\n');
    }
}

fn write_array(array: &[Json], out: &mut String, indent: &str) {
    let mut all_primitives = true;
    let mut length = 0usize;
    for element in array {
        match element {
            Json::Obj(_) | Json::Arr(_) => {
                all_primitives = false;
                break;
            }
            // 直接借原文数字符，不为量长度克隆一份 String（get_as_string 是 owned）。
            Json::Null | Json::Bool(true) => length += 4 + 2,
            Json::Bool(false) => length += 5 + 2,
            Json::Num(text) | Json::Str(text) => length += text.chars().count() + 2,
        }
    }
    if all_primitives && length < 70 {
        out.push('[');
        for (i, element) in array.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write_value(element, out, indent);
        }
        out.push(']');
        return;
    }
    out.push_str("[\n");
    for (i, element) in array.iter().enumerate() {
        out.push_str(indent);
        out.push_str("  ");
        match element {
            Json::Obj(nested) => {
                if nested.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push_str("{\n");
                    let child_indent = format!("{indent}  ");
                    write_members(nested, out, &child_indent, false);
                    out.push('\n');
                    out.push_str(indent);
                    out.push_str("  }");
                }
            }
            other => {
                let inner_indent = format!("{indent}  ");
                write_value(other, out, &inner_indent);
            }
        }
        if i < array.len() - 1 {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(indent);
    out.push(']');
}

fn write_value(value: &Json, out: &mut String, indent: &str) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(text) => out.push_str(text),
        Json::Str(s) => {
            if is_safe_quoteless(s) {
                out.push_str(s);
            } else {
                out.push_str(&quote(s));
            }
        }
        Json::Arr(a) => write_array(a, out, indent),
        Json::Obj(o) => {
            if o.is_empty() {
                out.push_str("{}");
            } else {
                out.push_str("{\n");
                let child_indent = format!("{indent}  ");
                write_members(o, out, &child_indent, false);
                out.push('\n');
                out.push_str(indent);
                out.push('}');
            }
        }
    }
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => {
                if (other as u32) < (' ' as u32) {
                    out.push_str(&format!("\\u{:04x}", other as u32));
                } else {
                    out.push(other);
                }
            }
        }
    }
    out.push('"');
    out
}

/// Whether a quoteless spelling re-parses to the same string.
fn is_safe_quoteless(text: &str) -> bool {
    // 单趟迭代 + peek 看下一字符，不为判定收集整条 Vec<char>（热路径：每个字符串值都过这里）。
    let mut it = text.chars().peekable();
    let first = match it.next() {
        Some(c) => c,
        None => return false,
    };
    if (first as u32) <= (' ' as u32) || ",:{}[]\"'#".contains(first) {
        return false;
    }
    let mut last = first;
    let mut c = first;
    loop {
        if (c as u32) < (' ' as u32)
            || c == '#'
            || c == '"'
            || c == '\''
            || c == '\\'
            || c == '}'
            || c == ']'
        {
            return false;
        }
        if c == '/' && matches!(it.peek(), Some('/') | Some('*')) {
            return false;
        }
        match it.next() {
            Some(next) => {
                last = next;
                c = next;
            }
            None => break,
        }
    }
    if (last as u32) <= (' ' as u32) || text.ends_with(',') || text.ends_with(':') {
        return false;
    }
    if text.contains(',') {
        return false;
    }
    if text == "true" || text == "false" || text == "null" {
        return false;
    }
    !looks_like_number(text)
}

fn write_key(key: &str) -> String {
    let ok = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        key.to_string()
    } else {
        quote(key)
    }
}
