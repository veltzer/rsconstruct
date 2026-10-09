//! Source text as characters: luacheck's `decoder` and `unicode` modules.
//!
//! luacheck decodes a source as UTF-8 when it holds a non-ASCII byte and
//! decodes cleanly, and as Latin-1 otherwise, so offsets and columns count
//! characters, not bytes. Indexes here are 1-based character indexes, as in
//! the Lua original; byte positions returned by `find` are 1-based too.

use std::sync::OnceLock;

use super::pattern::{self, Match, PatternError};

/// A decoded source.
pub struct Chars {
    bytes: Vec<u8>,
    /// Present only for a UTF-8 source with non-ASCII characters.
    unicode: Option<Unicode>,
}

struct Unicode {
    codepoints: Vec<u32>,
    /// `byte_offsets[i]` is the 1-based byte offset of character `i + 1`;
    /// one extra entry points one byte past the end.
    byte_offsets: Vec<usize>,
}

/// Decodes UTF-8 the way luacheck does (lenient: no overlong or surrogate
/// checks). `None` on a decoding error.
fn codepoints_and_byte_offsets(bytes: &[u8]) -> Option<Unicode> {
    let mut codepoints = Vec::new();
    let mut byte_offsets = Vec::new();
    let mut i = 0usize;
    let byte = |i: usize| bytes.get(i).map_or(0i64, |&b| i64::from(b));
    loop {
        byte_offsets.push(i + 1);
        let Some(&first) = bytes.get(i) else {
            return Some(Unicode {
                codepoints,
                byte_offsets,
            });
        };
        let mut codepoint = i64::from(first);
        i += 1;
        if codepoint >= 0x80 {
            if codepoint < 0xC0 {
                return None;
            }
            let mut cont = byte(i) - 0x80;
            if !(0..0x40).contains(&cont) {
                return None;
            }
            i += 1;
            if codepoint < 0xE0 {
                codepoint = cont + (codepoint - 0xC0) * 0x40;
            } else if codepoint < 0xF0 {
                codepoint = cont + (codepoint - 0xE0) * 0x40;
                cont = byte(i) - 0x80;
                if !(0..0x40).contains(&cont) {
                    return None;
                }
                i += 1;
                codepoint = cont + codepoint * 0x40;
            } else if codepoint < 0xF8 {
                codepoint = cont + (codepoint - 0xF0) * 0x40;
                for _ in 0..2 {
                    cont = byte(i) - 0x80;
                    if !(0..0x40).contains(&cont) {
                        return None;
                    }
                    i += 1;
                    codepoint = cont + codepoint * 0x40;
                }
                if codepoint > 0x0010_FFFF {
                    return None;
                }
            } else {
                return None;
            }
        }
        codepoints.push(codepoint as u32);
    }
}

/// `string.sub(s, i, j)` for positive or negative 1-based indexes.
pub fn lua_sub(s: &[u8], i: i64, j: i64) -> &[u8] {
    let len = s.len() as i64;
    let mut i = if i < 0 { (len + i + 1).max(0) } else { i };
    let mut j = if j < 0 { len + j + 1 } else { j };
    if i < 1 {
        i = 1;
    }
    if j > len {
        j = len;
    }
    if i > j {
        return &[];
    }
    &s[(i - 1) as usize..j as usize]
}

fn printability_boundaries() -> &'static [u32] {
    static BOUNDARIES: OnceLock<Vec<u32>> = OnceLock::new();
    BOUNDARIES.get_or_init(|| super::tables().printability_boundaries.clone())
}

/// luacheck's `unicode.is_printable`: a binary search over the boundaries of
/// alternating non-printable / printable blocks (the first is non-printable).
pub fn is_printable(codepoint: u32) -> bool {
    let boundaries = printability_boundaries();
    let mut floor_boundary_index = None;
    // 1-based indexes, as in the original.
    let mut begin_index = 1usize;
    let mut end_index = boundaries.len() + 1;
    while end_index - begin_index > 1 {
        let mid_index = usize::midpoint(begin_index, end_index);
        let mid_codepoint = boundaries[mid_index - 1];
        match codepoint.cmp(&mid_codepoint) {
            std::cmp::Ordering::Less => end_index = mid_index,
            std::cmp::Ordering::Greater => begin_index = mid_index,
            std::cmp::Ordering::Equal => {
                floor_boundary_index = Some(mid_index);
                break;
            }
        }
    }
    floor_boundary_index.unwrap_or(begin_index) % 2 == 0
}

impl Chars {
    pub fn decode(bytes: Vec<u8>) -> Self {
        let unicode = if bytes.iter().any(|&b| b >= 128) {
            codepoints_and_byte_offsets(&bytes)
        } else {
            None
        };
        Self { bytes, unicode }
    }

    /// The codepoint at a 1-based character index, `None` out of range.
    pub fn get_codepoint(&self, index: usize) -> Option<u32> {
        if index == 0 {
            return None;
        }
        match &self.unicode {
            Some(u) => u.codepoints.get(index - 1).copied(),
            None => self.bytes.get(index - 1).map(|&b| u32::from(b)),
        }
    }

    pub const fn get_length(&self) -> usize {
        match &self.unicode {
            Some(u) => u.codepoints.len(),
            None => self.bytes.len(),
        }
    }

    /// The bytes of characters `from..=to`.
    pub fn get_substring(&self, from: i64, to: i64) -> Vec<u8> {
        match &self.unicode {
            None => lua_sub(&self.bytes, from, to).to_vec(),
            Some(u) => {
                // byte_offsets[from] .. byte_offsets[to + 1] - 1, 1-based.
                let start = u.byte_offsets[(from - 1) as usize] as i64;
                let end = u.byte_offsets[to as usize] as i64 - 1;
                lua_sub(&self.bytes, start, end).to_vec()
            }
        }
    }

    /// Like `get_substring`, with characters that are not printable escaped
    /// (`\xNN`, or `\u{N}` above 255).
    pub fn get_printable_substring(&self, from: i64, to: i64) -> Vec<u8> {
        match &self.unicode {
            None => {
                let mut out = Vec::new();
                for &b in lua_sub(&self.bytes, from, to) {
                    if (32..=126).contains(&b) {
                        out.push(b);
                    } else {
                        out.extend_from_slice(format!("\\x{b:02X}").as_bytes());
                    }
                }
                out
            }
            Some(u) => {
                let mut out = Vec::new();
                let mut index = from;
                while index <= to {
                    let codepoint = u.codepoints[(index - 1) as usize];
                    if is_printable(codepoint) {
                        out.extend(self.get_substring(index, index));
                    } else if codepoint > 255 {
                        out.extend_from_slice(format!("\\u{{{codepoint:X}}}").as_bytes());
                    } else {
                        out.extend_from_slice(format!("\\x{codepoint:02X}").as_bytes());
                    }
                    index += 1;
                }
                out
            }
        }
    }

    /// `string.find` from a 1-based character index; the result is in bytes.
    pub fn find(&self, pattern: &[u8], from: usize) -> Result<Option<Match>, PatternError> {
        let init = match &self.unicode {
            None => from,
            Some(u) => u.byte_offsets[from - 1],
        };
        pattern::find(&self.bytes, pattern, init as i64)
    }
}

/// `decoder.decode(s)` followed by `get_printable_substring(1, length)`.
pub fn printable(bytes: &[u8]) -> Vec<u8> {
    let chars = Chars::decode(bytes.to_vec());
    chars.get_printable_substring(1, chars.get_length() as i64)
}
