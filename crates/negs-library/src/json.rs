//! JSON as the macOS app's `JSONValue` holds it (`Parameters.swift`): numbers
//! are doubles, objects are unordered. Reading follows Foundation's
//! `JSONDecoder` and writing its `JSONEncoder`, byte for byte, so a sidecar
//! written here is the file the Mac would write.

use std::collections::BTreeMap;
use std::fmt;

/// A JSON value. Objects keep their keys sorted, which is the order every
/// file is written in.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

static NULL: Json = Json::Null;

/// Why bytes aren't JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub reason: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.reason, self.offset)
    }
}

impl std::error::Error for ParseError {}

impl Json {
    /// An empty object.
    pub fn object() -> Json {
        Json::Object(BTreeMap::new())
    }

    pub fn from_pairs<K: Into<String>>(pairs: impl IntoIterator<Item = (K, Json)>) -> Json {
        Json::Object(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn string(text: impl Into<String>) -> Json {
        Json::String(text.into())
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        if let Json::Object(o) = self { Some(o) } else { None }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Json>> {
        if let Json::Object(o) = self { Some(o) } else { None }
    }

    pub fn as_array(&self) -> Option<&Vec<Json>> {
        if let Json::Array(a) = self { Some(a) } else { None }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Json::String(s) = self { Some(s) } else { None }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Json::Number(n) = self { Some(*n) } else { None }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Json::Bool(b) = self { Some(*b) } else { None }
    }

    /// A whole number below 10^15, as `JSONValue.integer`.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Json::Number(v) if v.round() == *v && v.abs() < 1e15 => Some(*v as i64),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// The value under `key` of an object; `Null` when there is none.
    pub fn get(&self, key: &str) -> &Json {
        self.as_object().and_then(|o| o.get(key)).unwrap_or(&NULL)
    }

    /// The value at a dotted path (`"prep.highlights"`, array indices as
    /// numbers), `Null` when there is none: `JSONValue`'s subscript.
    pub fn at(&self, path: &str) -> &Json {
        let mut value = self;
        for key in path.split('.').filter(|k| !k.is_empty()) {
            value = match value {
                Json::Object(o) => match o.get(key) {
                    Some(v) => v,
                    None => return &NULL,
                },
                Json::Array(a) => match key.parse::<usize>().ok().and_then(|i| a.get(i)) {
                    Some(v) => v,
                    None => return &NULL,
                },
                _ => return &NULL,
            };
        }
        value
    }

    /// Sets the value at a dotted path, making objects on the way as
    /// `JSONValue`'s subscript does. Setting `Null` stores a null.
    pub fn set_at(&mut self, path: &str, new: Json) {
        let keys: Vec<&str> = path.split('.').filter(|k| !k.is_empty()).collect();
        self.set_keys(&keys, new);
    }

    fn set_keys(&mut self, keys: &[&str], new: Json) {
        let Some((key, rest)) = keys.split_first() else {
            *self = new;
            return;
        };
        match self {
            Json::Object(o) => o.entry(key.to_string()).or_insert_with(Json::object).set_keys(rest, new),
            Json::Array(a) => {
                if let Some(child) = key.parse::<usize>().ok().and_then(|i| a.get_mut(i)) {
                    child.set_keys(rest, new);
                }
            }
            _ => {}
        }
    }

    /// `self` with `changes` laid over it: objects merge key by key,
    /// anything else is replaced whole (`JSONValue.overlaid`).
    pub fn overlaid(&self, changes: &Json) -> Json {
        match (self, changes) {
            (Json::Object(base), Json::Object(values)) => {
                let mut out = base.clone();
                for (key, value) in values {
                    let merged = match out.get(key) {
                        Some(old) => old.overlaid(value),
                        None => value.clone(),
                    };
                    out.insert(key.clone(), merged);
                }
                Json::Object(out)
            }
            _ => changes.clone(),
        }
    }

    /// What `self` changes from `base`, compared exactly: objects recurse,
    /// any other value is kept whole when it differs. `None` when nothing
    /// does (`JSONValue.changes(from:)`).
    pub fn changes_from(&self, base: &Json) -> Option<Json> {
        if let (Json::Object(values), Json::Object(defaults)) = (self, base) {
            let mut changed = BTreeMap::new();
            for (key, value) in values {
                match defaults.get(key) {
                    Some(fallback) => {
                        if let Some(change) = value.changes_from(fallback) {
                            changed.insert(key.clone(), change);
                        }
                    }
                    None => {
                        changed.insert(key.clone(), value.clone());
                    }
                }
            }
            return if changed.is_empty() { None } else { Some(Json::Object(changed)) };
        }
        if self == base { None } else { Some(self.clone()) }
    }

    /// The parts of `self`, a stored change, under keys none of `known` has
    /// at that place: what a newer version stored, to be written back
    /// unchanged (`JSONValue.unknown(to:)`).
    pub fn unknown_to(&self, known: &[&Json]) -> Option<Json> {
        let Json::Object(values) = self else { return None };
        let mut kept = BTreeMap::new();
        for (key, value) in values {
            let children: Vec<&Json> = known.iter().filter_map(|node| node.as_object().and_then(|o| o.get(key))).collect();
            if children.is_empty() {
                kept.insert(key.clone(), value.clone());
            } else if let Some(deeper) = value.unknown_to(&children) {
                kept.insert(key.clone(), deeper);
            }
        }
        if kept.is_empty() { None } else { Some(Json::Object(kept)) }
    }

    /// Keeps the engine's current defaults (`self`) and restores the saved
    /// values it knows, dropping the ones it doesn't (`JSONValue.restoring`).
    /// A saved `null` stays where `nullable` (the catalog's
    /// `nullable_paths`, `[*]` for array elements) says it's a value.
    pub fn restoring(&self, saved: &Json, nullable: &[String]) -> Json {
        self.restoring_at(saved, nullable, "")
    }

    fn restoring_at(&self, saved: &Json, nullable: &[String], path: &str) -> Json {
        if saved.is_null() && nullable.iter().any(|p| p == path) {
            return saved.clone();
        }
        match (self, saved) {
            (Json::Object(defaults), Json::Object(values)) => {
                let mut result = defaults.clone();
                for (key, value) in values {
                    let Some(fallback) = defaults.get(key) else { continue };
                    let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                    result.insert(key.clone(), fallback.restoring_at(value, nullable, &child));
                }
                Json::Object(result)
            }
            (Json::Array(defaults), Json::Array(values)) if defaults.len() == values.len() => {
                let child = format!("{path}[*]");
                Json::Array(defaults.iter().zip(values).map(|(d, v)| d.restoring_at(v, nullable, &child)).collect())
            }
            (Json::Number(_), Json::Number(_)) | (Json::Bool(_), Json::Bool(_)) | (Json::String(_), Json::String(_)) | (Json::Null, _) => {
                saved.clone()
            }
            _ => self.clone(),
        }
    }

    // MARK: Reading

    /// Parses bytes as `JSONDecoder` does: strict JSON, plus a leading byte
    /// order mark and trailing commas, which it accepts. The first of two
    /// equal keys wins; a number out of a double's range is refused.
    pub fn parse(bytes: &[u8]) -> Result<Json, ParseError> {
        let text = std::str::from_utf8(bytes).map_err(|e| ParseError { offset: e.valid_up_to(), reason: "invalid UTF-8" })?;
        let mut parser = Parser { s: text.as_bytes(), i: 0 };
        if text.starts_with('\u{FEFF}') {
            parser.i = 3;
        }
        parser.ws();
        let value = parser.value(0)?;
        parser.ws();
        if parser.i != parser.s.len() {
            return Err(parser.error("unexpected text after the value"));
        }
        Ok(value)
    }

    // MARK: Writing

    /// The bytes of a sidecar: `JSONEncoder` with pretty printing, sorted
    /// keys and unescaped slashes, then a newline (`PhotoSidecar.encode`).
    pub fn to_sidecar_bytes(&self) -> Vec<u8> {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out.push('\n');
        out.into_bytes()
    }

    /// `JSONEncoder` with sorted keys only, as the thumbnail looks hash it:
    /// no spaces, slashes escaped.
    pub fn to_compact_sorted(&self) -> String {
        let mut out = String::new();
        self.write_compact(&mut out);
        out
    }

    fn write_pretty(&self, out: &mut String, depth: usize) {
        match self {
            Json::Array(items) => {
                out.push('[');
                out.push('\n');
                if items.is_empty() {
                    out.push('\n');
                }
                for (index, item) in items.iter().enumerate() {
                    indent(out, depth + 1);
                    item.write_pretty(out, depth + 1);
                    if index + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(out, depth);
                out.push(']');
            }
            Json::Object(fields) => {
                out.push('{');
                out.push('\n');
                if fields.is_empty() {
                    out.push('\n');
                }
                for (index, (key, value)) in fields.iter().enumerate() {
                    indent(out, depth + 1);
                    write_string(out, key, false);
                    out.push_str(" : ");
                    value.write_pretty(out, depth + 1);
                    if index + 1 < fields.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(out, depth);
                out.push('}');
            }
            _ => self.write_scalar(out, false),
        }
    }

    fn write_compact(&self, out: &mut String) {
        match self {
            Json::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write_compact(out);
                }
                out.push(']');
            }
            Json::Object(fields) => {
                out.push('{');
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(out, key, true);
                    out.push(':');
                    value.write_compact(out);
                }
                out.push('}');
            }
            _ => self.write_scalar(out, true),
        }
    }

    fn write_scalar(&self, out: &mut String, escape_slashes: bool) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(v) => out.push_str(&json_number(*v)),
            Json::String(s) => write_string(out, s, escape_slashes),
            Json::Array(_) | Json::Object(_) => unreachable!(),
        }
    }
}

impl From<f64> for Json {
    fn from(v: f64) -> Json {
        Json::Number(v)
    }
}
impl From<i64> for Json {
    fn from(v: i64) -> Json {
        Json::Number(v as f64)
    }
}
impl From<bool> for Json {
    fn from(v: bool) -> Json {
        Json::Bool(v)
    }
}
impl From<&str> for Json {
    fn from(v: &str) -> Json {
        Json::String(v.to_string())
    }
}
impl From<String> for Json {
    fn from(v: String) -> Json {
        Json::String(v)
    }
}

impl fmt::Display for Json {
    /// The sidecar's pretty form, without its final newline.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        f.write_str(&out)
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, s: &str, escape_slashes: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' if escape_slashes => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The shortest digits that read back as `v`, and the power of ten of the
/// first: 1234.5 is ("12345", 3).
fn shortest_digits(v: f64) -> (String, i32) {
    // Rust's `{:e}` gives the shortest round-trip digits, as Swift's does,
    // except that of two as near Rust takes the upper and Swift the even.
    let split = |text: String| {
        let (mantissa, exponent) = text.split_once('e').expect("exponent form");
        let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        (digits, exponent.parse::<i32>().expect("exponent"))
    };
    let (digits, exponent) = split(format!("{:e}", v.abs()));
    let n = digits.len();
    let last = digits.as_bytes()[n - 1] - b'0';
    if last % 2 == 1 {
        // A tie: the exact value is halfway between the digits and the
        // ones below them. Doubles have at most 767 significant digits.
        let (exact, exact_exponent) = split(format!("{:.800e}", v.abs()));
        let below = format!("{}{}", &digits[..n - 1], last - 1);
        let tie = exact_exponent == exponent
            && exact[..n] == below
            && exact.as_bytes()[n] == b'5'
            && exact[n + 1..].bytes().all(|b| b == b'0');
        if tie && format!("{below}e{}", exponent - n as i32 + 1).parse::<f64>() == Ok(v.abs()) {
            return (below, exponent);
        }
    }
    (digits, exponent)
}

/// `Double.description` in Swift: shortest round-trip digits, in decimal
/// from 10^-4 up to 2^53 and in exponent form (`1e-05`, `1.5e+16`)
/// outside that; whole numbers end in `.0`.
pub fn swift_double_description(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let sign = if v.is_sign_negative() { "-" } else { "" };
    if v == 0.0 {
        return format!("{sign}0.0");
    }
    let (digits, exponent) = shortest_digits(v);
    if exponent < -4 || v.abs() > 9007199254740992.0 {
        let (first, rest) = digits.split_at(1);
        let fraction = if rest.is_empty() { String::new() } else { format!(".{rest}") };
        let esign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{first}{fraction}e{esign}{:02}", exponent.abs());
    }
    if exponent < 0 {
        let zeros = "0".repeat((-exponent - 1) as usize);
        return format!("{sign}0.{zeros}{digits}");
    }
    let point = exponent as usize + 1;
    if digits.len() <= point {
        format!("{sign}{digits}{}.0", "0".repeat(point - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..point], &digits[point..])
    }
}

/// A number as `JSONEncoder` writes it: `Double.description` without a
/// trailing `.0`.
pub fn json_number(v: f64) -> String {
    let mut text = swift_double_description(v);
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    text
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn error(&self, reason: &'static str) -> ParseError {
        ParseError { offset: self.i, reason }
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.s.get(self.i) {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Result<Json, ParseError> {
        if self.s[self.i..].starts_with(word) {
            self.i += word.len();
            Ok(value)
        } else {
            Err(self.error("unexpected character"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > 512 {
            return Err(self.error("too deeply nested"));
        }
        match self.peek() {
            None => Err(self.error("unexpected end")),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.i += 1;
        let mut fields = BTreeMap::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':'"));
            }
            self.i += 1;
            self.ws();
            let value = self.value(depth + 1)?;
            fields.entry(key).or_insert(value);
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    self.ws();
                    if self.peek() == Some(b'}') {
                        self.i += 1;
                        return Ok(Json::Object(fields));
                    }
                }
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Object(fields));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        return Ok(Json::Array(items));
                    }
                }
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let digits = self.s.get(self.i..self.i + 4).ok_or_else(|| self.error("short \\u escape"))?;
        let text = std::str::from_utf8(digits).map_err(|_| self.error("bad \\u escape"))?;
        let value = u32::from_str_radix(text, 16).map_err(|_| self.error("bad \\u escape"))?;
        self.i += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while let Some(&b) = self.s.get(self.i) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.i += 1;
            }
            // Slices between ASCII bytes of valid UTF-8 are valid UTF-8.
            out.push_str(std::str::from_utf8(&self.s[start..self.i]).expect("UTF-8"));
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let escape = self.peek().ok_or_else(|| self.error("unterminated string"))?;
                    self.i += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                if self.s.get(self.i..self.i + 2) != Some(b"\\u") {
                                    return Err(self.error("lone surrogate"));
                                }
                                self.i += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err(self.error("lone surrogate"));
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&first) {
                                return Err(self.error("lone surrogate"));
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).ok_or_else(|| self.error("bad \\u escape"))?);
                        }
                        _ => return Err(self.error("bad escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in a string")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        let digits = |p: &mut Self| {
            let from = p.i;
            while let Some(b'0'..=b'9') = p.peek() {
                p.i += 1;
            }
            p.i - from
        };
        let int_start = self.i;
        let whole = digits(self);
        if whole == 0 {
            return Err(self.error("bad number"));
        }
        if whole > 1 && self.s[int_start] == b'0' {
            return Err(self.error("leading zero"));
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if digits(self) == 0 {
                return Err(self.error("bad number"));
            }
        }
        let mantissa_end = self.i;
        if let Some(b'e' | b'E') = self.peek() {
            self.i += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.i += 1;
            }
            if digits(self) == 0 {
                return Err(self.error("bad number"));
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).expect("ASCII");
        let value: f64 = text.parse().map_err(|_| self.error("bad number"))?;
        let nonzero = self.s[start..mantissa_end].iter().any(|b| matches!(b, b'1'..=b'9'));
        if value.is_infinite() || (value == 0.0 && nonzero) {
            return Err(ParseError { offset: start, reason: "number out of range" });
        }
        Ok(Json::Number(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_swift_writes_them() {
        let cases = [
            (1.0, "1"),
            (0.1, "0.1"),
            (-0.0, "-0"),
            (1e20, "1e+20"),
            (123456789012345678.0, "1.2345678901234568e+17"),
            (1e-7, "1e-07"),
            (1e-6, "1e-06"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (1e15, "1000000000000000"),
            (1e16, "1e+16"),
            (9007199254740992.0, "9007199254740992"),
            (9007199254740994.0, "9.007199254740994e+15"),
            (1000000000000000.5, "1000000000000000.5"),
            (0.0001, "0.0001"),
            (9.9999e-5, "9.9999e-05"),
            (2.5e-5, "2.5e-05"),
            (-40.0, "-40"),
            (0.30000000000000004, "0.30000000000000004"),
            (12345678.9, "12345678.9"),
            // Halfway between two shortest forms: Swift takes the even one.
            (1072269968822823.25, "1072269968822823.2"),
        ];
        for (value, text) in cases {
            assert_eq!(json_number(value), text, "{value:e}");
        }
        assert_eq!(swift_double_description(1731859200.0), "1731859200.0");
        assert_eq!(swift_double_description(1731859200.25), "1731859200.25");
    }

    #[test]
    fn writes_as_json_encoder() {
        let value = Json::parse(br#"{"b": [], "a": {"z": {}, "y": [1, "x/y"]}, "c": "q\"\\\n\u0001\u007f"}"#).unwrap();
        let expected = "{\n  \"a\" : {\n    \"y\" : [\n      1,\n      \"x/y\"\n    ],\n    \"z\" : {\n\n    }\n  },\n  \"b\" : [\n\n  ],\n  \"c\" : \"q\\\"\\\\\\n\\u0001\u{7f}\"\n}\n";
        assert_eq!(String::from_utf8(value.to_sidecar_bytes()).unwrap(), expected);
        assert_eq!(value.to_compact_sorted(), "{\"a\":{\"y\":[1,\"x\\/y\"],\"z\":{}},\"b\":[],\"c\":\"q\\\"\\\\\\n\\u0001\u{7f}\"}");
    }

    #[test]
    fn reads_as_json_decoder() {
        assert_eq!(Json::parse(b"\xef\xbb\xbf{\"a\":1}").unwrap(), Json::from_pairs([("a", Json::Number(1.0))]));
        assert_eq!(Json::parse(br#"{"a":1,"a":2}"#).unwrap().get("a"), &Json::Number(1.0), "the first wins");
        assert_eq!(Json::parse(b"[1,]").unwrap(), Json::Array(vec![Json::Number(1.0)]));
        assert_eq!(Json::parse(br#"{"a":1,}"#).unwrap().get("a"), &Json::Number(1.0));
        assert_eq!(Json::parse(br#""\ud83d\ude00\/""#).unwrap(), Json::string("😀/"));
        for bad in [&b"{\"a\":1e400}"[..], b"{\"a\":01}", b"{\"a\":\"\\ud800\"}", b"{\"a\":1} x", b"\"a\tb\"", b"1.", b"-", b"\"\xff\"", b"1e-400", b""] {
            assert!(Json::parse(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn overlay_changes_and_unknown() {
        let base = Json::parse(br#"{"a": {"x": 0.10000000149011612, "y": [1, 2, 3], "z": null}, "b": true, "c": "auto"}"#).unwrap();
        let edited = base.overlaid(&Json::parse(br#"{"a": {"x": 0.1}, "d": 1}"#).unwrap());
        let changes = edited.changes_from(&base).unwrap();
        assert_eq!(changes, Json::parse(br#"{"a": {"x": 0.1}, "d": 1}"#).unwrap());
        assert_eq!(base.overlaid(&changes), edited);
        assert_eq!(base.changes_from(&base), None);
        let stored = Json::parse(br#"{"a": {"x": 1, "new": 2}, "future": {"k": 1}}"#).unwrap();
        assert_eq!(stored.unknown_to(&[&base]), Some(Json::parse(br#"{"a": {"new": 2}, "future": {"k": 1}}"#).unwrap()));
    }

    #[test]
    fn restoring_keeps_new_defaults_and_drops_removed_values() {
        let defaults = Json::parse(br#"{"a": 1, "b": {"c": 2, "d": [1, 2]}, "n": 5}"#).unwrap();
        let saved = Json::parse(br#"{"a": 3, "gone": 1, "b": {"c": "text", "d": [7, 8]}, "n": null}"#).unwrap();
        let restored = defaults.restoring(&saved, &[]);
        assert_eq!(restored, Json::parse(br#"{"a": 3, "b": {"c": 2, "d": [7, 8]}, "n": 5}"#).unwrap());
        let restored = defaults.restoring(&saved, &["n".to_string()]);
        assert_eq!(restored.get("n"), &Json::Null);
    }
}
