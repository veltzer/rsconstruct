//! svglint's `valid` rule: fast-xml-parser 5.10.1's `XMLValidator.validate`
//! with its default options (no boolean attributes, no unpaired tags),
//! ported statement for statement. It is a single pass over the text that
//! stops at the first problem and names it the way fast-xml-parser does;
//! svglint prints that message and nothing else, so the message text is
//! what matters here.
//!
//! The scan runs over UTF-16 code units, as the JavaScript does, so that
//! the one message carrying a position ("opened in line L, col C") counts
//! columns the same way.

const LT: u16 = b'<' as u16;
const GT: u16 = b'>' as u16;
const SLASH: u16 = b'/' as u16;
const BANG: u16 = b'!' as u16;
const QUESTION: u16 = b'?' as u16;
const AMP: u16 = b'&' as u16;
const SEMI: u16 = b';' as u16;
const HASH: u16 = b'#' as u16;
const EQ: u16 = b'=' as u16;
const DQ: u16 = b'"' as u16;
const SQ: u16 = b'\'' as u16;
const SPACE: u16 = b' ' as u16;

/// Validate `source`; `None` when it is well-formed, otherwise the message
/// fast-xml-parser reports.
pub fn validate(source: &str) -> Option<String> {
    let mut data: Vec<u16> = source.encode_utf16().collect();
    if data.first() == Some(&0xFEFF) {
        data.remove(0);
    }
    Validator { x: data }.run()
}

struct Validator {
    x: Vec<u16>,
}

struct OpenTag {
    name: String,
    start: usize,
}

/// A position in the text the way `getLineNumberForPosition` computes it:
/// 1-based line and column, columns in UTF-16 units.
fn line_col(x: &[u16], index: usize) -> (usize, usize) {
    let before = &x[..index.min(x.len())];
    let mut line = 1;
    let mut col_start = 0;
    let mut i = 0;
    while i < before.len() {
        if before[i] == u16::from(b'\n') {
            line += 1;
            col_start = i + 1;
        } else if before[i] == u16::from(b'\r') && before.get(i + 1) == Some(&u16::from(b'\n')) {
            line += 1;
            i += 1;
            col_start = i + 1;
        }
        i += 1;
    }
    (line, before.len() - col_start + 1)
}

const fn is_ws(c: u16) -> bool {
    c == SPACE || c == b'\t' as u16 || c == b'\n' as u16 || c == b'\r' as u16
}

/// JavaScript's `\s` and `String.prototype.trim` whitespace.
const fn is_js_space(c: u16) -> bool {
    matches!(
        c,
        0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF
    )
}

fn utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn trim(units: &[u16]) -> &[u16] {
    let start = units
        .iter()
        .position(|&c| !is_js_space(c))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|&c| !is_js_space(c))
        .map_or(start, |p| p + 1);
    &units[start..end.max(start)]
}

/// fast-xml-parser's `isName`: XML Name characters within the BMP.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    is_name_start(first) && chars.all(is_name_char)
}

fn is_name_start(c: char) -> bool {
    let u = c as u32;
    c == ':'
        || c.is_ascii_alphabetic()
        || c == '_'
        || (0xC0..=0xD6).contains(&u)
        || (0xD8..=0xF6).contains(&u)
        || (0xF8..=0x2FF).contains(&u)
        || (0x370..=0x37D).contains(&u)
        || (0x37F..=0x1FFF).contains(&u)
        || (0x200C..=0x200D).contains(&u)
        || (0x2070..=0x218F).contains(&u)
        || (0x2C00..=0x2FEF).contains(&u)
        || (0x3001..=0xD7FF).contains(&u)
        || (0xF900..=0xFDCF).contains(&u)
        || (0xFDF0..=0xFFFD).contains(&u)
}

fn is_name_char(c: char) -> bool {
    let u = c as u32;
    is_name_start(c)
        || c == '-'
        || c == '.'
        || c.is_ascii_digit()
        || u == 0xB7
        || (0x300..=0x36F).contains(&u)
        || (0x203F..=0x2040).contains(&u)
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("{s:?}"))
}

impl Validator {
    fn at(&self, i: usize) -> Option<u16> {
        self.x.get(i).copied()
    }

    fn run(&self) -> Option<String> {
        let x = &self.x;
        let len = x.len();
        let mut tags: Vec<OpenTag> = Vec::new();
        let mut tag_found = false;
        let mut reached_root = false;
        let mut i = 0usize;
        while i < len {
            if x[i] == LT && self.at(i + 1) == Some(QUESTION) {
                i += 2;
                i = match self.read_pi(i) {
                    Ok(i) => i,
                    Err(msg) => return Some(msg),
                };
            } else if x[i] == LT {
                let tag_start = i;
                i += 1;
                if self.at(i) == Some(BANG) {
                    i = self.read_comment_and_cdata(i);
                    i += 1;
                    continue;
                }
                let mut closing = false;
                if self.at(i) == Some(SLASH) {
                    closing = true;
                    i += 1;
                }
                let name_start = i;
                while i < len
                    && x[i] != GT
                    && x[i] != SPACE
                    && x[i] != u16::from(b'\t')
                    && x[i] != u16::from(b'\n')
                    && x[i] != u16::from(b'\r')
                {
                    i += 1;
                }
                let mut name_units = trim(&x[name_start..i]).to_vec();
                if name_units.last() == Some(&SLASH) {
                    name_units.pop();
                    i -= 1;
                }
                let tag_name = utf16(&name_units);
                if !is_name(&tag_name) {
                    if trim(&name_units).is_empty() {
                        return Some("Invalid space after '<'.".to_string());
                    }
                    return Some(format!("Tag '{tag_name}' is an invalid name."));
                }
                let Some((mut attr_str, after, tag_closed)) = self.read_attribute_str(i) else {
                    return Some(format!("Attributes for '{tag_name}' have open quote."));
                };
                i = after;
                if attr_str.last() == Some(&SLASH) {
                    attr_str.pop();
                    if let Some(msg) = validate_attribute_string(&attr_str) {
                        return Some(msg);
                    }
                    tag_found = true;
                } else if closing {
                    if !tag_closed {
                        return Some(format!(
                            "Closing tag '{tag_name}' doesn't have proper closing."
                        ));
                    } else if !trim(&attr_str).is_empty() {
                        return Some(format!(
                            "Closing tag '{tag_name}' can't have attributes or invalid starting."
                        ));
                    } else if tags.is_empty() {
                        return Some(format!("Closing tag '{tag_name}' has not been opened."));
                    }
                    let open = tags.pop().unwrap_or(OpenTag {
                        name: String::new(),
                        start: 0,
                    });
                    if tag_name != open.name {
                        let (line, col) = line_col(x, open.start);
                        return Some(format!(
                            "Expected closing tag '{}' (opened in line {line}, col {col}) instead of closing tag '{tag_name}'.",
                            open.name
                        ));
                    }
                    if tags.is_empty() {
                        reached_root = true;
                    }
                } else {
                    if let Some(msg) = validate_attribute_string(&attr_str) {
                        return Some(msg);
                    }
                    if reached_root {
                        return Some("Multiple possible root nodes found.".to_string());
                    }
                    tags.push(OpenTag {
                        name: tag_name,
                        start: tag_start,
                    });
                    tag_found = true;
                }

                // The text after the tag, with the comments, CDATA sections
                // and processing instructions inside it.
                i += 1;
                while i < len {
                    if x[i] == LT {
                        if self.at(i + 1) == Some(BANG) {
                            i += 1;
                            i = self.read_comment_and_cdata(i);
                            i += 1;
                            continue;
                        } else if self.at(i + 1) == Some(QUESTION) {
                            i += 1;
                            i = match self.read_pi(i) {
                                Ok(i) => i,
                                Err(msg) => return Some(msg),
                            };
                        } else {
                            break;
                        }
                    } else if x[i] == AMP {
                        let after_amp = self.validate_ampersand(i);
                        let Some(after_amp) = after_amp else {
                            return Some("char '&' is not expected.".to_string());
                        };
                        i = after_amp;
                    } else if reached_root && !is_ws(x[i]) {
                        return Some("Extra text at the end".to_string());
                    }
                    i += 1;
                }
                if self.at(i) == Some(LT) {
                    i -= 1;
                }
            } else {
                if is_ws(x[i]) {
                    i += 1;
                    continue;
                }
                return Some(format!("char '{}' is not expected.", utf16(&x[i..=i])));
            }
            i += 1;
        }

        if !tag_found {
            return Some("Start tag expected.".to_string());
        }
        if tags.len() == 1 {
            return Some(format!("Unclosed tag '{}'.", tags[0].name));
        }
        if !tags.is_empty() {
            let names: Vec<String> = tags
                .iter()
                .map(|t| format!("    {}", json_string(&t.name)))
                .collect();
            return Some(format!("Invalid '[{}]' found.", names.join(",")));
        }
        None
    }

    /// `readPI`: skip a processing instruction. `Ok(i)` is the index to
    /// continue from, `Err(msg)` a misplaced XML declaration.
    fn read_pi(&self, mut i: usize) -> Result<usize, String> {
        let start = i;
        while i < self.x.len() {
            let c = self.x[i];
            if c == QUESTION || c == SPACE {
                let name = utf16(&self.x[start..i]);
                if i > 5 && name == "xml" {
                    return Err(
                        "XML declaration allowed only at the start of the document.".to_string()
                    );
                } else if c == QUESTION && self.at(i + 1) == Some(GT) {
                    i += 1;
                    break;
                }
            }
            i += 1;
        }
        Ok(i)
    }

    /// `readCommentAndCDATA`, entered at the `!`.
    fn read_comment_and_cdata(&self, mut i: usize) -> usize {
        let x = &self.x;
        let len = x.len();
        let unit = |b: u8| u16::from(b);
        if len > i + 5 && self.at(i + 1) == Some(unit(b'-')) && self.at(i + 2) == Some(unit(b'-')) {
            i += 3;
            while i < len {
                if x[i] == unit(b'-')
                    && self.at(i + 1) == Some(unit(b'-'))
                    && self.at(i + 2) == Some(GT)
                {
                    i += 2;
                    break;
                }
                i += 1;
            }
        } else if len > i + 8 && x[i + 1..i + 8] == b"DOCTYPE".map(u16::from) {
            let mut depth = 1;
            i += 8;
            while i < len {
                if x[i] == LT {
                    depth += 1;
                } else if x[i] == GT {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
        } else if len > i + 9 && x[i + 1..i + 8] == b"[CDATA[".map(u16::from) {
            i += 8;
            while i < len {
                if x[i] == unit(b']')
                    && self.at(i + 1) == Some(unit(b']'))
                    && self.at(i + 2) == Some(GT)
                {
                    i += 2;
                    break;
                }
                i += 1;
            }
        }
        i
    }

    /// `readAttributeStr`: everything up to the `>` outside quotes. `None`
    /// for an unclosed quote; otherwise the text, the index of the `>` (or
    /// the end), and whether a `>` was found.
    fn read_attribute_str(&self, mut i: usize) -> Option<(Vec<u16>, usize, bool)> {
        let mut attr = Vec::new();
        let mut quote: Option<u16> = None;
        let mut closed = false;
        while i < self.x.len() {
            let c = self.x[i];
            if c == DQ || c == SQ {
                match quote {
                    None => quote = Some(c),
                    Some(q) if q == c => quote = None,
                    Some(_) => {}
                }
            } else if c == GT && quote.is_none() {
                closed = true;
                break;
            }
            attr.push(c);
            i += 1;
        }
        if quote.is_some() {
            return None;
        }
        Some((attr, i, closed))
    }

    /// `validateAmpersand`: the index of the `;` ending a reference (or the
    /// end of the text), `None` when the `&` is not a reference.
    fn validate_ampersand(&self, mut i: usize) -> Option<usize> {
        let x = &self.x;
        i += 1;
        if self.at(i) == Some(SEMI) {
            return None;
        }
        if self.at(i) == Some(HASH) {
            i += 1;
            let hex = self.at(i) == Some(u16::from(b'x'));
            if hex {
                i += 1;
            }
            while i < x.len() {
                if x[i] == SEMI {
                    return Some(i);
                }
                let ok = char::from_u32(u32::from(x[i])).is_some_and(|c| {
                    if hex {
                        c.is_ascii_hexdigit()
                    } else {
                        c.is_ascii_digit()
                    }
                });
                if !ok {
                    break;
                }
                i += 1;
            }
            return None;
        }
        let mut count = 0;
        while i < x.len() {
            let word = char::from_u32(u32::from(x[i]))
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            if word && count < 20 {
                i += 1;
                count += 1;
                continue;
            }
            if x[i] == SEMI {
                break;
            }
            return None;
        }
        Some(i)
    }
}

/// `validateAttributeString`: the attribute text of a tag, scanned with the
/// `(\s*)([^\s=]+)(\s*=)?(\s*(['"])(([\s\S])*?)\5)?` regular expression and
/// checked match by match.
fn validate_attribute_string(attr: &[u16]) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    for m in attr_matches(attr) {
        let name = utf16(&m.name);
        if m.leading_ws == 0 {
            return Some(format!("Attribute '{name}' has no space in starting."));
        } else if m.has_equals && !m.has_value {
            return Some(format!("Attribute '{name}' is without value."));
        } else if !m.has_equals {
            return Some(format!("boolean attribute '{name}' is not allowed."));
        }
        if !is_name(&name) {
            return Some(format!("Attribute '{name}' is an invalid name."));
        }
        if seen.contains(&name) {
            return Some(format!("Attribute '{name}' is repeated."));
        }
        seen.push(name);
    }
    None
}

struct AttrMatch {
    leading_ws: usize,
    name: Vec<u16>,
    has_equals: bool,
    has_value: bool,
}

/// The successive matches of the attribute regular expression, as a global
/// `exec` loop finds them: the leftmost position from which `\s*` reaches a
/// character that is neither whitespace nor `=` starts a match.
fn attr_matches(attr: &[u16]) -> Vec<AttrMatch> {
    let len = attr.len();
    let mut out = Vec::new();
    let mut last = 0usize;
    loop {
        // Find the leftmost start `s >= last` whose whitespace run ends at a
        // name character; a run ending at `=` or the end fails, and the
        // scan resumes after that `=`.
        let mut start = last;
        let name_start;
        loop {
            let mut r = start;
            while r < len && is_js_space(attr[r]) {
                r += 1;
            }
            if r >= len {
                return out;
            }
            if attr[r] == EQ {
                start = r + 1;
                continue;
            }
            name_start = r;
            break;
        }
        let leading_ws = name_start - start;
        let mut i = name_start;
        while i < len && !is_js_space(attr[i]) && attr[i] != EQ {
            i += 1;
        }
        let name = attr[name_start..i].to_vec();
        // `(\s*=)?`
        let mut j = i;
        while j < len && is_js_space(attr[j]) {
            j += 1;
        }
        let mut has_equals = false;
        if j < len && attr[j] == EQ {
            has_equals = true;
            i = j + 1;
        }
        // `(\s*(['"])(([\s\S])*?)\5)?`
        let mut has_value = false;
        let mut k = i;
        while k < len && is_js_space(attr[k]) {
            k += 1;
        }
        if k < len && (attr[k] == DQ || attr[k] == SQ) {
            let q = attr[k];
            if let Some(close) = attr[k + 1..].iter().position(|&c| c == q) {
                has_value = true;
                i = k + 1 + close + 1;
            }
        }
        out.push(AttrMatch {
            leading_ws,
            name,
            has_equals,
            has_value,
        });
        last = i;
    }
}
