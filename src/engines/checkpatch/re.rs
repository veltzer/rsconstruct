//! Perl's interpolation into patterns. checkpatch builds most of its regular expressions from
//! `qr//` variables (`$Ident`, `$Type`, ...); the port keeps each pattern
//! as written in checkpatch.pl and interpolates the variables the way Perl
//! does: a `qr//` value is `(?^flags:source)`, `\Q...\E` quotes what it
//! interpolates, `$;` is the subscript separator byte.

use super::perlre::{Captures, quotemeta};

pub use super::perlre::quotemeta as quotemeta_bytes;

/// `$;`, which `sanitise_line` writes over comments.
pub const SUBSEP: u8 = 0x1C;

/// Interpolate `$name`, `${name}` and `\Q...\E` in a pattern template.
pub fn interp(template: &[u8], lookup: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(template.len() * 2);
    let mut quote_from: Option<usize> = None;
    // `\L` / `\U`: where case folding started, and whether to upper.
    let mut case_from: Option<(usize, bool)> = None;
    let t = template;
    let mut i = 0;
    while i < t.len() {
        let c = t[i];
        if c == b'\\' && i + 1 < t.len() {
            match t[i + 1] {
                b'Q' => {
                    quote_from = Some(out.len());
                    i += 2;
                    continue;
                }
                b'L' | b'U' => {
                    case_from = Some((out.len(), t[i + 1] == b'U'));
                    i += 2;
                    continue;
                }
                b'E' => {
                    if let Some(from) = quote_from.take() {
                        let quoted = quotemeta(&out[from..]);
                        out.truncate(from);
                        out.extend(quoted);
                    }
                    if let Some((from, upper)) = case_from.take() {
                        fold_case(&mut out[from..], upper);
                    }
                    i += 2;
                    continue;
                }
                _ => {
                    // Inside \Q a backslash escape is the character.
                    if quote_from.is_none() {
                        out.push(c);
                    }
                    out.push(t[i + 1]);
                    i += 2;
                    continue;
                }
            }
        }
        if c == b'$' && i + 1 < t.len() {
            if t[i + 1] == b';' {
                out.push(SUBSEP);
                i += 2;
                continue;
            }
            let (name, next) = if t[i + 1] == b'{' {
                match t[i + 2..].iter().position(|&b| b == b'}') {
                    Some(end) => (&t[i + 2..i + 2 + end], i + 3 + end),
                    None => (&t[..0], i + 1),
                }
            } else {
                let len = t[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                    .count();
                (&t[i + 1..i + 1 + len], i + 1 + len)
            };
            if !name.is_empty() && !name[0].is_ascii_digit()
                || name.iter().all(u8::is_ascii_digit) && !name.is_empty()
            {
                let key = String::from_utf8_lossy(name);
                match lookup(&key) {
                    Some(value) => out.extend(value),
                    None => panic!("checkpatch pattern interpolates unknown ${key}"),
                }
                i = next;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    if let Some(from) = quote_from {
        let quoted = quotemeta(&out[from..]);
        out.truncate(from);
        out.extend(quoted);
    }
    if let Some((from, upper)) = case_from {
        fold_case(&mut out[from..], upper);
    }
    out
}

const fn fold_case(s: &mut [u8], upper: bool) {
    if upper {
        s.make_ascii_uppercase();
    } else {
        s.make_ascii_lowercase();
    }
}

/// A `qr//` value as Perl stringifies it.
pub fn qr(source: &[u8], flags: &str) -> Vec<u8> {
    let mut out = format!("(?^{flags}:").into_bytes();
    out.extend_from_slice(source);
    out.push(b')');
    out
}

/// A match and the text it was made on.
pub struct Caps<'a> {
    pub c: Captures,
    pub s: &'a [u8],
}

impl<'a> Caps<'a> {
    /// `$i`, `None` when undefined.
    pub fn get(&self, i: usize) -> Option<&'a [u8]> {
        self.c.text(self.s, i)
    }

    /// `$i` interpolated: empty when undefined.
    pub fn s(&self, i: usize) -> Vec<u8> {
        self.c.str(self.s, i)
    }

    pub fn defined(&self, i: usize) -> bool {
        self.c.get(i).is_some()
    }

    /// `$-[i]`.
    pub fn start(&self, i: usize) -> Option<usize> {
        self.c.get(i).map(|p| p.0)
    }

    /// `$+[i]`.
    pub fn end(&self, i: usize) -> Option<usize> {
        self.c.get(i).map(|p| p.1)
    }
}
