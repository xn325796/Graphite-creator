//! `mm-json` — the Mindustry HJSON/JSON dialect: tolerant reader, round-trip writer, paths and
//! text patches. Port of the Java `core.json` package (`HjsonReader`, `HjsonWriter`, `Json`,
//! `JsonPath`, `JsonTextPatch`). It is the foundation every other crate builds on, so it carries
//! no dependency on the Mindustry sources beyond their observable behaviour.

mod facade;
mod patch;
mod path;
mod reader;
mod value;
mod writer;

pub use facade::{
    parse, parse_object, parse_strict_object, read_file, strip_bom, write_compact,
    write_file, write as write_pretty,
};
pub use patch::{parse_or_null, replace, span_text, value_span, Span};
pub use path::{
    child, child_index, escape_segment, get, leaf_name, remove, set, steps, unescape_segment, Step,
};
pub use reader::{looks_like_number, read, read_json, read_json_object, read_object, HjsonError};
pub use value::{Json, Object};
pub use writer::{write, write_nested};
