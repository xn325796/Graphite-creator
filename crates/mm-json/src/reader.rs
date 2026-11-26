//! Tolerant reader for the JSON/HJSON dialect Mindustry accepts.
//!
//! Port of `core.json.HjsonReader`. The semantics follow the game's `ContentParser` (arc `Jval`),
//! not the hjson.org spec: `//`, `#` and block comments, unquoted keys, single quotes, trailing
//! commas, newline-separated members in a braceless root, comma-is-a-string-in-object-context,
//! triple-quoted multiline strings with arc indent-stripping, and `#` → `\#` rewriting for `.json`.

use crate::value::{Json, Object};

/// A parse failure whose message carries the line/column, mirroring the Java `IllegalArgumentException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HjsonError(pub String);

impl std::fmt::Display for HjsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HjsonError {}

const MAX_DEPTH: usize = 256;

/// `NUMBER` in the Java reader: `-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?`, matched over the whole
/// token. Hand-rolled to avoid a regex dependency and to keep exact rejection of leading zeros.
pub fn looks_like_number(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = 0usize;
    if i < b.len() && b[i] == b'-' {
        i += 1;
    }
    // Integral part: `0` or `[1-9]\d*`.
    if i >= b.len() {
        return false;
    }
    if b[i] == b'0' {
        i += 1;
    } else if b[i].is_ascii_digit() {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return false;
    }
    // Optional fraction `\.\d+`.
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    // Optional exponent `[eE][+-]?\d+`.
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == b.len()
}

/// Like `String.trim()` (Java) — strips leading/trailing code points `<= ' '`.
fn trim_ascii(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut start = 0;
    while start < bytes.len() && bytes[start] <= b' ' {
        start += 1;
    }
    let mut end = bytes.len();
    while end > start && bytes[end - 1] <= b' ' {
        end -= 1;
    }
    &s[start..end]
}

struct Reader {
    src: Vec<char>,
    pos: usize,
    depth: usize,
    in_array: bool,
}

pub fn read(text: &str) -> Result<Json, HjsonError> {
    let stripped = strip_bom(text);
    let reader = Reader::new(stripped);
    reader.read_inner()
}

pub fn read_json(text: &str) -> Result<Json, HjsonError> {
    read(&text.replace('#', "\\#"))
}

pub fn read_object(text: &str) -> Result<Object, HjsonError> {
    match read(text)? {
        Json::Obj(o) => Ok(o),
        _ => Err(HjsonError("Expected a JSON object".to_string())),
    }
}

pub fn read_json_object(text: &str) -> Result<Object, HjsonError> {
    match read_json(text)? {
        Json::Obj(o) => Ok(o),
        _ => Err(HjsonError("Expected a JSON object".to_string())),
    }
}

pub fn strip_bom(text: &str) -> &str {
    // The BOM is a single UTF-16 char in Java, but three UTF-8 bytes here, so strip by char rather
    // than by byte index.
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

impl Reader {
    fn new(text: &str) -> Self {
        Reader { src: text.chars().collect(), pos: 0, depth: 0, in_array: false }
    }

    fn read_inner(self) -> Result<Json, HjsonError> {
        let mut r = self;
        r.skip_trivia();
        if let Some(c) = r.peek()
            && (c == '{' || c == '[')
        {
            let value = r.parse_value()?;
            r.skip_trivia();
            if !r.at_end() {
                return Err(r.error("Unexpected trailing content"));
            }
            return Ok(value);
        }
        // Braceless root object; retry as a single scalar on failure, like arc Jval.parse.
        let mut attempt = Reader { src: r.src.clone(), pos: 0, depth: 0, in_array: false };
        match attempt.parse_unbraced_object() {
            Ok(object) => {
                attempt.skip_trivia();
                if attempt.at_end() {
                    return Ok(Json::Obj(object));
                }
                Err(attempt.error("Unexpected trailing content"))
            }
            Err(object_failure) => {
                let mut retry = Reader { src: r.src.clone(), pos: 0, depth: 0, in_array: false };
                retry.skip_trivia();
                match retry.parse_value() {
                    Ok(value) => {
                        retry.skip_trivia();
                        if retry.at_end() {
                            Ok(value)
                        } else {
                            Err(object_failure)
                        }
                    }
                    Err(_) => Err(object_failure),
                }
            }
        }
    }

    fn parse_value(&mut self) -> Result<Json, HjsonError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error(&format!("JSON nested more than {MAX_DEPTH} levels deep")));
        }
        let result = self.parse_value_inner();
        self.depth -= 1;
        result
    }

    fn parse_value_inner(&mut self) -> Result<Json, HjsonError> {
        self.skip_trivia();
        if self.at_end() {
            return Err(self.error("Unexpected end of input"));
        }
        match self.peek().unwrap() {
            '{' => self.parse_object().map(Json::Obj),
            '[' => self.parse_array(),
            '"' => self.parse_quoted('"').map(Json::Str),
            '\'' => {
                if self.starts_with_str("'''") {
                    self.parse_raw_string().map(Json::Str)
                } else {
                    self.parse_quoted('\'').map(Json::Str)
                }
            }
            _ => self.parse_bare(),
        }
    }

    fn parse_object(&mut self) -> Result<Object, HjsonError> {
        self.expect('{')?;
        let mut object = Object::new();
        loop {
            self.skip_trivia();
            if self.at_end() {
                return Err(self.error("Unterminated object"));
            }
            if self.peek() == Some('}') {
                self.pos += 1;
                return Ok(object);
            }
            let key = self.parse_key()?;
            self.skip_trivia();
            self.expect(':')?;
            let value = self.parse_value()?;
            object.insert(key, value);
            self.skip_trivia();
            if self.peek() == Some(',') {
                self.pos += 1;
            }
        }
    }

    fn parse_unbraced_object(&mut self) -> Result<Object, HjsonError> {
        let mut object = Object::new();
        loop {
            self.skip_trivia();
            if self.at_end() {
                return Ok(object);
            }
            let key = self.parse_key()?;
            self.skip_trivia();
            self.expect(':')?;
            let value = self.parse_value()?;
            object.insert(key, value);
            self.skip_trivia();
            if self.peek() == Some(',') {
                self.pos += 1;
            }
        }
    }

    fn parse_array(&mut self) -> Result<Json, HjsonError> {
        self.expect('[')?;
        let mut array = Vec::new();
        let outer = self.in_array;
        self.in_array = true;
        let result = self.parse_array_inner(&mut array);
        self.in_array = outer;
        result?;
        Ok(Json::Arr(array))
    }

    fn parse_array_inner(&mut self, array: &mut Vec<Json>) -> Result<(), HjsonError> {
        loop {
            self.skip_trivia();
            if self.at_end() {
                return Err(self.error("Unterminated array"));
            }
            if self.peek() == Some(']') {
                self.pos += 1;
                return Ok(());
            }
            let value = self.parse_value()?;
            array.push(value);
            self.skip_trivia();
            if self.peek() == Some(',') {
                self.pos += 1;
            }
        }
    }

    fn parse_key(&mut self) -> Result<String, HjsonError> {
        self.skip_trivia();
        match self.peek() {
            Some('"') => return self.parse_quoted('"'),
            Some('\'') => return self.parse_quoted('\''),
            _ => {}
        }
        let start = self.pos;
        while !self.at_end() {
            let current = self.peek().unwrap();
            if current == ':' || current == '\n' || current == '\r' || current == '{' || current == '}'
            {
                break;
            }
            self.pos += 1;
        }
        let key: String = self.src[start..self.pos].iter().collect();
        let key = trim_ascii(&key).to_string();
        if key.is_empty() {
            return Err(self.error("Expected a key"));
        }
        Ok(key)
    }

    fn parse_quoted(&mut self, quote: char) -> Result<String, HjsonError> {
        self.expect(quote)?;
        let mut out = String::new();
        loop {
            if self.at_end() {
                return Err(self.error("Unterminated string"));
            }
            let c = self.src[self.pos];
            self.pos += 1;
            if c == quote {
                return Ok(out);
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            if self.at_end() {
                return Err(self.error("Unterminated escape"));
            }
            let escaped = self.src[self.pos];
            self.pos += 1;
            match escaped {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                '/' => out.push('/'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    if self.pos + 4 > self.src.len() {
                        return Err(self.error("Truncated unicode escape"));
                    }
                    let hex: String = self.src[self.pos..self.pos + 4].iter().collect();
                    self.pos += 4;
                    let value = u32::from_str_radix(&hex, 16)
                        .map_err(|_| self.error(&format!("Bad unicode escape \\u{hex}")))?;
                    out.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
                }
                // Unknown escape is literal (the game rewrites bare '#' to '\#').
                other => out.push(other),
            }
        }
    }

    /// Triple-quoted string with arc `readMlString` indent semantics.
    fn parse_raw_string(&mut self) -> Result<String, HjsonError> {
        let line_start = self.rfind_newline(self.pos).map_or(0, |i| i + 1);
        let indent = self.pos - line_start;
        self.pos += 3;
        let end = match self.find_str("'''", self.pos) {
            Some(e) => e,
            None => return Err(self.error("Unterminated raw string")),
        };
        let body: String = self.src[self.pos..end].iter().collect();
        self.pos = end + 3;
        let normalised = body.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalised.split('\n').collect();
        let mut out = String::new();
        let mut first_written = !lines[0].is_empty() && !trim_ascii(lines[0]).is_empty();
        if first_written {
            out.push_str(strip_leading(lines[0]));
        }
        for (i, &line) in lines.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let mut strip = 0usize;
            let lb: Vec<char> = line.chars().collect();
            while strip < indent && strip < lb.len() && (lb[strip] == ' ' || lb[strip] == '\t') {
                strip += 1;
            }
            let stripped: String = lb[strip..].iter().collect();
            if first_written {
                out.push('\n');
            }
            first_written = true;
            out.push_str(&stripped);
        }
        if out.ends_with('\n') {
            out.remove(out.len() - 1);
        }
        Ok(out)
    }

    fn parse_bare(&mut self) -> Result<Json, HjsonError> {
        let start = self.pos;
        let first = self.peek().unwrap();
        if ",:[]}".contains(first) {
            return Err(self.error(&format!(
                "Found a punctuator character '{first}' when expecting a quoteless string (check your syntax)"
            )));
        }
        loop {
            let is_comment = !self.at_end()
                && (self.peek() == Some('#')
                    || (self.peek() == Some('/')
                        && self.pos + 1 < self.src.len()
                        && (self.src[self.pos + 1] == '/' || self.src[self.pos + 1] == '*')));
            let p = self.peek();
            let is_eol = self.at_end()
                || p == Some('\n')
                || p == Some('\r')
                || p == Some(']')
                || p == Some('}')
                || is_comment
                || (p == Some(',') && self.in_array);
            if is_eol || p == Some(',') {
                let stop = if self.at_end() { self.src.len() } else { self.pos };
                if let Some(literal) = self.try_literal(start, stop, first) {
                    return Ok(literal);
                }
                if is_eol {
                    let mut end = stop;
                    if end > start && self.src[end - 1] == ',' {
                        end -= 1;
                    }
                    let raw: String = self.src[start..end].iter().collect();
                    let trimmed = trim_ascii(&raw).replace("\\#", "#");
                    return Ok(Json::Str(trimmed));
                }
            }
            self.pos += 1;
        }
    }

    fn try_literal(&self, start: usize, stop: usize, first: char) -> Option<Json> {
        let mut s = start;
        let mut e = stop;
        while s < e && self.src[s] <= ' ' {
            s += 1;
        }
        while e > s && self.src[e - 1] <= ' ' {
            e -= 1;
        }
        let len = e - s;
        match first {
            'f' => {
                if len == 5 && self.starts_with_at("false", s) {
                    Some(Json::Bool(false))
                } else {
                    None
                }
            }
            'n' => {
                if len == 4 && self.starts_with_at("null", s) {
                    Some(Json::Null)
                } else {
                    None
                }
            }
            't' => {
                if len == 4 && self.starts_with_at("true", s) {
                    Some(Json::Bool(true))
                } else {
                    None
                }
            }
            _ => {
                if first == '-' || first.is_ascii_digit() {
                    let token: String = self.src[s..e].iter().collect();
                    if looks_like_number(&token) {
                        Some(Json::Num(token))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    fn skip_trivia(&mut self) {
        while !self.at_end() {
            let c = self.peek().unwrap();
            if is_whitespace(c) {
                self.pos += 1;
                continue;
            }
            if c == '/' && self.pos + 1 < self.src.len() {
                let next = self.src[self.pos + 1];
                if next == '/' {
                    self.skip_to_line_end();
                    continue;
                }
                if next == '*' {
                    self.skip_block_comment();
                    continue;
                }
            }
            if c == '#' {
                self.skip_to_line_end();
                continue;
            }
            return;
        }
    }

    fn skip_to_line_end(&mut self) {
        while !self.at_end() && self.peek() != Some('\n') {
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        self.pos += 2;
        while self.pos + 1 < self.src.len()
            && !(self.src[self.pos] == '*' && self.src[self.pos + 1] == '/')
        {
            self.pos += 1;
        }
        self.pos = (self.pos + 2).min(self.src.len());
    }

    fn expect(&mut self, expected: char) -> Result<(), HjsonError> {
        self.skip_trivia();
        if self.at_end() || self.peek() != Some(expected) {
            return Err(self.error(&format!("Expected '{expected}'")));
        }
        self.pos += 1;
        Ok(())
    }

    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn starts_with_str(&self, needle: &str) -> bool {
        self.starts_with_at(needle, self.pos)
    }

    fn starts_with_at(&self, needle: &str, at: usize) -> bool {
        let nc: Vec<char> = needle.chars().collect();
        at + nc.len() <= self.src.len() && self.src[at..at + nc.len()] == nc[..]
    }

    fn find_str(&self, needle: &str, from: usize) -> Option<usize> {
        let nc: Vec<char> = needle.chars().collect();
        if nc.is_empty() {
            return Some(from);
        }
        let mut i = from;
        while i + nc.len() <= self.src.len() {
            if self.src[i..i + nc.len()] == nc[..] {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn rfind_newline(&self, at: usize) -> Option<usize> {
        // Java lastIndexOf('\n', pos): the last newline at an index <= `at`, or None.
        let limit = at.min(self.src.len().saturating_sub(1));
        if self.src.is_empty() {
            return None;
        }
        let mut i = limit + 1;
        while i > 0 {
            i -= 1;
            if self.src[i] == '\n' {
                return Some(i);
            }
        }
        None
    }

    fn error(&self, message: &str) -> HjsonError {
        let mut line = 1;
        let mut column = 1;
        let limit = self.pos.min(self.src.len());
        for &c in &self.src[..limit] {
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        HjsonError(format!("{message} at line {line}, column {column}"))
    }
}

fn is_whitespace(c: char) -> bool {
    c.is_whitespace()
}

fn strip_leading(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut start = 0;
    while start < bytes.len() && bytes[start] <= b' ' {
        start += 1;
    }
    &s[start..]
}
