//! Dotted paths into a [`Json`] tree with optional array indices: `ammoTypes.copper`,
//! `objectives[0].preset`. Port of `core.json.JsonPath`.

use crate::value::Json;

const MAX_INDEX: usize = 100_000;

/// One step of a path: a member name or an array index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub key: Option<String>,
    pub index: usize,
    pub is_index: bool,
}

impl Step {
    pub fn member(key: impl Into<String>) -> Step {
        Step { key: Some(key.into()), index: 0, is_index: false }
    }

    pub fn index(index: usize) -> Step {
        Step { key: None, index, is_index: true }
    }
}

fn new_shape(next: &Step) -> Json {
    if next.is_index {
        Json::Arr(Vec::new())
    } else {
        Json::object()
    }
}

pub fn steps(path: &str) -> Vec<Step> {
    let mut out = Vec::new();
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        match segment.find('[') {
            None => out.push(Step::member(unescape_segment(segment))),
            Some(b) => {
                let key = unescape_segment(&segment[..b]);
                if !key.is_empty() {
                    out.push(Step::member(key));
                }
                for index in parse_indices(&segment[b..]) {
                    out.push(Step::index(index));
                }
            }
        }
    }
    out
}

/// Scans `\[(\d+)]` occurrences, mirroring the Java `Matcher` loop with the bounded-index rule.
fn parse_indices(text: &str) -> Vec<usize> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'[' {
            let digits_start = i + 1;
            let mut j = digits_start;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > digits_start && j < b.len() && b[j] == b']' {
                if let Ok(value) = text[digits_start..j].parse::<usize>()
                    && value <= MAX_INDEX
                {
                    out.push(value);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn get<'a>(root: &'a Json, path: &str) -> Option<&'a Json> {
    let steps = steps(path);
    get_steps(root, &steps)
}

fn get_steps<'a>(node: &'a Json, steps: &[Step]) -> Option<&'a Json> {
    let (step, rest) = match steps.split_first() {
        Some(x) => x,
        None => return Some(node),
    };
    if node.is_null() {
        return None;
    }
    let child = if step.is_index {
        match node {
            Json::Arr(a) if step.index < a.len() => &a[step.index],
            _ => return None,
        }
    } else {
        match node {
            Json::Obj(o) => o.get(step.key.as_deref().unwrap_or(""))?,
            _ => return None,
        }
    };
    if rest.is_empty() {
        Some(child)
    } else {
        get_steps(child, rest)
    }
}

/// Replaces the value at `path`, creating intermediate members when missing.
pub fn set(root: &mut Json, path: &str, value: Json) {
    let steps = steps(path);
    set_steps(root, &steps, value);
}

fn set_steps(node: &mut Json, steps: &[Step], value: Json) {
    let (step, rest) = match steps.split_first() {
        Some(x) => x,
        None => return,
    };
    if rest.is_empty() {
        if step.is_index {
            if let Json::Arr(array) = node {
                pad_to(array, step.index, None);
                array[step.index] = value;
            }
        } else if let Json::Obj(object) = node {
            object.insert(step.key.clone().unwrap_or_default(), value);
        }
        return;
    }
    let next = &rest[0];
    if step.is_index {
        if let Json::Arr(array) = node {
            pad_to(array, step.index, Some(next));
            let slot = &mut array[step.index];
            if !is_compatible(slot, next) {
                *slot = new_shape(next);
            }
            set_steps(slot, rest, value);
        }
    } else if let Json::Obj(object) = node {
        let key = step.key.clone().unwrap_or_default();
        let needs = match object.get(&key) {
            None => true,
            Some(child) => !is_compatible(child, next),
        };
        if needs {
            object.insert(key.clone(), new_shape(next));
        }
        if let Some(child) = object.get_mut(&key) {
            set_steps(child, rest, value);
        }
    }
}

fn is_compatible(element: &Json, next: &Step) -> bool {
    if next.is_index {
        element.is_array()
    } else {
        element.is_object()
    }
}

fn pad_to(array: &mut Vec<Json>, index: usize, next: Option<&Step>) {
    while array.len() <= index {
        array.push(match next {
            Some(s) if s.is_index => Json::Arr(Vec::new()),
            _ => Json::object(),
        });
    }
}

pub fn leaf_name(path: &str) -> String {
    let steps = steps(path);
    for step in steps.iter().rev() {
        if !step.is_index {
            return step.key.clone().unwrap_or_default();
        }
    }
    path.to_string()
}

/// Removes the value at `path`. Returns true when something was removed.
pub fn remove(root: &mut Json, path: &str) -> bool {
    let steps = steps(path);
    if steps.is_empty() {
        return false;
    }
    remove_steps(root, &steps)
}

fn remove_steps(node: &mut Json, steps: &[Step]) -> bool {
    let (step, rest) = steps.split_first().unwrap();
    if rest.is_empty() {
        if step.is_index {
            if let Json::Arr(array) = node
                && step.index < array.len()
            {
                array.remove(step.index);
                return true;
            }
            false
        } else if let Json::Obj(object) = node {
            object.remove(step.key.as_deref().unwrap_or("")).is_some()
        } else {
            false
        }
    } else {
        let child = if step.is_index {
            match node {
                Json::Arr(a) if step.index < a.len() => &mut a[step.index],
                _ => return false,
            }
        } else {
            match node {
                Json::Obj(o) => match o.get_mut(step.key.as_deref().unwrap_or("")) {
                    Some(child) => child,
                    None => return false,
                },
                _ => return false,
            }
        };
        remove_steps(child, rest)
    }
}

pub fn child(path: &str, key: &str) -> String {
    let escaped = escape_segment(key);
    if path.is_empty() {
        escaped
    } else {
        format!("{path}.{escaped}")
    }
}

pub fn escape_segment(key: &str) -> String {
    key.replace('~', "~0").replace('.', "~1").replace('[', "~2").replace(']', "~3")
}

pub fn unescape_segment(segment: &str) -> String {
    let chars: Vec<char> = segment.chars().collect();
    let mut out = String::with_capacity(segment.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '~' && i + 1 < chars.len() {
            let next = chars[i + 1];
            match next {
                '0' => out.push('~'),
                '1' => out.push('.'),
                '2' => out.push('['),
                '3' => out.push(']'),
                other => {
                    out.push('~');
                    out.push(other);
                }
            }
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

pub fn child_index(path: &str, index: usize) -> String {
    format!("{path}[{index}]")
}
